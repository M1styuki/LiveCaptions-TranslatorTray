//! A native file dialog owns one short-lived STA and a hidden owner HWND on
//! that same thread, owned by the requesting main window. Only scalar HWNDs
//! and paths/results cross threads. Cancellation posts to
//! that HWND: its STA closes the exact Shell picker and active modal child.
use std::{
    cell::Cell,
    ffi::OsString,
    io,
    os::windows::ffi::OsStringExt,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};
use windows::{
    Win32::{
        Foundation::{
            ERROR_CANCELLED, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM,
            LRESULT, WPARAM,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize,
            },
            LibraryLoader::GetModuleHandleW,
            Ole::IOleWindow,
            Threading::{GetCurrentProcessId, GetCurrentThreadId},
        },
        UI::{
            Controls::TDM_CLICK_BUTTON,
            Input::KeyboardAndMouse::IsWindowEnabled,
            Shell::{
                Common::COMDLG_FILTERSPEC, FOS_DONTADDTORECENT, FOS_FILEMUSTEXIST,
                FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_NOTESTFILECREATE, FOS_OVERWRITEPROMPT,
                FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FOS_STRICTFILETYPES, FileOpenDialog,
                FileSaveDialog, IFileDialog, SIGDN_FILESYSPATH,
            },
            WindowsAndMessaging::*,
        },
    },
    core::{HRESULT, Interface, w},
};
const CANCEL: u32 = WM_APP + 74;
const CANCEL_TIMER: usize = 1;
const CLASS: windows::core::PCWSTR = w!("RustLiveCaptionsFileDialogOwner");
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Folder,
    Config,
    Csv,
}
pub type Callback = Arc<dyn Fn(Result<Option<PathBuf>, String>) + Send + Sync>;
#[derive(Default)]
struct Control {
    cancelled: bool,
    window: Option<usize>,
    finished: bool,
}
pub struct Job {
    control: Arc<Mutex<Control>>,
    thread: Option<JoinHandle<()>>,
    owned: bool,
}
impl Job {
    #[cfg(test)]
    pub fn start(kind: Kind, topmost: bool, callback: Callback) -> io::Result<Self> {
        Self::spawn(kind, topmost, callback, true, None)
    }
    /// The controller keeps the main UI alive until this job has joined. The
    /// main window owns our STA popup, never receives cross-thread EnableWindow.
    pub fn start_owned(
        kind: Kind,
        topmost: bool,
        parent: usize,
        callback: Callback,
    ) -> io::Result<Self> {
        Self::spawn(kind, topmost, callback, true, Some(parent))
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn start_hidden_fixture(kind: Kind, callback: Callback) -> io::Result<Self> {
        Self::spawn(kind, false, callback, false, None)
    }
    fn spawn(
        kind: Kind,
        topmost: bool,
        callback: Callback,
        show: bool,
        parent: Option<usize>,
    ) -> io::Result<Self> {
        let control = Arc::new(Mutex::new(Control::default()));
        let shared = control.clone();
        let thread = thread::Builder::new()
            .name("file-dialog-sta".into())
            .spawn(move || {
                let result = (|| {
                    // SAFETY: this new thread has no COM apartment yet. All dialog
                    // and Shell interfaces are destroyed before this guard drops.
                    unsafe {
                        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
                    }
                    let _apartment = Apartment;
                    run(kind, topmost, &shared, show, parent)
                })()
                .map_err(|error: windows::core::Error| {
                    format!("文件选择失败（{}）。", error.code())
                });
                let result = {
                    let mut state = shared.lock().unwrap();
                    // run(), its HWND owner and COM apartment have all been
                    // destroyed before publishing completion to the UI thread.
                    state.finished = true;
                    if state.cancelled { Ok(None) } else { result }
                };
                callback(result);
            })?;
        Ok(Self {
            control,
            thread: Some(thread),
            owned: parent.is_some(),
        })
    }
    pub fn cancel(&self) {
        let mut control = self.control.lock().unwrap();
        control.cancelled = true;
        if let Some(hwnd) = control.window {
            // SAFETY: removal and posting share this mutex, so an HWND cannot
            // be destroyed/recycled between this read and the post.
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), CANCEL, WPARAM(0), LPARAM(0));
            }
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            // Normal owned jobs stay in the controller until completion. During
            // abnormal UI unwinding never block its input queue on a still-live
            // cross-thread owned HWND. Cancellation is already posted above.
            if !self.owned || self.control.lock().unwrap().finished {
                let _ = thread.join();
            }
        }
    }
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
struct Context {
    dialog: IFileDialog,
    showing: Cell<bool>,
    canceling: Cell<bool>,
}
struct Owner {
    hwnd: HWND,
    control: Arc<Mutex<Control>>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.control.lock().unwrap().window = None;
        // SAFETY: this owner is only constructed and destroyed on the STA;
        // its Context remains alive until after DestroyWindow returns.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: CREATESTRUCT points at a live Box<Context> for this hidden HWND's
    // whole lifetime. Access is shared/reentrant, never an outstanding &mut.
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        if message == CANCEL || (message == WM_TIMER && wparam.0 == CANCEL_TIMER) {
            let context = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Context;
            if let Some(context) = context.as_ref() {
                if message == CANCEL {
                    context.canceling.set(true);
                }
                if !context.canceling.get() {
                    return LRESULT(0);
                }
                if context.showing.get() {
                    request_dialog_close(context);
                    if message == CANCEL {
                        // Only while cancellation is pending: handle early Show
                        // construction and an active overwrite-confirmation child.
                        SetTimer(Some(hwnd), CANCEL_TIMER, 50, None);
                    }
                } else {
                    let _ = KillTimer(Some(hwnd), CANCEL_TIMER);
                    PostQuitMessage(0);
                }
            }
            return LRESULT(0);
        }
        if message == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}
unsafe fn request_dialog_close(context: &Context) {
    // Close() from an arbitrary owner WndProc can return S_OK without ending
    // Show(). Post to the actual Shell dialog, using its own STA COM interface.
    let Ok(window) = context.dialog.cast::<IOleWindow>() else {
        return;
    };
    let Ok(dialog) = (unsafe { window.GetWindow() }) else {
        return;
    };
    let mut process = 0;
    let thread = unsafe { GetWindowThreadProcessId(dialog, Some(&mut process)) };
    if thread != unsafe { GetCurrentThreadId() } || process != unsafe { GetCurrentProcessId() } {
        return;
    }
    // GetLastActivePopup returns an owned picker itself. GW_ENABLEDPOPUP
    // finds its enabled confirmation child while the picker is disabled.
    let popup = unsafe { GetWindow(dialog, GW_ENABLEDPOPUP) }.unwrap_or(dialog);
    let target = if popup != dialog && unsafe { IsWindowVisible(popup) }.as_bool() {
        let mut popup_process = 0;
        let popup_thread = unsafe { GetWindowThreadProcessId(popup, Some(&mut popup_process)) };
        if popup_thread != thread || popup_process != process {
            return;
        }
        popup
    } else {
        dialog
    };
    if target != dialog {
        // Some Yes/No confirmations ignore WM_CLOSE. Choose only an existing,
        // enabled negative button; cancellation must never affirm overwrite.
        for id in [2, 7] {
            // IDCANCEL, IDNO
            if let Ok(button) = unsafe { GetDlgItem(Some(target), id) }
                && unsafe { IsWindowEnabled(button) }.as_bool()
            {
                let _ = unsafe {
                    PostMessageW(
                        Some(target),
                        WM_COMMAND,
                        WPARAM(id as usize),
                        LPARAM(button.0 as isize),
                    )
                };
                return;
            }
        }
        // Modern TaskDialog buttons are virtual controls with no HWND ID.
        // IDNO is a negative response; never select the affirmative default.
        let _ = unsafe {
            PostMessageW(
                Some(target),
                TDM_CLICK_BUTTON.0 as u32,
                WPARAM(7),
                LPARAM(0),
            )
        };
    }
    // The same STA owns this exact dialog and its active modal child. The next
    // cancellation tick closes the root after a confirmation child has gone.
    let _ = unsafe { PostMessageW(Some(target), WM_CLOSE, WPARAM(0), LPARAM(0)) };
}
fn configured(kind: Kind) -> windows::core::Result<IFileDialog> {
    // SAFETY: caller is the initialized STA. The returned COM interface stays
    // on it, including option setup, Show, GetResult and destruction.
    unsafe {
        let class = if kind == Kind::Csv {
            &FileSaveDialog
        } else {
            &FileOpenDialog
        };
        let dialog: IFileDialog = CoCreateInstance(class, None, CLSCTX_INPROC_SERVER)?;
        let common = dialog.GetOptions()?
            | FOS_FORCEFILESYSTEM
            | FOS_NOCHANGEDIR
            | FOS_DONTADDTORECENT
            | FOS_PATHMUSTEXIST;
        match kind {
            Kind::Folder => {
                dialog.SetOptions(common | FOS_PICKFOLDERS)?;
                dialog.SetTitle(w!("选择旧版实时字幕翻译的数据文件夹"))?;
            }
            Kind::Config => {
                dialog.SetOptions(common | FOS_FILEMUSTEXIST)?;
                dialog.SetTitle(w!("选择用于恢复的配置文件"))?;
                dialog.SetFileTypes(&[
                    COMDLG_FILTERSPEC {
                        pszName: w!("JSON 配置文件"),
                        pszSpec: w!("*.json"),
                    },
                    COMDLG_FILTERSPEC {
                        pszName: w!("所有文件"),
                        pszSpec: w!("*.*"),
                    },
                ])?;
            }
            Kind::Csv => {
                dialog.SetOptions(
                    common | FOS_OVERWRITEPROMPT | FOS_NOTESTFILECREATE | FOS_STRICTFILETYPES,
                )?;
                dialog.SetTitle(w!("导出全部翻译历史"))?;
                dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
                    pszName: w!("CSV 文件"),
                    pszSpec: w!("*.csv"),
                }])?;
                dialog.SetDefaultExtension(w!("csv"))?;
                dialog.SetFileName(w!("translation_history.csv"))?;
            }
        }
        #[cfg(test)]
        if kind == Kind::Csv
            && let Some(path) = std::env::var_os("LIVE_CAPTIONS_OVERWRITE_FIXTURE")
        {
            use std::os::windows::ffi::OsStrExt;
            let filename: Vec<u16> = path.encode_wide().chain(Some(0)).collect();
            dialog.SetFileName(windows::core::PCWSTR(filename.as_ptr()))?;
        }
        Ok(dialog)
    }
}
fn run(
    kind: Kind,
    topmost: bool,
    control: &Arc<Mutex<Control>>,
    show: bool,
    parent: Option<usize>,
) -> windows::core::Result<Option<PathBuf>> {
    if control.lock().unwrap().cancelled {
        return Ok(None);
    }
    let context = Box::new(Context {
        dialog: configured(kind)?,
        showing: Cell::new(false),
        canceling: Cell::new(false),
    });
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
    let class = WNDCLASSW {
        lpfnWndProc: Some(procedure),
        hInstance: instance,
        lpszClassName: CLASS,
        ..Default::default()
    };
    let parent = parent
        .map(|raw| {
            let window = HWND(raw as *mut _);
            let mut pid = 0;
            // Only the current worker's live UI can own this popup. The controller
            // keeps its Slint component/ HWND alive through cancellation and join.
            unsafe {
                GetWindowThreadProcessId(window, Some(&mut pid));
            }
            if pid == unsafe { GetCurrentProcessId() } {
                Ok(window)
            } else {
                Err(windows::core::Error::from(
                    windows::Win32::Foundation::E_INVALIDARG,
                ))
            }
        })
        .transpose()?;
    let hwnd = unsafe {
        if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(windows::core::Error::from_thread());
        }
        CreateWindowExW(
            WS_EX_TOOLWINDOW
                | if topmost {
                    WS_EX_TOPMOST
                } else {
                    WINDOW_EX_STYLE(0)
                },
            CLASS,
            w!("实时字幕翻译 · 文件选择"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            parent,
            None,
            Some(instance),
            Some(context.as_ref() as *const _ as *const _),
        )?
    };
    let owner = Owner {
        hwnd,
        control: control.clone(),
    };
    {
        let mut state = control.lock().unwrap();
        if state.cancelled {
            return Ok(None);
        }
        state.window = Some(hwnd.0 as usize);
    }
    if !show {
        // Test support never calls IFileDialog::Show or makes an HWND visible.
        let mut message = MSG::default();
        unsafe {
            loop {
                let result = GetMessageW(&mut message, None, 0, 0).0;
                if result == -1 {
                    return Err(windows::core::Error::from_thread());
                }
                if result == 0 {
                    break;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        return Ok(None);
    }
    context.showing.set(true);
    let shown = unsafe { context.dialog.Show(Some(owner.hwnd)) };
    context.showing.set(false);
    if control.lock().unwrap().cancelled {
        return Ok(None);
    }
    match shown {
        Err(error) if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => Ok(None),
        Err(error) => Err(error),
        Ok(()) => unsafe {
            let result = context.dialog.GetResult()?;
            let path = result.GetDisplayName(SIGDN_FILESYSPATH)?;
            let value = PathBuf::from(OsString::from_wide(path.as_wide()));
            CoTaskMemFree(Some(path.0 as *const _));
            Ok(Some(value))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    fn wait_for_actual_dialog(job: &Job) -> Option<HWND> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(owner) = job.control.lock().unwrap().window {
                let owner = HWND(owner as *mut _);
                let popup = unsafe { GetLastActivePopup(owner) };
                if popup != owner && unsafe { IsWindowVisible(popup) }.as_bool() {
                    return Some(popup);
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        None
    }
    fn cancel_and_join(job: Job, results: mpsc::Receiver<Result<Option<PathBuf>, String>>) {
        let (finished, completion) = mpsc::sync_channel(1);
        let started = Instant::now();
        let closer = thread::spawn(move || {
            drop(job);
            let _ = finished.send(());
        });
        let closed = completion.recv_timeout(Duration::from_secs(5)).is_ok();
        println!(
            "actual Shell cancellation joined={closed}, elapsed={:?}",
            started.elapsed()
        );
        assert!(
            closed,
            "actual modal Show did not process cancellation; test process will release its own windows"
        );
        closer.join().unwrap();
        assert!(
            results
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap()
                .is_none()
        );
    }
    #[test]
    #[ignore = "authorized actual Shell Show/cancel acceptance; briefly displays isolated dialogs"]
    fn actual_save_dialog_cancellation_finishes_its_sta() {
        for (kind, settle) in [
            (Kind::Folder, true),
            (Kind::Config, true),
            (Kind::Csv, true),
            (Kind::Folder, false),
            (Kind::Config, false),
            (Kind::Csv, false),
        ] {
            let (sender, results) = mpsc::sync_channel(1);
            let job = Job::start(
                kind,
                false,
                Arc::new(move |result| {
                    let _ = sender.send(result);
                }),
            )
            .unwrap();
            let shown = settle && wait_for_actual_dialog(&job).is_some();
            if settle {
                thread::sleep(Duration::from_secs(2));
            }
            println!("kind={kind:?}, settled={settle}, actual window observed={shown}");
            cancel_and_join(job, results);
            assert!(shown || !settle, "actual modal dialog was not observed");
        }
    }
    #[test]
    #[ignore = "authorized real overwrite-confirmation cancellation on an existing isolated public fixture"]
    fn actual_overwrite_prompt_cancellation_preserves_existing_file() {
        let file = PathBuf::from(std::env::var_os("LIVE_CAPTIONS_OVERWRITE_FIXTURE").unwrap())
            .canonicalize()
            .unwrap();
        let allowed = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../evidence/functional")
            .canonicalize()
            .unwrap();
        assert!(file.starts_with(allowed));
        assert_eq!(file.file_name().unwrap(), "keep.csv");
        assert!(
            file.parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("dialog-overwrite-")
        );
        let original = std::fs::read(&file).unwrap();
        let (sender, results) = mpsc::sync_channel(1);
        let job = Job::start(
            Kind::Csv,
            false,
            Arc::new(move |result| {
                let _ = sender.send(result);
            }),
        )
        .unwrap();
        let dialog = wait_for_actual_dialog(&job);
        let mut confirmation = false;
        if let Some(dialog) = dialog {
            thread::sleep(Duration::from_secs(2));
            // Only this test-created dialog: select the prefilled existing
            // fixture to enter the real overwrite confirmation, never export.
            unsafe {
                PostMessageW(Some(dialog), WM_COMMAND, WPARAM(1), LPARAM(0)).unwrap();
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                let popup = unsafe { GetWindow(dialog, GW_ENABLEDPOPUP) }.unwrap_or(dialog);
                if popup != dialog && unsafe { IsWindowVisible(popup) }.as_bool() {
                    confirmation = true;
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
        println!("actual overwrite confirmation observed={confirmation}");
        if confirmation {
            thread::sleep(Duration::from_secs(1));
        }
        cancel_and_join(job, results);
        assert!(
            confirmation,
            "actual overwrite confirmation was not observed"
        );
        assert_eq!(
            std::fs::read(&file).unwrap(),
            original,
            "cancellation changed the existing fixture"
        );
        println!("actual overwrite confirmation canceled; existing fixture bytes unchanged");
    }
    #[test]
    fn native_sta_cancel_owns_its_com_and_hidden_window_and_joins() {
        // Inspect the real Shell objects on an initialized STA, without Show.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
        }
        let _apartment = Apartment;
        for kind in [Kind::Folder, Kind::Config, Kind::Csv] {
            let dialog = configured(kind).unwrap();
            let options = unsafe { dialog.GetOptions().unwrap() }.0;
            assert_ne!(options & FOS_FORCEFILESYSTEM.0, 0);
            assert_ne!(options & FOS_DONTADDTORECENT.0, 0);
            if kind == Kind::Csv {
                assert_ne!(options & FOS_OVERWRITEPROMPT.0, 0);
                assert_ne!(options & FOS_STRICTFILETYPES.0, 0);
                assert_ne!(options & FOS_NOTESTFILECREATE.0, 0);
            } else if kind == Kind::Folder {
                assert_ne!(options & FOS_PICKFOLDERS.0, 0);
            } else {
                assert_ne!(options & FOS_FILEMUSTEXIST.0, 0);
            }
        }
        for kind in [Kind::Folder, Kind::Config, Kind::Csv] {
            let (sender, receiver) = mpsc::sync_channel(1);
            let job = Job::start_hidden_fixture(
                kind,
                Arc::new(move |result| {
                    sender.send(result).unwrap();
                }),
            )
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            let hwnd = loop {
                if let Some(hwnd) = job.control.lock().unwrap().window {
                    break hwnd;
                }
                assert!(Instant::now() < deadline, "hidden STA failed to initialize");
                thread::sleep(Duration::from_millis(5));
            };
            assert!(!unsafe { IsWindowVisible(HWND(hwnd as *mut _)) }.as_bool());
            let started = Instant::now();
            job.cancel();
            assert!(
                receiver
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .unwrap()
                    .is_none()
            );
            drop(job);
            assert!(started.elapsed() < Duration::from_secs(2));
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let job = Job::start_hidden_fixture(
            Kind::Csv,
            Arc::new(move |result| {
                sender.send(result).unwrap();
            }),
        )
        .unwrap();
        job.cancel();
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap()
                .is_none()
        );
        drop(job);
    }
    #[test]
    fn owned_sta_popup_cancels_while_requesting_thread_keeps_pumping() {
        fn pump() {
            let mut message = MSG::default();
            // This test parent remains invisible; no desktop input is sent.
            unsafe {
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
        let parent = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Hidden requesting test UI"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let job = Job::spawn(
            Kind::Csv,
            false,
            Arc::new(move |result| {
                sender.send(result).unwrap();
            }),
            false,
            Some(parent.0 as usize),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let owner = loop {
            pump();
            if let Some(window) = job.control.lock().unwrap().window {
                break HWND(window as *mut _);
            }
            assert!(
                Instant::now() < deadline,
                "owned STA initialization must not block the requesting UI"
            );
            thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(unsafe { GetWindow(owner, GW_OWNER).unwrap() }, parent);
        assert_eq!(unsafe { GetAncestor(owner, GA_ROOTOWNER) }, parent);
        assert_ne!(unsafe { GetWindowThreadProcessId(owner, None) }, unsafe {
            GetCurrentThreadId()
        });
        assert!(unsafe { IsWindowEnabled(parent) }.as_bool());
        assert!(!unsafe { IsWindowVisible(owner) }.as_bool());
        job.cancel();
        let result = loop {
            pump();
            if let Ok(result) = receiver.try_recv() {
                break result;
            }
            assert!(
                Instant::now() < deadline,
                "owned STA cancellation must complete while UI pumps"
            );
            thread::sleep(Duration::from_millis(2));
        };
        assert!(result.unwrap().is_none());
        assert!(job.control.lock().unwrap().finished);
        drop(job);
        assert!(!unsafe { IsWindow(Some(owner)) }.as_bool());
        assert!(unsafe { IsWindowEnabled(parent) }.as_bool());
        unsafe {
            DestroyWindow(parent).unwrap();
        }
    }
}
