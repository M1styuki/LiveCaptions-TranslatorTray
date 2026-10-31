//! Read-only desktop facts; no windows are created or activated here.
use std::io;
use windows::Win32::{
    System::Com::CoCreateGuid,
    UI::{
        HiDpi::GetDpiForSystem,
        WindowsAndMessaging::{
            GetSystemMetrics, NONCLIENTMETRICSW, SM_CXSCREEN, SM_CYSCREEN, SPI_GETNONCLIENTMETRICS,
            SystemParametersInfoW,
        },
    },
};

/// Same system message-font family used by WPF's default Window/Control font.
/// Query once when a UI window is created; no timer, font installation or cache.
pub fn ui_font_family() -> String {
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    // SAFETY: GET operation only; correctly sized writable native structure.
    let result = unsafe {
        SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            metrics.cbSize,
            Some(std::ptr::from_mut(&mut metrics).cast()),
            Default::default(),
        )
    };
    if result.is_ok() {
        let face = metrics.lfMessageFont.lfFaceName;
        let end = face
            .iter()
            .position(|&value| value == 0)
            .unwrap_or(face.len());
        if end > 0 {
            return String::from_utf16_lossy(&face[..end]);
        }
    }
    "Segoe UI".to_owned()
}

pub fn instance_id() -> io::Result<String> {
    // SAFETY: CoCreateGuid takes no borrowed pointers and requires no COM apartment.
    let id = unsafe { CoCreateGuid() }.map_err(io::Error::other)?;
    Ok(format!("{:032x}", id.to_u128()))
}
pub fn screen_size() -> (f64, f64) {
    // SAFETY: these calls only read system metrics; no handles or ownership escape.
    unsafe {
        let scale = f64::from(GetDpiForSystem().max(96)) / 96.0;
        (
            f64::from(GetSystemMetrics(SM_CXSCREEN).max(1)) / scale,
            f64::from(GetSystemMetrics(SM_CYSCREEN).max(1)) / scale,
        )
    }
}
