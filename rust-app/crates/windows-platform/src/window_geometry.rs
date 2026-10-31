//! Monitor work areas and normal-window placement, always physical at Win32.
//! Saved placement conversion is shared with the UI-independent geometry rules.
use captions_core::{
    geometry::{self, Anchor, Monitor, Placement, Restored},
    settings::Bounds,
};
use std::io;
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT},
    Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST,
        MONITORINFOEXW, MonitorFromPoint, MonitorFromWindow,
    },
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        WindowsAndMessaging::{
            GetCursorPos, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindow, IsZoomed,
            SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
        },
    },
};
use windows::core::BOOL;

fn monitor_info(handle: HMONITOR) -> io::Result<Monitor> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    let mut x = 96;
    let mut y = 96;
    // SAFETY: initialized output sizes, monitor handle from the current system.
    unsafe {
        if !GetMonitorInfoW(handle, &mut info.monitorInfo).as_bool() {
            return Err(io::Error::last_os_error());
        }
        GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut x, &mut y).map_err(io::Error::other)?;
    }
    let end = info
        .szDevice
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(info.szDevice.len());
    let area = info.monitorInfo.rcWork;
    Ok(Monitor {
        name: String::from_utf16_lossy(&info.szDevice[..end]),
        work_area: Bounds {
            x: f64::from(area.left),
            y: f64::from(area.top),
            width: f64::from(area.right - area.left),
            height: f64::from(area.bottom - area.top),
        },
        scale: f64::from(x.max(1)) / 96.0,
    })
}

pub fn monitors() -> io::Result<Vec<Monitor>> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        // SAFETY: EnumDisplayMonitors invokes this synchronously while the
        // caller's Vec remains exclusively borrowed and live.
        if let Ok(info) = monitor_info(monitor) {
            unsafe { &mut *(data.0 as *mut Vec<Monitor>) }.push(info);
        }
        BOOL(1)
    }
    let mut result = Vec::new();
    // SAFETY: callback does not retain the stack pointer or monitor handles.
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM((&mut result as *mut Vec<Monitor>) as isize),
        )
    }
    .ok()
    .map_err(io::Error::other)?;
    if result.is_empty() {
        return Err(io::Error::other("无法读取显示器工作区。"));
    }
    Ok(result)
}

fn own_window(window: HWND) -> io::Result<()> {
    let mut process = 0;
    // SAFETY: scalar checks; reject desktop/foreign/recycled handles before any
    // mutation. Only the window's owning UI thread calls these helpers.
    unsafe {
        if !IsWindow(Some(window)).as_bool() {
            return Err(io::Error::other("窗口已关闭。"));
        }
        GetWindowThreadProcessId(window, Some(&mut process));
    }
    if process != std::process::id() {
        return Err(io::Error::other("窗口不属于当前程序。"));
    }
    Ok(())
}

/// Windows 11 DWM preference for the original application's rounded frame.
/// A successful call is a hint; maximized/snapped windows remain system policy.
pub fn round_corners(window: usize) -> io::Result<()> {
    use windows::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
    };
    let window = HWND(window as *mut _);
    own_window(window)?;
    // SAFETY: a live window of this process, and the exact enum/size required
    // by DWMWA_WINDOW_CORNER_PREFERENCE. No regions or layered style changes.
    unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&DWMWCP_ROUND).cast(),
            size_of_val(&DWMWCP_ROUND) as u32,
        )
    }
    .map_err(io::Error::other)
}

/// Use the same Windows 11 Mica material as the original FluentWindow.
/// Callers keep their opaque fallback unless all required DWM calls succeed.
pub fn mica_backdrop(window: usize, dark: bool) -> io::Result<()> {
    use windows::Win32::Graphics::Dwm::{
        DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
        DwmSetWindowAttribute,
    };
    let window = HWND(window as *mut _);
    own_window(window)?;
    let dark = BOOL(i32::from(dark));
    // SAFETY: own live HWND and exact attribute sizes. Winit already creates
    // the transparent client surface; extending the full non-client frame
    // would also paint native caption buttons over the custom title bar.
    // This changes only this worker's main/API window, never OS preferences.
    unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            std::ptr::from_ref(&dark).cast(),
            size_of_val(&dark) as u32,
        )
        .map_err(io::Error::other)?;
        DwmSetWindowAttribute(
            window,
            DWMWA_SYSTEMBACKDROP_TYPE,
            std::ptr::from_ref(&DWMSBT_MAINWINDOW).cast(),
            size_of_val(&DWMSBT_MAINWINDOW) as u32,
        )
        .map_err(io::Error::other)
    }
}

pub fn restore(
    saved: Bounds,
    preferred: Option<&Anchor>,
    minimum: (f64, f64),
) -> io::Result<Restored> {
    geometry::restore(saved, preferred, &monitors()?, minimum)
        .ok_or_else(|| io::Error::other("窗口位置或尺寸无效。"))
}

pub fn snapshot(window: usize) -> io::Result<Option<Placement>> {
    let window = HWND(window as *mut _);
    own_window(window)?;
    // Never replace useful normal bounds with the taskbar's minimized position
    // or a maximized rectangle. Close flushes the latest normal snapshot.
    unsafe {
        if IsIconic(window).as_bool() || IsZoomed(window).as_bool() {
            return Ok(None);
        }
        let mut rect = RECT::default();
        GetWindowRect(window, &mut rect).map_err(io::Error::other)?;
        let mut display = monitor_info(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST))?;
        // An existing per-monitor-aware HWND is authoritative for its DPI.
        let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(window);
        if dpi > 0 {
            display.scale = f64::from(dpi) / 96.0;
        }
        Ok(geometry::capture(
            Bounds {
                x: f64::from(rect.left),
                y: f64::from(rect.top),
                width: f64::from(rect.right - rect.left),
                height: f64::from(rect.bottom - rect.top),
            },
            &display,
        ))
    }
}

pub(crate) fn work_area(window: usize) -> io::Result<Bounds> {
    let window = HWND(window as *mut _);
    own_window(window)?;
    // Called on geometry notifications, never for each text-rendering frame.
    unsafe { monitor_info(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST)) }
        .map(|monitor| monitor.work_area)
}

/// Match the original ownerless WPF CenterScreen startup location. Call once
/// for a newly created API window, never for later theme/redraw notifications.
pub fn center_on_cursor_monitor(window: usize) -> io::Result<()> {
    let handle = HWND(window as *mut _);
    own_window(handle)?;
    let mut cursor = POINT::default();
    let mut rect = RECT::default();
    // SAFETY: initialized output pointers and an HWND checked to be ours.
    // Reading the pointer position does not move it or synthesize any input.
    let (display, dpi) = unsafe {
        GetCursorPos(&mut cursor).map_err(io::Error::other)?;
        GetWindowRect(handle, &mut rect).map_err(io::Error::other)?;
        (
            monitor_info(MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST))?,
            windows::Win32::UI::HiDpi::GetDpiForWindow(handle),
        )
    };
    if dpi == 0 {
        return Err(io::Error::other("无法读取窗口 DPI。"));
    }
    // Preserve the logical size if the pointer is on a monitor with another
    // scale. Winit receives the native move/size and DPI notifications as usual.
    let ratio = display.scale * 96.0 / f64::from(dpi);
    let width = f64::from(rect.right - rect.left) * ratio;
    let height = f64::from(rect.bottom - rect.top) * ratio;
    apply(
        window,
        Bounds {
            x: display.work_area.x + (display.work_area.width - width) / 2.0,
            y: display.work_area.y + (display.work_area.height - height) / 2.0,
            width,
            height,
        },
    )
}

pub fn apply(window: usize, physical: Bounds) -> io::Result<()> {
    let window = HWND(window as *mut _);
    own_window(window)?;
    let values = [physical.x, physical.y, physical.width, physical.height];
    if values.iter().any(|value| {
        !value.is_finite() || *value < f64::from(i32::MIN) || *value > f64::from(i32::MAX)
    }) || physical.width < 1.0
        || physical.height < 1.0
    {
        return Err(io::Error::other("窗口位置或尺寸超出范围。"));
    }
    // SAFETY: exact process ownership checked on this UI thread; this does not
    // show/activate a hidden window or change its Z order.
    unsafe {
        SetWindowPos(
            window,
            None,
            physical.x.round() as i32,
            physical.y.round() as i32,
            physical.width.round() as i32,
            physical.height.round() as i32,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }
    .map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{
        Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GetDesktopWindow, IsWindowVisible, WINDOW_EX_STYLE,
            WS_POPUP,
        },
        core::w,
    };
    struct Hidden(HWND);
    impl Drop for Hidden {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    struct Awareness(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT);
    impl Drop for Awareness {
        fn drop(&mut self) {
            unsafe {
                windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(self.0);
            }
        }
    }
    #[test]
    fn native_monitor_snapshot_and_hidden_window_placement_round_trip_without_activation() {
        // Test binaries have no product manifest. Match the product's per-monitor
        // thread behavior without changing system settings or other test threads.
        let _awareness = Awareness(unsafe {
            windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(
                windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            )
        });
        assert!(
            !_awareness.0.0.is_null(),
            "thread DPI context could not be set"
        );
        let displays = monitors().unwrap();
        assert!(
            displays
                .iter()
                .all(|display| display.scale > 0.0 && display.work_area.width > 0.0)
        );
        // SAFETY: built-in STATIC class, no WS_VISIBLE, no owner and no activation.
        let window = Hidden(
            unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    w!(""),
                    WS_POPUP,
                    0,
                    0,
                    775,
                    170,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .unwrap(),
        );
        let display = &displays[0];
        let saved = Bounds {
            x: (display.work_area.x + 80.0) / display.scale,
            y: (display.work_area.y + 80.0) / display.scale,
            width: 775.0,
            height: 170.0,
        };
        let anchor = Anchor {
            monitor: display.name.clone(),
            x: 80.0 / display.scale,
            y: 80.0 / display.scale,
        };
        let restored = geometry::restore(saved, Some(&anchor), &displays, (750.0, 170.0)).unwrap();
        apply(window.0.0 as usize, restored.physical).unwrap();
        let actual = snapshot(window.0.0 as usize).unwrap().unwrap();
        assert_eq!(actual.anchor.monitor, display.name);
        for (a, b) in [
            actual.bounds.x,
            actual.bounds.y,
            actual.bounds.width,
            actual.bounds.height,
        ]
        .into_iter()
        .zip([saved.x, saved.y, saved.width, saved.height])
        {
            assert!(
                (a - b).abs() <= 1.0,
                "rounded logical placement mismatch: {a} != {b}"
            );
        }
        assert!(!unsafe { IsWindowVisible(window.0) }.as_bool());
        assert!(snapshot(0).is_err());
        assert!(apply(unsafe { GetDesktopWindow() }.0 as usize, restored.physical).is_err());
    }
}
