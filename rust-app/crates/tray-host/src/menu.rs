//! Native popup containing only the real commands and their separator.
use std::{io, sync::OnceLock};
use windows::{
    Win32::{
        Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUNDSMALL, DwmSetWindowAttribute},
        System::{
            LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
            Threading::GetCurrentThreadId,
        },
        UI::WindowsAndMessaging::*,
    },
    core::{PCSTR, PCWSTR, w},
};

fn prepare_dark_theme(owner: HWND) {
    // Win11 native dark menus use ordinal exports. Keep uxtheme loaded for the
    // process lifetime. Microsoft PowerToys uses these exports in ZoomIt/Utility.cpp.
    static UXTHEME: OnceLock<Option<usize>> = OnceLock::new();
    let module = UXTHEME.get_or_init(|| unsafe {
        LoadLibraryExW(w!("uxtheme.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
            .ok()
            .map(|module| module.0 as usize)
    });
    let Some(module) = module else { return };
    unsafe {
        let module = HMODULE(*module as *mut _);
        // Refresh the native theme before each popup without changing Windows settings.
        if let Some(refresh) = GetProcAddress(module, PCSTR(104usize as *const u8)) {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, unsafe extern "system" fn()>(
                refresh,
            )();
        }
        if let Some(set_mode) = GetProcAddress(module, PCSTR(135usize as *const u8)) {
            std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(i32) -> i32,
            >(set_mode)(2); // ForceDark: preserve the original dark tray popup.
        }
        if !owner.is_invalid()
            && let Some(allow) = GetProcAddress(module, PCSTR(133usize as *const u8))
        {
            std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(HWND, bool) -> bool,
            >(allow)(owner, true);
        }
        if let Some(flush) = GetProcAddress(module, PCSTR(136usize as *const u8)) {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, unsafe extern "system" fn()>(
                flush,
            )();
        }
    }
}

// The hook is local to the tray thread and exists only while its popup exists.
// Apply DWM's corner preference after the native menu window has been created;
// all client/nonclient painting and hit testing remain Windows' responsibility.
unsafe extern "system" fn popup_created(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if code == HC_ACTION as i32 {
            let message = &*(lparam.0 as *const CWPRETSTRUCT);
            if message.message == WM_CREATE {
                let mut name = [0u16; 16];
                let length = GetClassNameW(message.hwnd, &mut name) as usize;
                if name[..length] == [35, 51, 50, 55, 54, 56] {
                    // #32768: native popup menu
                    let corners = DWMWCP_ROUNDSMALL;
                    let _ = DwmSetWindowAttribute(
                        message.hwnd,
                        DWMWA_WINDOW_CORNER_PREFERENCE,
                        std::ptr::from_ref(&corners).cast(),
                        size_of_val(&corners) as u32,
                    );
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }
}

pub(super) struct Menu {
    handle: HMENU,
    hook: Option<HHOOK>,
}
impl Menu {
    pub(super) fn new(owner: HWND, toggle: &str, busy: bool) -> io::Result<Self> {
        prepare_dark_theme(owner);
        unsafe {
            let mut menu = Self {
                handle: CreatePopupMenu().map_err(crate::native::os_error)?,
                hook: None,
            };
            for (id, label, flags) in [
                (
                    super::tray::TOGGLE,
                    toggle,
                    MF_STRING | if busy { MF_GRAYED } else { MF_ENABLED },
                ),
                (super::tray::SETTINGS, "设置", MF_STRING),
                (0, "", MF_SEPARATOR),
                (super::tray::EXIT, "退出", MF_STRING),
            ] {
                let text = crate::native::wide(label);
                AppendMenuW(menu.handle, flags, id as usize, PCWSTR(text.as_ptr()))
                    .map_err(crate::native::os_error)?;
            }
            menu.hook = SetWindowsHookExW(
                WH_CALLWNDPROCRET,
                Some(popup_created),
                None,
                GetCurrentThreadId(),
            )
            .ok();
            Ok(menu)
        }
    }
    pub(super) fn handle(&self) -> HMENU {
        self.handle
    }
}
impl Drop for Menu {
    fn drop(&mut self) {
        unsafe {
            if let Some(hook) = self.hook.take() {
                let _ = UnhookWindowsHookEx(hook);
            }
            let _ = DestroyMenu(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_menu_preserves_labels_commands_and_busy_state() {
        for (toggle, busy) in [
            ("开启字幕", false),
            ("关闭字幕", false),
            ("正在切换…", true),
        ] {
            let menu = Menu::new(HWND::default(), toggle, busy).unwrap();
            unsafe {
                assert_eq!(GetMenuItemCount(Some(menu.handle())), 4);
                for (index, (id, label)) in [
                    (super::super::tray::TOGGLE, toggle),
                    (super::super::tray::SETTINGS, "设置"),
                    (0, ""),
                    (super::super::tray::EXIT, "退出"),
                ]
                .into_iter()
                .enumerate()
                {
                    let mut text = [0u16; 32];
                    let mut info = MENUITEMINFOW {
                        cbSize: size_of::<MENUITEMINFOW>() as u32,
                        fMask: MIIM_FTYPE | MIIM_ID | MIIM_STATE | MIIM_STRING,
                        dwTypeData: windows::core::PWSTR(text.as_mut_ptr()),
                        cch: text.len() as u32,
                        ..Default::default()
                    };
                    GetMenuItemInfoW(menu.handle(), index as u32, true, &mut info).unwrap();
                    assert_eq!(info.wID, id);
                    assert_eq!(info.fType.0 & MFT_OWNERDRAW.0, 0);
                    assert_eq!(info.fType.0 & MFT_SEPARATOR.0 != 0, index == 2);
                    if index != 2 {
                        assert_eq!(info.fState.0 & MFS_DISABLED.0 != 0, index == 0 && busy);
                    }
                    assert_eq!(String::from_utf16_lossy(&text[..info.cch as usize]), label);
                }
            }
        }
    }
}
