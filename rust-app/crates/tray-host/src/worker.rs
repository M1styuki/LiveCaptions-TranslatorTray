use crate::{
    native::{handle, os_error, own, wide},
    pipe::Pipe,
};
use protocol::{CHANNEL_CAPACITY, Envelope};
use std::{
    collections::VecDeque,
    io,
    os::windows::io::OwnedHandle,
    path::Path,
    sync::{Arc, Mutex},
    thread,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom},
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JobObjectExtendedLimitInformation, SetInformationJobObject,
            },
            Threading::{
                CREATE_SUSPENDED, CreateProcessW, GetExitCodeProcess, INFINITE,
                PROCESS_INFORMATION, ResumeThread, STARTUPINFOW, TerminateProcess,
                WaitForSingleObject,
            },
        },
        UI::WindowsAndMessaging::PostMessageW,
    },
    core::{PCWSTR, PWSTR},
};

pub enum Event {
    Message(Envelope),
    PipeError(String),
    Exited(u32),
}

#[derive(Clone)]
pub struct EventSink {
    window: usize,
    events: Arc<Mutex<VecDeque<(u32, Event)>>>,
    generation: u32,
}

impl EventSink {
    pub fn new(window: HWND, events: Arc<Mutex<VecDeque<(u32, Event)>>>, generation: u32) -> Self {
        Self {
            window: window.0 as usize,
            events,
            generation,
        }
    }

    pub fn post(&self, event: Event) {
        let mut queue = self.events.lock().unwrap();
        // State updates can be coalesced. Errors/exit still wake the host when full.
        if queue.len() == CHANNEL_CAPACITY {
            if let Some(index) = queue
                .iter()
                .position(|(_, item)| matches!(item, Event::Message(_)))
            {
                queue.remove(index);
            } else {
                queue.pop_front();
            }
        }
        queue.push_back((self.generation, event));
        drop(queue);
        // SAFETY: only a numeric HWND crosses threads; PostMessage is thread-safe.
        // Queue owns all payloads; a destroyed window simply rejects the wakeup.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.window as *mut _)),
                crate::tray::WM_WORKER_EVENT,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

pub struct Worker {
    job: Option<OwnedHandle>,
    process: OwnedHandle,
    pub pipe: Pipe,
    pub pid: u32,
}

impl Worker {
    pub fn launch(data_dir: &Path, events: EventSink) -> io::Result<Self> {
        let executable = std::env::current_exe()?;
        let mut nonce = [0_u8; 16];
        // SAFETY: buffer is writable for exactly its declared length; system RNG needs no algorithm handle.
        unsafe {
            BCryptGenRandom(None, &mut nonce, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
                .ok()
                .map_err(os_error)?;
        }
        let suffix = nonce
            .iter()
            .map(|value| format!("{value:02x}"))
            .collect::<String>();
        let pipe_name = format!(r"\\.\pipe\LiveCaptionsTray-{suffix}");
        let mut pipe = Pipe::create(&pipe_name)?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // Own only the worker. System captions launched by it must not enter this job.
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
        // SAFETY: job is newly allocated, non-inheritable, and exclusively owned here.
        let job = unsafe {
            let job = own(CreateJobObjectW(None, None).map_err(os_error)?);
            SetInformationJobObject(
                handle(&job),
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of_val(&limits) as u32,
            )
            .map_err(os_error)?;
            job
        };
        let app_name = wide(&executable);
        let mut command = wide(format!(
            "{} --worker --pipe {} --host-pid {} --data-dir {}",
            quote(&executable.to_string_lossy()),
            quote(&pipe_name),
            std::process::id(),
            quote(&data_dir.to_string_lossy())
        ));
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut process_info = PROCESS_INFORMATION::default();
        // SAFETY: all argument buffers remain live through CreateProcess; no handles
        // are inherited. Suspension prevents the worker escaping before job assignment.
        let (process, thread_handle) = unsafe {
            CreateProcessW(
                PCWSTR(app_name.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_SUSPENDED,
                None,
                None,
                &startup,
                &mut process_info,
            )
            .map_err(os_error)?;
            (own(process_info.hProcess), own(process_info.hThread))
        };
        // SAFETY: the suspended process and primary thread are exclusively owned here.
        unsafe {
            if let Err(error) = AssignProcessToJobObject(handle(&job), handle(&process)) {
                let _ = TerminateProcess(handle(&process), 1);
                return Err(os_error(error));
            }
            if ResumeThread(handle(&thread_handle)) == u32::MAX {
                return Err(io::Error::last_os_error());
            }
        }
        let watcher = process.try_clone()?;
        let watcher_events = events.clone();
        thread::Builder::new()
            .name("host-worker-wait".into())
            .spawn(move || {
                // SAFETY: this thread owns a duplicated process handle until wait completes.
                let code = unsafe {
                    WaitForSingleObject(handle(&watcher), INFINITE);
                    let mut code = 1;
                    let _ = GetExitCodeProcess(handle(&watcher), &mut code);
                    code
                };
                watcher_events.post(Event::Exited(code));
            })?;
        pipe.start(process_info.dwProcessId, events)?;
        Ok(Self {
            job: Some(job),
            process,
            pipe,
            pid: process_info.dwProcessId,
        })
    }

    pub fn terminate(&self) {
        // SAFETY: exact process handle created by this host, never a name/PID lookup.
        unsafe {
            let _ = TerminateProcess(handle(&self.process), 1);
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Close the job before Pipe::drop joins I/O threads, so a stuck peer cannot
        // retain them. Normal exit already saved; forced exit is managed by host timers.
        drop(self.job.take());
    }
}

fn quote(argument: &str) -> String {
    let mut output = String::from("\"");
    let mut slashes = 0;
    for character in argument.chars() {
        if character == '\\' {
            slashes += 1;
            continue;
        }
        if character == '"' {
            output.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
        } else {
            output.extend(std::iter::repeat_n('\\', slashes));
        }
        slashes = 0;
        output.push(character);
    }
    output.extend(std::iter::repeat_n('\\', slashes * 2));
    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use super::quote;
    #[test]
    fn windows_arguments_preserve_spaces_quotes_and_trailing_slashes() {
        assert_eq!(quote(r"C:\some folder\"), "\"C:\\some folder\\\\\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote(""), "\"\"");
    }
}
