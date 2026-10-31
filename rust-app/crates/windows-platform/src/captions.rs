//! All UI Automation objects stay inside one MTA thread, including destruction.
use crate::window_state::WindowSnapshot;
#[cfg(test)]
use std::os::windows::io::AsRawHandle;
use std::{
    io,
    path::PathBuf,
    process::{Child, Command},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
#[cfg(test)]
use windows::Win32::{
    Foundation::{HANDLE, WAIT_OBJECT_0},
    System::Threading::WaitForSingleObject,
};
use windows::{
    Win32::{
        Foundation::HWND,
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            Variant::VARIANT,
        },
        UI::{
            Accessibility::{
                CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationCondition,
                IUIAutomationElement, TreeScope_Subtree, UIA_AutomationIdPropertyId,
            },
            WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId},
        },
    },
    core::{Interface, w},
};

pub enum CaptionEvent {
    Ready,
    Text(CaptionSample),
    Fault(String),
}
#[derive(Clone, Debug, Default)]
pub struct CaptionSample {
    pub text: String,
    pub changes: u64,
    pub unchanged: u16,
}
#[derive(Default)]
struct Sampler(CaptionSample, String);
impl Sampler {
    fn observe(&mut self, text: &str) -> Option<CaptionSample> {
        // Match the original sync loop: empty UIA reads during silence are
        // neither source changes nor stability observations.
        if text.trim().is_empty() {
            return None;
        }
        let display_changed = self.0.text != text;
        let source_changed = if display_changed {
            let latest = captions_core::segment::extract(text, 0, 0)?.translation;
            let changed = self.1 != latest;
            self.1 = latest;
            self.0.text = text.into();
            changed
        } else {
            false
        };
        // Translator.SyncLoop counts changes to OriginalCaption after latest
        // extraction, not edits elsewhere in the accumulated UIA transcript.
        if self.0.changes == 0 || source_changed {
            self.0.changes += 1;
            self.0.unchanged = 0;
            Some(self.0.clone())
        } else {
            self.0.unchanged = self.0.unchanged.saturating_add(1).min(51);
            (display_changed || self.0.unchanged == 50).then(|| self.0.clone())
        }
    }
}
pub type CaptionCallback = Arc<dyn Fn(CaptionEvent) + Send + Sync>;

#[derive(Default)]
struct Control {
    stop: bool,
    hidden: bool,
}

pub struct CaptionSession {
    control: Arc<(Mutex<Control>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl CaptionSession {
    pub fn start(hidden: bool, callback: CaptionCallback) -> io::Result<Self> {
        let control = Arc::new((
            Mutex::new(Control {
                stop: false,
                hidden,
            }),
            Condvar::new(),
        ));
        let thread_control = control.clone();
        let thread = thread::Builder::new()
            .name("captions-mta".into())
            .spawn(move || {
                // SAFETY: this new thread has no prior COM apartment. All COM objects
                // are created and dropped in run_mta before this guard uninitializes it.
                let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() };
                match initialized {
                    Ok(()) => {
                        let result = run_mta(&thread_control, &callback);
                        if let Err(error) = result
                            && !stopped(&thread_control)
                        {
                            callback(CaptionEvent::Fault(error.to_string()));
                        }
                        // SAFETY: matches the successful initialization on this same thread.
                        unsafe {
                            CoUninitialize();
                        }
                    }
                    Err(error) => {
                        callback(CaptionEvent::Fault(format!("无法连接系统字幕：{error}")))
                    }
                }
            })?;
        Ok(Self {
            control,
            thread: Some(thread),
        })
    }

    pub fn stop(&mut self) {
        self.control.0.lock().unwrap().stop = true;
        self.control.1.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
    pub fn set_hidden(&self, hidden: bool) {
        self.control.0.lock().unwrap().hidden = hidden;
        self.control.1.notify_all();
    }
}

impl Drop for CaptionSession {
    fn drop(&mut self) {
        self.stop();
    }
}

fn stopped(control: &Arc<(Mutex<Control>, Condvar)>) -> bool {
    control.0.lock().unwrap().stop
}

fn wait(control: &Arc<(Mutex<Control>, Condvar)>, duration: Duration) -> bool {
    let state = control.0.lock().unwrap();
    let (state, _) = control
        .1
        .wait_timeout_while(state, duration, |state| !state.stop)
        .unwrap();
    state.stop
}

struct AttachedWindow {
    hwnd: HWND,
    snapshot: WindowSnapshot,
    owned: Option<Child>,
    ready: bool,
    hidden: bool,
    control: Arc<(Mutex<Control>, Condvar)>,
}

impl Drop for AttachedWindow {
    fn drop(&mut self) {
        // Before the first successful caption-node or explicit ready-marker read,
        // leave the window available for Windows onboarding/consent/language setup,
        // including when attach, COM initialization or cancellation fails.
        if !self.ready {
            return;
        }
        if self.owned.is_some() || stopped(&self.control) {
            // Stop ends the active recognition session, including a ready process
            // reused from an earlier run. Hiding the window is not stopping it.
            self.snapshot.close();
        } else {
            self.snapshot.restore(self.hidden);
        }
    }
}

fn find_window() -> Option<(HWND, u32)> {
    // SAFETY: fixed class name; FindWindow is limited to our interactive desktop.
    let window = unsafe { FindWindowW(w!("LiveCaptionsDesktopWindow"), None) }.ok()?;
    let mut pid = 0;
    // SAFETY: output is a live u32 and HWND was just obtained from Win32.
    unsafe {
        GetWindowThreadProcessId(window, Some(&mut pid));
    }
    if pid == 0 { None } else { Some((window, pid)) }
}

fn attach(control: &Arc<(Mutex<Control>, Condvar)>) -> io::Result<AttachedWindow> {
    // Child drops close our handle, without terminating a still-unready launch.
    let mut pending = None;
    let existing = find_window();
    if existing.is_none() {
        let system = std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("找不到 Windows 系统目录。"))?;
        pending =
            Some(Command::new(PathBuf::from(system).join("System32/LiveCaptions.exe")).spawn()?);
    }
    let start = Instant::now();
    let (hwnd, pid) = loop {
        if stopped(control) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "已停止字幕"));
        }
        if let Some(window) = existing.or_else(find_window) {
            break window;
        }
        if start.elapsed() >= Duration::from_secs(15) {
            return Err(io::Error::other(
                "未能打开 Windows 实时字幕窗口。请使用 Win+Ctrl+L 打开系统字幕后重试。",
            ));
        }
        if wait(control, Duration::from_millis(100)) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "已停止字幕"));
        }
    };
    // A simultaneous independent launch is never treated as our owned process.
    let owned = matching_launch(pending, pid);
    let snapshot = WindowSnapshot::capture(hwnd, pid)?;
    Ok(AttachedWindow {
        hwnd,
        snapshot,
        owned,
        ready: false,
        hidden: false,
        control: control.clone(),
    })
}

fn matching_launch(pending: Option<Child>, pid: u32) -> Option<Child> {
    pending.filter(|child| child.id() == pid)
}

fn find_element(
    root: &IUIAutomationElement,
    condition: &IUIAutomationCondition,
) -> windows::core::Result<Option<IUIAutomationElement>> {
    // FindFirst returns S_OK + null when there is no match. windows-rs represents
    // this as Error(S_OK); absence is a normal pending state, not a COM failure.
    // SAFETY: these interfaces are created and used only on the capture MTA.
    match unsafe { root.FindFirst(TreeScope_Subtree, condition) } {
        Ok(element) => Ok(Some(element)),
        Err(error) if error.code().is_ok() => Ok(None),
        Err(error) => Err(error),
    }
}

fn run_mta(control: &Arc<(Mutex<Control>, Condvar)>, callback: &CaptionCallback) -> io::Result<()> {
    let mut window = attach(control)?;
    // SAFETY: COM apartment is initialized on this thread; returned objects never
    // cross threads and are dropped before run_mta returns.
    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
            .map_err(win_error)?;
    let timeouts: IUIAutomation2 = automation.cast().map_err(win_error)?;
    // SAFETY: same MTA owns this interface. Bound provider calls so stop does not
    // inherit UIA's long default timeout when the system window stops responding.
    unsafe {
        timeouts.SetConnectionTimeout(1_000).map_err(win_error)?;
        timeouts.SetTransactionTimeout(1_000).map_err(win_error)?;
    }
    drop(timeouts);
    let property = VARIANT::from("CaptionsTextBlock");
    // SAFETY: variant stays live for this call; condition owns its own value copy.
    let condition =
        unsafe { automation.CreatePropertyCondition(UIA_AutomationIdPropertyId, &property) }
            .map_err(win_error)?;
    // Windows exposes this explicit ready indicator before the first speech.
    // Do not wait for a non-existent caption node or send UI status as speech.
    let ready_condition = unsafe {
        automation.CreatePropertyCondition(
            UIA_AutomationIdPropertyId,
            &VARIANT::from("ReadyToCaptionTextBlock"),
        )
    }
    .map_err(win_error)?;
    let mut element: Option<IUIAutomationElement> = None;
    let mut failures = 0;
    let retries = [250, 500, 1_000, 2_000, 5_000];
    let mut sampler = Sampler::default();
    let mut applied_hidden = None;
    let startup = Instant::now();
    let node_deadline = startup + Duration::from_secs(15);
    let mut startup_window_shown = false;
    while !stopped(control) {
        let mut operation = "读取字幕文本";
        let text = (|| {
            // SAFETY: HWND and all COM interfaces belong to this MTA. Element is
            // cached only here and invalidated on any failed access.
            unsafe {
                if element.is_none() {
                    operation = "连接字幕窗口";
                    if !window.snapshot.same_window() {
                        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                            0x80040201u32 as i32,
                        )));
                    }
                    let root = automation.ElementFromHandle(window.hwnd)?;
                    operation = "查找字幕文本";
                    element = find_element(&root, &condition)?;
                    if element.is_none() {
                        // Hiding a ready, silent window also removes its ready
                        // marker from UIA. A live root with no text is silence;
                        // only the first successful attach requires the marker.
                        if window.ready {
                            return Ok(Some(String::new()));
                        }
                        operation = "检查系统字幕就绪";
                        let ready = find_element(&root, &ready_condition)?;
                        return match ready {
                            Some(ready) if !ready.CurrentName()?.is_empty() => {
                                Ok(Some(String::new()))
                            }
                            _ => Ok(None),
                        };
                    }
                }
                operation = "读取字幕文本";
                element
                    .as_ref()
                    .unwrap()
                    .CurrentName()
                    .map(|name| Some(name.to_string()))
            }
        })();
        match text {
            Ok(None) => {
                if stopped(control) {
                    break;
                }
                failures = 0;
                // A pre-existing hidden window is not initialized merely because
                // its HWND exists. Let a new window finish native construction,
                // then show it once so XAML/UIA and any setup UI can become ready.
                // Apply the user's hidden preference only after readiness below.
                if !startup_window_shown && startup.elapsed() >= Duration::from_secs(1) {
                    window.snapshot.show_for_setup()?;
                    startup_window_shown = true;
                }
                if Instant::now() >= node_deadline {
                    return Err(io::Error::other(
                        "未能读取系统字幕的就绪状态。请查看 Windows 实时字幕窗口后重试。",
                    ));
                }
                if wait(control, Duration::from_millis(250)) {
                    break;
                }
            }
            Ok(Some(text)) => {
                // A provider call can return after the user stopped startup.
                // Do not take ownership or hide an onboarding window then.
                let state = control.0.lock().unwrap();
                if state.stop {
                    break;
                }
                failures = 0;
                let first_ready = !window.ready;
                window.ready = true;
                let hidden = state.hidden;
                drop(state);
                let visibility = (hidden, hidden && element.is_none());
                if applied_hidden != Some(visibility) {
                    window.snapshot.set_hidden(visibility.0, visibility.1)?;
                    window.hidden = hidden;
                    applied_hidden = Some(visibility);
                }
                if first_ready {
                    callback(CaptionEvent::Ready);
                }
                if let Some(sample) = sampler.observe(&text) {
                    callback(CaptionEvent::Text(sample));
                }
                if wait(
                    control,
                    Duration::from_millis(if text.is_empty() { 50 } else { 25 }),
                ) {
                    break;
                }
            }
            Err(error) => {
                #[cfg(test)]
                eprintln!(
                    "UIA {operation} failed: {:#010X}; same native window: {}; old: {:?}; current: {:?}; owned process running: {}",
                    error.code().0,
                    window.snapshot.same_window(),
                    window.hwnd,
                    find_window(),
                    window.owned.as_ref().is_some_and(|child| unsafe {
                        WaitForSingleObject(HANDLE(child.as_raw_handle()), 0) != WAIT_OBJECT_0
                    })
                );
                element = None;
                if failures == retries.len() {
                    let message = if window.ready {
                        "系统字幕连接已中断，请从托盘重新开启。"
                    } else {
                        "连接 Windows 实时字幕失败，请从托盘重新开启。"
                    };
                    return Err(io::Error::other(format!(
                        "{message}（{operation}，系统错误 {:#010X}）",
                        error.code().0
                    )));
                }
                let delay = retries[failures];
                failures += 1;
                if wait(control, Duration::from_millis(delay)) {
                    break;
                }
            }
        }
    }
    window.hidden = control.0.lock().unwrap().hidden;
    // UIA interfaces drop before AttachedWindow restores/closes the system window.
    drop(element);
    drop(condition);
    drop(ready_condition);
    drop(automation);
    drop(window);
    Ok(())
}

fn win_error(error: windows::core::Error) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn earlier_transcript_edits_do_not_reset_current_caption_stability() {
        let mut sampler = Sampler::default();
        let first = sampler
            .observe("Context number two. Current unfinished caption")
            .unwrap();
        assert_eq!(first.changes, 1);
        for poll in 1..=50 {
            let raw = format!("Context number {poll}. Current unfinished caption");
            let sample = sampler.observe(&raw).unwrap();
            assert_eq!(
                sample.changes, 1,
                "the original counter compares latestCaption"
            );
            assert_eq!(sample.unchanged, poll);
        }
        let complete = sampler
            .observe("Context number 50. Current unfinished caption.")
            .unwrap();
        assert_eq!(complete.changes, 2);
        assert_eq!(complete.unchanged, 0);
    }
    #[test]
    fn silence_does_not_count_as_caption_changes_or_erase_the_last_sample() {
        let mut sampler = Sampler::default();
        assert!(sampler.observe("").is_none());
        for (index, text) in [
            "A partial sentence",
            "A partial sentence grows",
            "A partial sentence grows further",
        ]
        .into_iter()
        .enumerate()
        {
            let sample = sampler.observe(text).unwrap();
            assert_eq!(sample.changes, index as u64 + 1);
            assert_eq!(sample.unchanged, 0);
            assert!(sampler.observe("").is_none());
            assert!(sampler.observe(" \r\n").is_none());
        }
        assert!(
            sampler
                .observe("A partial sentence grows further")
                .is_none()
        );
        assert_eq!(sampler.0.changes, 3);
        assert_eq!(sampler.0.unchanged, 1);
    }
    #[test]
    #[ignore = "read-only UIA comparison on the exact existing test caption window; no launch or visibility changes"]
    fn real_existing_caption_node_search_scope_diagnostic() {
        let expected: u32 = std::env::var("LIVE_CAPTIONS_EXPECT_PID")
            .unwrap()
            .parse()
            .unwrap();
        let (window, process) = find_window().expect("no existing caption window");
        assert_eq!(process, expected, "preserve an unrelated caption window");
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).unwrap();
        }
        {
            let automation: IUIAutomation =
                unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }.unwrap();
            let timeouts: IUIAutomation2 = automation.cast().unwrap();
            unsafe {
                timeouts.SetConnectionTimeout(1000).unwrap();
                timeouts.SetTransactionTimeout(1000).unwrap();
            }
            let root = unsafe { automation.ElementFromHandle(window) }.unwrap();
            let condition = unsafe {
                automation.CreatePropertyCondition(
                    UIA_AutomationIdPropertyId,
                    &VARIANT::from("CaptionsTextBlock"),
                )
            }
            .unwrap();
            let cache = unsafe { automation.CreateCacheRequest() }.unwrap();
            unsafe {
                cache
                    .SetTreeFilter(&automation.RawViewCondition().unwrap())
                    .unwrap();
            }
            let describe = |result: windows::core::Result<IUIAutomationElement>| match result {
                Ok(element) => unsafe {
                    format!(
                        "found; control={:?}; offscreen={:?}; rect={:?}",
                        element.CurrentIsControlElement(),
                        element.CurrentIsOffscreen(),
                        element.CurrentBoundingRectangle()
                    )
                },
                Err(error) => format!("not found/error={:#010X}", error.code().0),
            };
            println!(
                "Default FindFirst: {}",
                describe(unsafe { root.FindFirst(TreeScope_Subtree, &condition) })
            );
            println!(
                "Raw-filter FindFirstBuildCache: {}",
                describe(unsafe {
                    root.FindFirstBuildCache(TreeScope_Subtree, &condition, &cache)
                })
            );
        }
        unsafe {
            CoUninitialize();
        }
    }
    #[test]
    #[ignore = "explicitly authorized real Windows caption diagnostic; may briefly display its window"]
    fn real_hidden_caption_connection_remains_readable() {
        if std::env::var_os("LIVE_CAPTIONS_ATTACH_PROBE").is_some() {
            assert!(
                find_window().is_some(),
                "attach-only diagnostic must not launch another system caption process"
            );
        }
        let (sender, events) = std::sync::mpsc::channel();
        let callback = Arc::new(move |event| {
            let _ = sender.send(event);
        });
        let mut session = CaptionSession::start(true, callback).unwrap();
        let deadline = Instant::now() + Duration::from_secs(35);
        let mut ready_at = None;
        loop {
            match events.recv_timeout(Duration::from_millis(100)) {
                Ok(CaptionEvent::Ready) => {
                    println!(
                        "Actual UIA Ready; observing hidden caption connectivity without reading/logging text"
                    );
                    ready_at = Some(Instant::now());
                }
                Ok(CaptionEvent::Fault(error)) => panic!("{error}"),
                Ok(CaptionEvent::Text(_)) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("{error}"),
            }
            if ready_at.is_some_and(|ready| ready.elapsed() >= Duration::from_secs(15)) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "actual system caption Ready was not observed"
            );
        }
        session.stop();
    }
    use std::{
        os::windows::{
            io::{AsRawHandle, BorrowedHandle, OwnedHandle},
            process::CommandExt,
        },
        process::Stdio,
    };
    use windows::Win32::{
        Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::{CREATE_NO_WINDOW, TerminateProcess, WaitForSingleObject},
    };

    struct ProcessProbe(OwnedHandle);
    impl ProcessProbe {
        fn alive(&self) -> bool {
            // SAFETY: this retained duplicate refers only to a test-created child.
            unsafe { WaitForSingleObject(HANDLE(self.0.as_raw_handle()), 0) == WAIT_TIMEOUT }
        }
    }
    impl Drop for ProcessProbe {
        fn drop(&mut self) {
            // SAFETY: exact retained test process handle; never a PID/name lookup.
            unsafe {
                if self.alive() {
                    let _ = TerminateProcess(HANDLE(self.0.as_raw_handle()), 0);
                    let _ = WaitForSingleObject(HANDLE(self.0.as_raw_handle()), 2_000);
                }
            }
        }
    }
    fn test_process() -> (Child, ProcessProbe) {
        let root = std::env::var_os("SystemRoot").unwrap();
        let child = Command::new(
            PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
        )
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 60",
        ])
        .creation_flags(CREATE_NO_WINDOW.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
        // SAFETY: clone while Child owns its original valid process handle.
        let handle = unsafe { BorrowedHandle::borrow_raw(child.as_raw_handle()) }
            .try_clone_to_owned()
            .unwrap();
        (child, ProcessProbe(handle))
    }

    #[test]
    fn unready_caption_launch_survives_cancel_timeout_and_pid_mismatch_cleanup() {
        let (child, probe) = test_process();
        // This is the pending-launch cleanup used on timeout/cancellation/error.
        drop(Some(child));
        assert!(
            probe.alive(),
            "waiting for a caption window must not kill onboarding"
        );
        let (child, second) = test_process();
        assert!(matching_launch(Some(child), std::process::id()).is_none());
        assert!(
            second.alive(),
            "a competing window is not proof our launch is disposable"
        );
    }

    #[test]
    fn stop_releases_ready_recognition_but_preserves_setup_and_unrelated_processes() {
        for (ready, owned, stop) in [
            (false, true, false),
            (false, true, true),
            (false, false, true),
            (true, true, false),
            (true, true, true),
            (true, false, false),
            (true, false, true),
        ] {
            let (child, probe) = test_process();
            let (_unrelated_child, unrelated) = test_process();
            let snapshot = WindowSnapshot::without_window_for_test(&child);
            let window = AttachedWindow {
                hwnd: HWND::default(),
                snapshot,
                owned: owned.then_some(child),
                ready,
                hidden: false,
                control: Arc::new((
                    Mutex::new(Control {
                        stop,
                        hidden: false,
                    }),
                    Condvar::new(),
                )),
            };
            let start = Instant::now();
            drop(window);
            if ready && (owned || stop) {
                assert!(
                    // TerminateProcess is asynchronous: observe termination
                    // through the exact retained handle, not a name/PID poll.
                    unsafe {
                        WaitForSingleObject(HANDLE(probe.0.as_raw_handle()), 1_000) == WAIT_OBJECT_0
                    },
                    "stopping a ready session must release recognition, including reused processes"
                );
                assert!(start.elapsed() < Duration::from_secs(4));
            } else {
                assert!(
                    probe.alive(),
                    "unready setup and a reused process on connection failure must survive"
                );
            }
            assert!(unrelated.alive(), "never terminate processes by name");
        }
    }
    #[test]
    fn capture_emits_changes_and_exactly_one_stability_sample_without_periodic_ui_wakes() {
        let mut sampler = Sampler::default();
        assert_eq!(sampler.observe("short").unwrap().changes, 1);
        for poll in 1..=200 {
            let sample = sampler.observe("short");
            assert_eq!(sample.is_some(), poll == 50);
            if let Some(sample) = sample {
                assert_eq!(sample.unchanged, 50);
            }
        }
        assert_eq!(sampler.observe("different").unwrap().changes, 2);
        sampler.observe("");
        for _ in 0..100 {
            assert!(sampler.observe("").is_none());
        }
    }
}
