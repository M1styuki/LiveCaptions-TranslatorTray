//! Standard Windows popup menus; Windows owns metrics, paint and access keys.
use std::{io, sync::OnceLock};
use windows::{
    Win32::{
        Foundation::HMODULE,
        System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        UI::WindowsAndMessaging::*,
    },
    core::{PCSTR, PCWSTR, w},
};

fn follow_system_theme() {
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
        // Refresh per popup so system theme changes apply without polling or
        // changing the user's preferences. All sizing and drawing stay native.
        if let Some(refresh) = GetProcAddress(module, PCSTR(104usize as *const u8)) {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, unsafe extern "system" fn()>(
                refresh,
            )();
        }
        if let Some(set_mode) = GetProcAddress(module, PCSTR(135usize as *const u8)) {
            std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(i32) -> i32,
            >(set_mode)(1); // AllowDark
        }
        if let Some(flush) = GetProcAddress(module, PCSTR(136usize as *const u8)) {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, unsafe extern "system" fn()>(
                flush,
            )();
        }
    }
}

pub(super) struct Menu(HMENU);
impl Menu {
    pub(super) fn new(toggle: &str, busy: bool) -> io::Result<Self> {
        follow_system_theme();
        unsafe {
            let menu = Self(CreatePopupMenu().map_err(crate::native::os_error)?);
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
                AppendMenuW(menu.0, flags, id as usize, PCWSTR(text.as_ptr()))
                    .map_err(crate::native::os_error)?;
            }
            Ok(menu)
        }
    }
    pub(super) fn handle(&self) -> HMENU {
        self.0
    }
}
impl Drop for Menu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
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
            let menu = Menu::new(toggle, busy).unwrap();
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
