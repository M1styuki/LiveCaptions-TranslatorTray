use crate::{
    native::{handle, os_error, wide},
    security::PipeSecurity,
    worker::{Event, EventSink},
};
use protocol::{CHANNEL_CAPACITY, Envelope, Message, PROTOCOL_VERSION, read_frame, write_frame};
use std::{
    collections::VecDeque,
    fs::File,
    io,
    os::windows::io::FromRawHandle,
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
};
use windows::{
    Win32::{
        Security::SECURITY_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX,
        },
        System::Pipes::{
            CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
        },
    },
    core::PCWSTR,
};

#[derive(Default)]
struct QueueState {
    messages: VecDeque<Envelope>,
    connected: bool,
    closed: bool,
}

#[derive(Default)]
struct Outbox {
    state: Mutex<QueueState>,
    changed: Condvar,
}

pub struct Pipe {
    file: windows_platform::pipe_stream::PipeStream,
    outbox: Arc<Outbox>,
    reader: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
}

impl Pipe {
    pub fn create(name: &str) -> io::Result<Self> {
        let security = PipeSecurity::current_logon()?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: security.descriptor().0,
            bInheritHandle: false.into(),
        };
        let name = wide(name);
        // SAFETY: name and ACL allocation outlive CreateNamedPipeW, which copies both.
        // The pipe handle is non-inheritable and uniquely transferred into File.
        let file = unsafe {
            let raw = CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                protocol::MAX_MESSAGE_BYTES as u32,
                protocol::MAX_MESSAGE_BYTES as u32,
                0,
                Some(&attributes),
            );
            if raw.is_invalid() {
                return Err(io::Error::last_os_error());
            }
            windows_platform::pipe_stream::PipeStream::new(File::from_raw_handle(raw.0))?
        };
        Ok(Self {
            file,
            outbox: Arc::default(),
            reader: None,
            writer: None,
        })
    }

    pub fn start(&mut self, worker_pid: u32, events: EventSink) -> io::Result<()> {
        let mut reader = self.file.try_clone()?;
        let mut writer = self.file.try_clone()?;
        let writer_queue = self.outbox.clone();
        let writer_events = events.clone();
        self.writer = Some(
            thread::Builder::new()
                .name("host-pipe-write".into())
                .spawn(move || {
                    loop {
                        let message = {
                            let mut state = writer_queue.state.lock().unwrap();
                            while !state.closed && (!state.connected || state.messages.is_empty()) {
                                state = writer_queue.changed.wait(state).unwrap();
                            }
                            if state.closed {
                                break;
                            }
                            state.messages.pop_front().unwrap()
                        };
                        if let Err(error) = write_frame(&mut writer, &message) {
                            writer_events.post(Event::PipeError(error.to_string()));
                            break;
                        }
                    }
                })?,
        );
        let reader_queue = self.outbox.clone();
        self.reader = Some(thread::Builder::new().name("host-pipe-read".into()).spawn(move || {
            let result = (|| -> io::Result<()> {
                reader.connect()?;
                // SAFETY: the connected server-pipe handle remains owned by reader.
                unsafe {
                    let mut actual_pid = 0;
                    GetNamedPipeClientProcessId(handle(&reader), &mut actual_pid).map_err(os_error)?;
                    if actual_pid != worker_pid { return Err(io::Error::other("Unexpected pipe client")); }
                }
                let ready = read_frame(&mut reader)?.ok_or_else(|| io::Error::other("Worker disconnected before Ready"))?;
                if !matches!(ready.message, Message::Ready(value) if value.worker_pid == worker_pid && value.protocol_version == PROTOCOL_VERSION) {
                    return Err(io::Error::other("Invalid worker Ready message"));
                }
                {
                    let mut state = reader_queue.state.lock().unwrap();
                    state.connected = true;
                    reader_queue.changed.notify_all();
                }
                events.post(Event::Message(ready));
                while let Some(message) = read_frame(&mut reader)? { events.post(Event::Message(message)); }
                Ok(())
            })();
            if let Err(error) = result { events.post(Event::PipeError(error.to_string())); }
        })?);
        Ok(())
    }

    pub fn send(&self, envelope: Envelope) -> io::Result<()> {
        let mut state = self.outbox.state.lock().unwrap();
        if state.closed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Worker pipe closed",
            ));
        }
        if matches!(envelope.message, Message::Exit(_)) {
            // Exit supersedes queued UI/engine actions and always has a queue slot.
            state.messages.clear();
            state.messages.push_front(envelope);
        } else {
            if state.messages.len() >= CHANNEL_CAPACITY {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Worker command queue full",
                ));
            }
            state.messages.push_back(envelope);
        }
        self.outbox.changed.notify_all();
        Ok(())
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        {
            let mut state = self.outbox.state.lock().unwrap();
            state.closed = true;
            self.outbox.changed.notify_all();
        }
        // SAFETY: handles are still owned here. Disconnect invalidates the connection
        // for both ends; cancellation also wakes a pending overlapped Connect.
        unsafe {
            let _ = DisconnectNamedPipe(handle(&self.file));
            self.file.cancel_all();
        }
        if let Some(thread) = self.reader.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.writer.take() {
            let _ = thread.join();
        }
    }
}
