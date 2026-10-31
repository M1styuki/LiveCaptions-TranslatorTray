//! Manual visual fixture for the production native menu; no tray, worker or data.
#![allow(dead_code)]
#[path = "../src/menu.rs"]
mod menu;
mod tray {
    pub const TOGGLE: u32 = 100;
    pub const SETTINGS: u32 = 101;
    pub const EXIT: u32 = 102;
}
mod native {
    pub fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain([0]).collect()
    }
    pub fn os_error(error: windows::core::Error) -> std::io::Error {
        std::io::Error::other(error)
    }
}
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_CONTEXTMENU => {
                let popup = menu::Menu::new(window, "开启字幕", false).unwrap();
                let mut point = POINT { x: 40, y: 40 };
                windows::Win32::Graphics::Gdi::ClientToScreen(window, &mut point)
                    .ok()
                    .unwrap();
                let _ = SetForegroundWindow(window);
                let choice = TrackPopupMenu(
                    popup.handle(),
                    TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RIGHTBUTTON,
                    point.x,
                    point.y,
                    None,
                    window,
                    None,
                )
                .0;
                println!("native menu selection={choice}");
                let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
                return LRESULT(0);
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return LRESULT(0);
            }
            _ => {}
        }
        DefWindowProcW(window, message, wp, lp)
    }
}
fn main() -> windows::core::Result<()> {
    unsafe {
        let instance = GetModuleHandleW(None)?.into();
        let class = w!("CaptionsNativeMenuPreview");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class,
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(6usize as *mut _),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        });
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("原生菜单预览（右键打开）"),
            WS_OVERLAPPEDWINDOW,
            240,
            180,
            440,
            280,
            None,
            None,
            Some(instance),
            None,
        )?;
        let _ = ShowWindow(window, SW_SHOW);
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}
