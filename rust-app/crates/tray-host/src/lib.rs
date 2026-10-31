mod menu;
mod native;
mod pipe;
mod security;
mod single_instance;
mod tray;
mod worker;

use native::{os_error, wide};
use protocol::{Empty, EngineState, Envelope, Message, WindowState};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, RemoteDesktop::ProcessIdToSessionId},
        UI::WindowsAndMessaging::*,
    },
    core::{PCWSTR, w},
};
use worker::{Event, EventSink, Worker};

const READY_TIMER: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostState {
    Idle,
    Launching,
    Active,
    Stopping,
    Exiting,
}

struct Host {
    window: HWND,
    tray: Option<tray::Tray>,
    worker: Option<Worker>,
    events: Arc<Mutex<VecDeque<(u32, Event)>>>,
    generation: u32,
    state: HostState,
    engine: EngineState,
    hidden: bool,
    next_request: u64,
    error_reported: bool,
    data_dir: PathBuf,
    taskbar_created: u32,
    worker_saving: bool,
    exit_timer: Option<usize>,
    next_timer: usize,
}

impl Host {
    fn busy(&self) -> bool {
        self.worker_saving
            || matches!(
                self.state,
                HostState::Launching | HostState::Stopping | HostState::Exiting
            )
    }

    fn refresh(&mut self) {
        let running = self.engine == EngineState::Running;
        let busy = self.busy();
        if let Some(tray) = &mut self.tray {
            tray.update(running, busy);
        }
    }

    fn error(&mut self, message: &str) {
        if !self.error_reported {
            if let Some(tray) = &mut self.tray {
                tray.error(message);
            }
            self.error_reported = true;
        }
    }

    fn command(&mut self, command: Message) {
        if self.worker_saving && self.state != HostState::Exiting {
            self.error("正在保存并退出，请稍后再操作。");
            return;
        }
        if self.state == HostState::Exiting {
            return;
        }
        if self.worker.is_none() {
            self.generation = self.generation.wrapping_add(1);
            self.error_reported = false;
            let sink = EventSink::new(self.window, self.events.clone(), self.generation);
            match Worker::launch(&self.data_dir, sink) {
                Ok(worker) => {
                    self.worker = Some(worker);
                    self.worker_saving = false;
                    self.state = HostState::Launching;
                    // SAFETY: startup-only timer; removed on Ready, failure or process exit.
                    unsafe {
                        SetTimer(Some(self.window), READY_TIMER, 10_000, None);
                    }
                }
                Err(error) => {
                    self.error(&error.to_string());
                    return;
                }
            }
        }
        self.next_request += 1;
        if let Some(worker) = &self.worker
            && let Err(error) = worker.pipe.send(Envelope::new(self.next_request, command))
        {
            self.error(&error.to_string());
        }
        self.refresh();
    }

    fn toggle(&mut self) {
        if self.busy() {
            return;
        }
        if self.engine == EngineState::Running {
            self.state = HostState::Stopping;
            self.command(Message::StopEngine(Empty {}));
        } else {
            self.command(Message::StartEngine(WindowState {
                live_captions_hidden: self.hidden,
            }));
        }
    }

    fn show_settings(&mut self) {
        self.command(Message::ShowSettings(WindowState {
            live_captions_hidden: self.hidden,
        }));
    }

    fn exit(&mut self) {
        if self.state == HostState::Exiting {
            return;
        }
        self.state = HostState::Exiting;
        if let Some(worker) = &self.worker {
            self.next_request += 1;
            let _ = worker
                .pipe
                .send(Envelope::new(self.next_request, Message::Exit(Empty {})));
            // SAFETY: one-shot graceful-exit deadline; never an idle polling timer.
            if !self.worker_saving {
                self.next_timer = self.next_timer.wrapping_add(1).max(2);
                self.exit_timer = Some(self.next_timer);
                unsafe {
                    SetTimer(Some(self.window), self.next_timer, 5_000, None);
                }
            }
        } else {
            // SAFETY: this is our UI-thread-owned hidden window.
            unsafe {
                let _ = DestroyWindow(self.window);
            }
        }
        self.refresh();
    }

    fn drain_events(&mut self) {
        let events: Vec<_> = self.events.lock().unwrap().drain(..).collect();
        for (generation, event) in events {
            if generation != self.generation {
                continue;
            }
            match event {
                Event::Message(envelope) if self.worker.is_some() => match envelope.message {
                    Message::Ready(ready) => {
                        if self
                            .worker
                            .as_ref()
                            .is_some_and(|worker| worker.pid == ready.worker_pid)
                        {
                            // SAFETY: cancel the startup deadline for this exact launch.
                            unsafe {
                                let _ = KillTimer(Some(self.window), READY_TIMER);
                            }
                            if self.state != HostState::Exiting {
                                self.state = HostState::Active;
                            }
                        }
                    }
                    Message::EngineStateChanged { state } => {
                        self.engine = state;
                        if self.state != HostState::Exiting {
                            self.state =
                                if matches!(state, EngineState::Starting | EngineState::Stopping) {
                                    HostState::Stopping
                                } else {
                                    HostState::Active
                                };
                        }
                    }
                    Message::WindowsChanged {
                        live_captions_hidden,
                        ..
                    } => self.hidden = live_captions_hidden,
                    Message::Error { code, message } => {
                        if code == "SaveFailed" {
                            // SAFETY: a failed final save cancels our own exit
                            // deadline so the live worker can offer recovery.
                            self.cancel_exit_deadline();
                            self.worker_saving = false;
                            self.state = HostState::Active;
                        }
                        self.error(&message);
                    }
                    Message::Exiting(_) => self.saving_started(),
                    Message::Ack(_) => {}
                    _ => self.error("工作进程发来了无效消息。"),
                },
                Event::PipeError(message) => {
                    if self.state != HostState::Exiting {
                        self.error(&message);
                    }
                    if let Some(worker) = &self.worker {
                        worker.terminate();
                    }
                }
                Event::Exited(code) => {
                    // Reader and process-wait threads may report in either order.
                    // Invalidate this launch before any buffered messages arrive.
                    self.generation = self.generation.wrapping_add(1);
                    // SAFETY: remove active deadlines before dropping the exact worker.
                    unsafe {
                        let _ = KillTimer(Some(self.window), READY_TIMER);
                    }
                    self.cancel_exit_deadline();
                    self.worker_saving = false;
                    self.worker.take();
                    self.engine = EngineState::Disabled;
                    if self.state == HostState::Exiting {
                        // SAFETY: host alone owns this hidden window.
                        unsafe {
                            let _ = DestroyWindow(self.window);
                        }
                    } else {
                        self.state = HostState::Idle;
                        if code != 0 {
                            self.error("工作进程已退出。可从托盘重新打开。");
                        }
                    }
                }
                Event::Message(_) => {}
            }
        }
        self.refresh();
    }
    fn cancel_exit_deadline(&mut self) {
        if let Some(timer) = self.exit_timer.take() {
            // SAFETY: this exact timer was created for this host window.
            unsafe {
                let _ = KillTimer(Some(self.window), timer);
            }
        }
    }
    fn saving_started(&mut self) {
        self.worker_saving = true;
        self.cancel_exit_deadline();
    }
    fn exit_deadline(&mut self, timer: usize) -> bool {
        if self.exit_timer != Some(timer) {
            return false;
        }
        self.cancel_exit_deadline();
        self.state == HostState::Exiting && !self.worker_saving
    }
}

pub fn show_error(message: &str) {
    let message = wide(message);
    // SAFETY: string is live throughout this synchronous error dialog.
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            w!("实时字幕翻译"),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub fn run(mut arguments: impl Iterator<Item = std::ffi::OsString>) -> io::Result<()> {
    let mut data_dir = None;
    while let Some(argument) = arguments.next() {
        if argument == "--data-dir" {
            data_dir = Some(PathBuf::from(
                arguments
                    .next()
                    .ok_or_else(|| io::Error::other("缺少数据目录。"))?,
            ));
        } else {
            return Err(io::Error::other("未知启动参数。"));
        }
    }
    let data_dir = match data_dir {
        Some(path) => path,
        None => std::env::current_exe()?
            .parent()
            .ok_or_else(|| io::Error::other("找不到程序目录。"))?
            .join("data"),
    };
    if !data_dir.is_absolute() {
        return Err(io::Error::other("数据目录必须是绝对路径。"));
    }
    let mut session = 0;
    // SAFETY: process ID is this process; output is a live u32.
    unsafe {
        ProcessIdToSessionId(std::process::id(), &mut session).map_err(os_error)?;
    }
    let class_name = wide(format!("LiveCaptionsTray.MessageWindow.{session}"));
    let _instance_guard = match single_instance::acquire(&class_name, session)? {
        single_instance::Instance::Primary(guard) => guard,
        single_instance::Instance::Existing => return Ok(()),
    };
    check_data_directory(&data_dir).map_err(|error| {
        io::Error::other(format!(
            "无法写入数据目录 {}。请将程序放在可写的文件夹中。{error}",
            data_dir.display()
        ))
    })?;
    // SAFETY: retrieves the loaded current module; handle is borrowed for process lifetime.
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.map_err(os_error)?.0);
    let mut host = Box::new(RefCell::new(Host {
        window: HWND::default(),
        tray: None,
        worker: None,
        events: Arc::default(),
        generation: 0,
        state: HostState::Idle,
        engine: EngineState::Disabled,
        hidden: true,
        next_request: 0,
        error_reported: false,
        data_dir,
        // SAFETY: fixed NUL-terminated OS-defined broadcast message name.
        taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        worker_saving: false,
        exit_timer: None,
        next_timer: 1,
    }));
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    // SAFETY: callback is static, class name lives through the loop. Box allocation
    // stays at a stable address until after window destruction and message-loop exit.
    let window = unsafe {
        if RegisterClassW(&class) == 0 {
            return Err(io::Error::last_os_error());
        }
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class_name.as_ptr()),
            w!("实时字幕翻译托盘"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            Some((&mut *host as *mut RefCell<Host>).cast()),
        )
        .map_err(os_error)?
    };
    {
        let mut state = host.borrow_mut();
        state.window = window;
        state.tray = Some(tray::Tray::new(window, instance)?);
    }
    let mut message = MSG::default();
    // SAFETY: GetMessage blocks without timers in idle. All state access stays on
    // this UI thread; RefCell prevents mutable aliasing during Win32 re-entry.
    unsafe {
        loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result == 0 {
                break;
            }
            if result == -1 {
                return Err(io::Error::last_os_error());
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

fn check_data_directory(path: &std::path::Path) -> io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(path)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let probe = path.join(format!(".write-probe-{}-{nonce}.tmp", std::process::id()));
    // create_new ensures no existing data is overwritten, even after a stale run.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)?;
    let result = file
        .write_all(b"LiveCaptionsTray")
        .and_then(|_| file.sync_all());
    drop(file);
    let cleanup = std::fs::remove_file(probe);
    result.and(cleanup)
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: on NCCREATE lparam points to Win32's live CREATESTRUCT. Userdata is
    // the stable RefCell allocation owned by run(), valid until the loop ends.
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        if message == WM_DESTROY {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *const RefCell<Host>;
        if !pointer.is_null()
            && let Ok(mut host) = (*pointer).try_borrow_mut()
        {
            match message {
                tray::WM_SHOW_SETTINGS => host.show_settings(),
                tray::WM_WORKER_EVENT => host.drain_events(),
                tray::WM_TRAY => match tray::notification(lparam.0 as u32) {
                    Some(tray::Notification::Activate) => host.toggle(),
                    Some(tray::Notification::ContextMenu) => {
                        if let Some(tray) = &host.tray {
                            let choice =
                                tray.menu(host.engine == EngineState::Running, host.busy());
                            // TrackPopupMenu pumps a nested loop while Host is
                            // borrowed. Apply any queued worker events now.
                            host.drain_events();
                            match choice {
                                Ok(tray::TOGGLE) => host.toggle(),
                                Ok(tray::SETTINGS) => host.show_settings(),
                                Ok(tray::EXIT) => host.exit(),
                                Ok(_) => {}
                                Err(error) => host.error(&error.to_string()),
                            }
                        }
                    }
                    None => {}
                },
                WM_CLOSE => host.exit(),
                WM_TIMER if wparam.0 == READY_TIMER => {
                    let _ = KillTimer(Some(window), READY_TIMER);
                    host.error("工作进程启动超过 10 秒，请重试。");
                    if let Some(worker) = &host.worker {
                        worker.terminate();
                    }
                }
                WM_TIMER if host.exit_deadline(wparam.0) => {
                    if let Some(worker) = &host.worker {
                        worker.terminate();
                    }
                }
                value if value == host.taskbar_created => {
                    if let Some(tray) = &mut host.tray {
                        let _ = tray.register();
                        host.refresh();
                    }
                }
                _ => return DefWindowProcW(window, message, wparam, lparam),
            }
            return LRESULT(0);
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_exit_invalidates_buffered_state_events_from_the_same_launch() {
        let events: Arc<Mutex<VecDeque<(u32, Event)>>> = Arc::default();
        events.lock().unwrap().extend([
            (7, Event::Exited(0)),
            (
                7,
                Event::Message(Envelope::new(
                    0,
                    Message::EngineStateChanged {
                        state: EngineState::Running,
                    },
                )),
            ),
            (
                7,
                Event::Message(Envelope::new(
                    0,
                    Message::EngineStateChanged {
                        state: EngineState::Starting,
                    },
                )),
            ),
        ]);
        let mut host = Host {
            window: HWND::default(),
            tray: None,
            worker: None,
            events,
            generation: 7,
            state: HostState::Active,
            engine: EngineState::Running,
            hidden: true,
            next_request: 0,
            error_reported: false,
            data_dir: PathBuf::new(),
            taskbar_created: 0,
            worker_saving: false,
            exit_timer: None,
            next_timer: 1,
        };
        host.drain_events();
        assert_eq!(host.state, HostState::Idle);
        assert_eq!(host.engine, EngineState::Disabled);
        assert_ne!(host.generation, 7);
        // The save phase cancels even an already queued timeout. A later exit
        // attempt has a different timer ID, so that stale message cannot kill it.
        host.state = HostState::Exiting;
        host.exit_timer = Some(8);
        host.saving_started();
        assert!(!host.exit_deadline(8));
        host.worker_saving = false;
        host.state = HostState::Active;
        assert!(!host.exit_deadline(8));
        host.state = HostState::Exiting;
        host.exit_timer = Some(9);
        assert!(!host.exit_deadline(8));
        assert!(host.exit_deadline(9));
    }
}
