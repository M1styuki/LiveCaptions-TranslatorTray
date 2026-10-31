use std::io;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, POINT, WPARAM},
        UI::{
            Shell::{
                NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_ERROR, NIM_ADD,
                NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_SELECT, NINF_KEY, NOTIFYICON_VERSION_4,
                NOTIFYICONDATAW, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                GetCursorPos, HICON, LoadIconW, PostMessageW, SetForegroundWindow, TPM_BOTTOMALIGN,
                TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP,
                WM_CONTEXTMENU, WM_NULL,
            },
        },
    },
    core::PCWSTR,
};

pub const WM_TRAY: u32 = WM_APP + 1;
pub const WM_SHOW_SETTINGS: u32 = WM_APP + 2;
pub const WM_WORKER_EVENT: u32 = WM_APP + 3;
pub const TOGGLE: u32 = 100;
pub const SETTINGS: u32 = 101;
pub const EXIT: u32 = 102;
// The SDK macro is not emitted by windows-rs 0.62.2: NIN_SELECT | NINF_KEY.
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notification {
    Activate,
    ContextMenu,
}
pub fn notification(payload: u32) -> Option<Notification> {
    // We register NOTIFYICON_VERSION_4. Use semantic notifications as in the
    // Microsoft NotificationIcon sample, rather than also toggling on raw ups.
    match payload & 0xffff {
        NIN_SELECT | NIN_KEYSELECT => Some(Notification::Activate),
        WM_CONTEXTMENU => Some(Notification::ContextMenu),
        _ => None,
    }
}

pub struct Tray {
    data: NOTIFYICONDATAW,
    enabled_icon: HICON,
    disabled_icon: HICON,
}

impl Tray {
    pub fn new(window: HWND, instance: HINSTANCE) -> io::Result<Self> {
        // SAFETY: resources 2/3 are linked at build time. LoadIcon returns shared
        // module resource; the OS retains it for the module's lifetime.
        // MAKEINTRESOURCEW IDs are integers, never dereferenced as pointers.
        #[allow(clippy::manual_dangling_ptr)]
        let enabled_icon = unsafe { LoadIconW(Some(instance), PCWSTR(2_usize as *const u16)) }
            .map_err(crate::native::os_error)?;
        let disabled_icon = unsafe { LoadIconW(Some(instance), PCWSTR(3_usize as *const u16)) }
            .map_err(crate::native::os_error)?;
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: WM_TRAY,
            hIcon: disabled_icon,
            ..Default::default()
        };
        copy_text(&mut data.szTip, "实时字幕翻译 · 已关闭");
        let mut tray = Self {
            data,
            enabled_icon,
            disabled_icon,
        };
        tray.register()?;
        Ok(tray)
    }

    pub fn register(&mut self) -> io::Result<()> {
        self.data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
        // SAFETY: Shell copies this fixed-size structure synchronously. HWND/icon
        // remain valid until this Tray is dropped before window destruction.
        unsafe {
            if !Shell_NotifyIconW(NIM_ADD, &self.data).as_bool() {
                return Err(io::Error::last_os_error());
            }
            self.data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            if !Shell_NotifyIconW(NIM_SETVERSION, &self.data).as_bool() {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    pub fn update(&mut self, running: bool, busy: bool) {
        self.data.uFlags = NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        self.data.hIcon = if running {
            self.enabled_icon
        } else {
            self.disabled_icon
        };
        copy_text(
            &mut self.data.szTip,
            if busy {
                "实时字幕翻译 · 正在切换…"
            } else if running {
                "实时字幕翻译 · 已开启"
            } else {
                "实时字幕翻译 · 已关闭"
            },
        );
        // SAFETY: fixed structure remains live through the synchronous shell call.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.data);
        }
    }

    pub fn error(&mut self, message: &str) {
        self.data.uFlags = NIF_INFO;
        self.data.dwInfoFlags = NIIF_ERROR;
        copy_text(&mut self.data.szInfoTitle, "实时字幕翻译");
        copy_text(&mut self.data.szInfo, message);
        // SAFETY: balloon text is copied by the shell, never retained as Rust pointers.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.data);
        }
    }

    pub fn menu(&self, running: bool, busy: bool) -> io::Result<u32> {
        let toggle = if busy {
            "正在切换…"
        } else if running {
            "关闭字幕"
        } else {
            "开启字幕"
        };
        let menu = crate::menu::Menu::new(self.data.hWnd, toggle, busy)?;
        // SAFETY: the menu remains live through this nested message loop.
        unsafe {
            let mut position = POINT::default();
            GetCursorPos(&mut position).map_err(crate::native::os_error)?;
            let _ = SetForegroundWindow(self.data.hWnd);
            let command = TrackPopupMenu(
                menu.handle(),
                TPM_RETURNCMD | TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
                position.x,
                position.y,
                None,
                self.data.hWnd,
                None,
            )
            .0 as u32;
            // Complete the foreground activation before another tray popup.
            let _ = PostMessageW(Some(self.data.hWnd), WM_NULL, WPARAM(0), LPARAM(0));
            Ok(command)
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // SAFETY: shell receives the same HWND/id pair registered by this object.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
        }
    }
}

fn copy_text<const N: usize>(destination: &mut [u16; N], text: &str) {
    destination.fill(0);
    for (slot, character) in destination.iter_mut().take(N - 1).zip(text.encode_utf16()) {
        *slot = character;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{WM_LBUTTONUP, WM_RBUTTONUP};
    #[test]
    fn version_four_callback_sequences_activate_once_without_raw_mouse_duplicates() {
        let icon = 1 << 16;
        let left = [WM_LBUTTONUP | icon, NIN_SELECT | icon];
        assert_eq!(
            left.into_iter()
                .filter(|p| notification(*p) == Some(Notification::Activate))
                .count(),
            1
        );
        let right = [WM_RBUTTONUP | icon, WM_CONTEXTMENU | icon];
        assert_eq!(
            right
                .into_iter()
                .filter(|p| notification(*p) == Some(Notification::ContextMenu))
                .count(),
            1
        );
        assert_eq!(
            notification(NIN_KEYSELECT | icon),
            Some(Notification::Activate)
        );
    }
}
