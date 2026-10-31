//! Duplex named-pipe transport with an independent OVERLAPPED/event per stream.
//! Blocking waits run only on pipe threads; a pending read never serializes writes.
use std::{
    fs::File,
    io::{self, Read, Write},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::{ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, HANDLE},
        Storage::FileSystem::{ReadFile, WriteFile},
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Pipes::ConnectNamedPipe,
            Threading::{CreateEventW, ResetEvent},
        },
    },
    core::HRESULT,
};

pub struct PipeStream {
    shared: Arc<Shared>,
    event: OwnedHandle,
}
struct Shared {
    file: File,
    closing: AtomicBool,
}

fn event() -> io::Result<OwnedHandle> {
    // SAFETY: unnamed, non-inheritable, uniquely owned kernel event.
    let handle = unsafe { CreateEventW(None, true, false, None) }.map_err(os_error)?;
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}
fn os_error(error: windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error((error.code().0 as u32 & 0xffff) as i32)
}

impl PipeStream {
    /// # Safety
    /// `file` must be a named-pipe handle opened with FILE_FLAG_OVERLAPPED.
    /// No other code may perform synchronous I/O on the handle.
    pub unsafe fn new(file: File) -> io::Result<Self> {
        Ok(Self {
            shared: Arc::new(Shared {
                file,
                closing: AtomicBool::new(false),
            }),
            event: event()?,
        })
    }
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            shared: self.shared.clone(),
            event: event()?,
        })
    }
    fn overlapped(&self) -> io::Result<OVERLAPPED> {
        if self.shared.closing.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "Pipe is closing"));
        }
        // SAFETY: no operation remains pending when an I/O method returns.
        unsafe { ResetEvent(HANDLE(self.event.as_raw_handle())) }.map_err(os_error)?;
        Ok(OVERLAPPED {
            hEvent: HANDLE(self.event.as_raw_handle()),
            ..Default::default()
        })
    }
    fn finish(
        &self,
        result: windows::core::Result<()>,
        operation: &OVERLAPPED,
    ) -> io::Result<usize> {
        if let Err(error) = result
            && error.code() != HRESULT::from_win32(ERROR_IO_PENDING.0)
        {
            return Err(os_error(error));
        }
        if self.shared.closing.load(Ordering::SeqCst) {
            // Cancellation may have run between the preflight check and I/O
            // submission. Cancel this exact operation, then await completion.
            unsafe {
                let _ = CancelIoEx(HANDLE(self.as_raw_handle()), Some(operation));
            }
        }
        let mut count = 0;
        // SAFETY: caller keeps the operation, event and I/O buffer alive until
        // completion (including cancellation). Every clone has a separate event.
        unsafe { GetOverlappedResult(HANDLE(self.as_raw_handle()), operation, &mut count, true) }
            .map_err(os_error)?;
        Ok(count as usize)
    }
    pub fn connect(&mut self) -> io::Result<()> {
        let mut operation = self.overlapped()?;
        // SAFETY: operation/event remain live until finish waits for completion.
        let result =
            unsafe { ConnectNamedPipe(HANDLE(self.as_raw_handle()), Some(&mut operation)) };
        if result
            .as_ref()
            .is_err_and(|error| error.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0))
        {
            return Ok(());
        }
        self.finish(result, &operation).map(|_| ())
    }
    pub fn cancel_all(&self) {
        self.shared.closing.store(true, Ordering::SeqCst);
        // SAFETY: only requests cancellation. I/O threads keep buffers alive and
        // observe completed cancellation before their stack storage is released.
        unsafe {
            let _ = CancelIoEx(HANDLE(self.as_raw_handle()), None);
        }
    }
}
impl AsRawHandle for PipeStream {
    fn as_raw_handle(&self) -> RawHandle {
        self.shared.file.as_raw_handle()
    }
}
impl Read for PipeStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let mut operation = self.overlapped()?;
        // SAFETY: the mutable buffer cannot be accessed until finish completes.
        let result = unsafe {
            ReadFile(
                HANDLE(self.as_raw_handle()),
                Some(buffer),
                None,
                Some(&mut operation),
            )
        };
        match self.finish(result, &operation) {
            Err(error) if error.raw_os_error() == Some(ERROR_BROKEN_PIPE.0 as i32) => Ok(0),
            value => value,
        }
    }
}
impl Write for PipeStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let mut operation = self.overlapped()?;
        // SAFETY: immutable buffer remains live until finish completes.
        let result = unsafe {
            WriteFile(
                HANDLE(self.as_raw_handle()),
                Some(buffer),
                None,
                Some(&mut operation),
            )
        };
        self.finish(result, &operation)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{
        Win32::{
            Storage::FileSystem::{FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX},
            System::Pipes::{
                CreateNamedPipeW, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
            },
        },
        core::PCWSTR,
    };
    #[test]
    fn cancellation_before_or_during_connect_never_leaves_a_blocked_join() {
        for index in 0..64_usize {
            let name: Vec<_> = format!(
                r"\\.\pipe\LiveCaptionsTray-cancel-test-{}-{index}",
                std::process::id()
            )
            .encode_utf16()
            .chain(Some(0))
            .collect();
            let file = unsafe {
                let handle = CreateNamedPipeW(
                    PCWSTR(name.as_ptr()),
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                    PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    1024,
                    1024,
                    0,
                    None,
                );
                assert!(!handle.is_invalid());
                File::from_raw_handle(handle.0)
            };
            let server = unsafe { PipeStream::new(file) }.unwrap();
            let mut reader = server.try_clone().unwrap();
            if index.is_multiple_of(2) {
                server.cancel_all();
            }
            let (finished, result) = std::sync::mpsc::sync_channel(1);
            let thread = std::thread::spawn(move || {
                let result = reader.connect();
                let _ = finished.send(result);
            });
            if !index.is_multiple_of(2) {
                std::thread::yield_now();
                server.cancel_all();
            }
            assert!(
                result
                    .recv_timeout(std::time::Duration::from_secs(1))
                    .unwrap()
                    .is_err()
            );
            thread.join().unwrap();
        }
    }
}
