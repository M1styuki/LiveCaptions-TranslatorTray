use crate::native::{os_error, own, wide};
use std::{io, os::windows::io::OwnedHandle, time::Duration};
use windows::{
    Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError, LPARAM, WPARAM},
        System::Threading::CreateMutexW,
        UI::WindowsAndMessaging::{FindWindowW, PostMessageW},
    },
    core::PCWSTR,
};

pub enum Instance {
    Primary(OwnedHandle),
    Existing,
}

pub fn acquire(class_name: &[u16], session: u32) -> io::Result<Instance> {
    let name = wide(format!("Local\\LiveCaptionsTray-{session}"));
    // SAFETY: UTF-16 names remain live for these synchronous calls; mutex is held by RAII.
    let (mutex, existing) = unsafe {
        let mutex = own(CreateMutexW(None, false, PCWSTR(name.as_ptr())).map_err(os_error)?);
        (mutex, GetLastError() == ERROR_ALREADY_EXISTS)
    };
    if !existing {
        return Ok(Instance::Primary(mutex));
    }
    // A second launch may race the primary's message-window creation. This bounded
    // startup wait never runs in the resident host's idle path.
    for _ in 0..20 {
        // SAFETY: class_name is NUL-terminated and remains valid until the call returns.
        if let Ok(window) = unsafe { FindWindowW(PCWSTR(class_name.as_ptr()), None) } {
            // SAFETY: posts scalar data only; no cross-process pointers.
            unsafe {
                PostMessageW(
                    Some(window),
                    crate::tray::WM_SHOW_SETTINGS,
                    WPARAM(0),
                    LPARAM(0),
                )
            }
            .map_err(os_error)?;
            return Ok(Instance::Existing);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(io::Error::other("已有托盘进程尚未响应，请稍后再试。"))
}
