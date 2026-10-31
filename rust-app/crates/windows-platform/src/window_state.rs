use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::PathBuf,
};
use windows::Win32::{
    Foundation::{COLORREF, HANDLE, HWND, WAIT_TIMEOUT},
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW, WaitForSingleObject,
        },
    },
    UI::WindowsAndMessaging::*,
};
use windows::core::PWSTR;

pub(crate) struct WindowSnapshot {
    window: HWND,
    pid: u32,
    placement: WINDOWPLACEMENT,
    extended_style: isize,
    layered: Option<(COLORREF, u8, LAYERED_WINDOW_ATTRIBUTES_FLAGS)>,
    layered_saved: bool,
    visible: bool,
    process: OwnedHandle,
}

impl WindowSnapshot {
    #[cfg(test)]
    pub(crate) fn without_window_for_test(child: &std::process::Child) -> Self {
        use std::os::windows::io::BorrowedHandle;
        // SAFETY: the borrowed process handle is valid for this clone; the
        // duplicate is owned independently. No real HWND or desktop is used.
        let process = unsafe { BorrowedHandle::borrow_raw(child.as_raw_handle()) }
            .try_clone_to_owned()
            .unwrap();
        Self {
            window: HWND::default(),
            pid: child.id(),
            placement: WINDOWPLACEMENT::default(),
            extended_style: 0,
            layered: None,
            layered_saved: false,
            visible: false,
            process,
        }
    }
    pub(crate) fn capture(window: HWND, pid: u32) -> io::Result<Self> {
        let mut current_session = 0;
        let mut caption_session = 0;
        // SAFETY: validates the actual window process and retains its handle until
        // restoration, preventing a recycled HWND/PID from inheriting saved state.
        let process = unsafe {
            ProcessIdToSessionId(std::process::id(), &mut current_session).map_err(win_error)?;
            ProcessIdToSessionId(pid, &mut caption_session).map_err(win_error)?;
            if current_session != caption_session {
                return Err(io::Error::other("系统字幕不在当前登录会话。"));
            }
            let raw = OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
            .map_err(win_error)?;
            OwnedHandle::from_raw_handle(raw.0)
        };
        let mut path = vec![0_u16; 32_768];
        let mut length = path.len() as u32;
        // SAFETY: path has length writable UTF-16 slots, process is a valid owned handle.
        unsafe {
            QueryFullProcessImageNameW(
                HANDLE(process.as_raw_handle()),
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut length,
            )
        }
        .map_err(win_error)?;
        let image = String::from_utf16(&path[..length as usize]).map_err(io::Error::other)?;
        let expected = PathBuf::from(
            std::env::var_os("SystemRoot")
                .ok_or_else(|| io::Error::other("找不到 Windows 系统目录。"))?,
        )
        .join("System32\\LiveCaptions.exe");
        if !image.eq_ignore_ascii_case(&expected.to_string_lossy()) {
            return Err(io::Error::other("系统字幕进程路径不正确。"));
        }
        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        // SAFETY: called on the caption-owning MTA thread with an existing HWND;
        // output structure is initialized to the required size.
        unsafe { GetWindowPlacement(window, &mut placement) }
            .map_err(|error| io::Error::other(error.to_string()))?;
        // SAFETY: reads a scalar window property; no pointer ownership is transferred.
        let extended_style = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) };
        // SAFETY: scalar visibility query on the same validated system window.
        let visible = unsafe { IsWindowVisible(window) }.as_bool();
        Ok(Self {
            window,
            pid,
            placement,
            extended_style,
            layered: None,
            layered_saved: false,
            visible,
            process,
        })
    }

    pub(crate) fn set_hidden(&mut self, hidden: bool, waiting_for_caption: bool) -> io::Result<()> {
        if !self.same_window() {
            return Err(io::Error::other("系统字幕窗口已关闭。"));
        }
        // Do not query layered attributes during native-window construction:
        // Windows initializes them later. Save them only after UIA is Ready,
        // immediately before this session's first alpha change.
        if hidden && !self.layered_saved {
            if self.extended_style & WS_EX_LAYERED.0 as isize != 0 {
                let mut color = COLORREF::default();
                let mut alpha = 0;
                let mut flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS::default();
                unsafe {
                    GetLayeredWindowAttributes(
                        self.window,
                        Some(&mut color),
                        Some(&mut alpha),
                        Some(&mut flags),
                    )
                }
                .map_err(win_error)?;
                self.layered = Some((color, alpha, flags));
            }
            self.layered_saved = true;
        }
        // SAFETY: exact HWND/PID was rechecked. Only this session's tool-window and
        // visibility is changed; original placement/style remain in this snapshot.
        unsafe {
            if hidden {
                let style = (self.extended_style
                    | WS_EX_TOOLWINDOW.0 as isize
                    | WS_EX_LAYERED.0 as isize
                    | WS_EX_TRANSPARENT.0 as isize
                    | WS_EX_NOACTIVATE.0 as isize)
                    & !(WS_EX_APPWINDOW.0 as isize);
                SetWindowLongPtrW(self.window, GWL_EXSTYLE, style);
                SetLayeredWindowAttributes(self.window, COLORREF(0), 0, LWA_ALPHA)
                    .map_err(win_error)?;
                if waiting_for_caption {
                    // SW_HIDE before the first speech prevents Windows from
                    // creating its text node. Keep layout active but invisible
                    // and non-interactive until that node exists.
                    let _ = ShowWindowAsync(self.window, SW_SHOWNOACTIVATE);
                } else {
                    // Keep alpha=0 until a later show/restore: SW_HIDE is
                    // asynchronous, so removing it here can briefly flash.
                    let _ = ShowWindowAsync(self.window, SW_HIDE);
                }
            } else {
                SetWindowLongPtrW(self.window, GWL_EXSTYLE, self.extended_style);
                self.restore_layered();
                let _ = ShowWindowAsync(self.window, SW_RESTORE);
            }
        }
        Ok(())
    }

    unsafe fn restore_layered(&self) {
        if let Some((color, alpha, flags)) = self.layered {
            // SAFETY: caller rechecked the exact HWND/PID and restored its style.
            let _ = unsafe { SetLayeredWindowAttributes(self.window, color, alpha, flags) };
        }
    }

    pub(crate) fn restore(&self, hidden: bool) {
        if !self.same_window() {
            return;
        }
        // SAFETY: restores only the saved properties of the same live HWND/PID.
        unsafe {
            SetWindowLongPtrW(self.window, GWL_EXSTYLE, self.extended_style);
            self.restore_layered();
            // Restore position/styles without reopening a window that the user
            // explicitly kept hidden during this caption session.
            let placement = restored_placement(self.placement, self.visible, hidden);
            let _ = SetWindowPlacement(self.window, &placement);
            if hidden || !self.visible {
                let _ = ShowWindowAsync(self.window, SW_HIDE);
            }
        }
    }

    pub(crate) fn same_window(&self) -> bool {
        let mut pid = 0;
        let mut class = [0u16; 64];
        // SAFETY: no dereference of HWND; Win32 validates whether it is still live.
        unsafe {
            GetWindowThreadProcessId(self.window, Some(&mut pid));
            let length = GetClassNameW(self.window, &mut class).max(0) as usize;
            IsWindow(Some(self.window)).as_bool()
                && pid == self.pid
                && class[..length]
                    == "LiveCaptionsDesktopWindow"
                        .encode_utf16()
                        .collect::<Vec<_>>()
                && WaitForSingleObject(HANDLE(self.process.as_raw_handle()), 0) == WAIT_TIMEOUT
        }
    }
}

fn restored_placement(
    mut placement: WINDOWPLACEMENT,
    originally_visible: bool,
    hidden: bool,
) -> WINDOWPLACEMENT {
    if hidden || !originally_visible {
        placement.showCmd = SW_HIDE.0 as u32;
    }
    placement
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopping_capture_preserves_the_requested_hidden_window() {
        let original = WINDOWPLACEMENT {
            showCmd: SW_SHOWNORMAL.0 as u32,
            ..Default::default()
        };
        for (visible, hidden, expected) in [
            (true, true, SW_HIDE),
            (true, false, SW_SHOWNORMAL),
            (false, true, SW_HIDE),
            (false, false, SW_HIDE),
        ] {
            assert_eq!(
                restored_placement(original, visible, hidden).showCmd,
                expected.0 as u32
            );
        }
    }
}

fn win_error(error: windows::core::Error) -> io::Error {
    io::Error::other(error.to_string())
}
