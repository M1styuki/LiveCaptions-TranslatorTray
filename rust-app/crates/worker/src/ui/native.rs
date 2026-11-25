use super::runtime;
use gpui::{
    AnyWindowHandle, AppContext, Bounds, Pixels, Render, Size, TitlebarOptions, Window,
    WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowOptions,
    point, px, size,
};
#[cfg(not(test))]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::{
        Input::KeyboardAndMouse::ReleaseCapture,
        WindowsAndMessaging::{
            HWND_NOTOPMOST, HWND_TOPMOST, SW_HIDE, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOSIZE, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow,
            WM_NCLBUTTONDOWN,
        },
    },
};

type Close = Rc<dyn Fn()>;
type BoundsChanged = Rc<dyn Fn(bool)>;

#[derive(Default)]
pub struct WindowState {
    handle: Cell<Option<AnyWindowHandle>>,
    hwnd: Cell<Option<usize>>,
    visible: Cell<bool>,
    size: Cell<Option<Size<Pixels>>>,
    close: RefCell<Option<Close>>,
    bounds_changed: RefCell<Option<BoundsChanged>>,
}

impl WindowState {
    pub fn on_close_requested(&self, callback: impl Fn() + 'static) {
        *self.close.borrow_mut() = Some(Rc::new(callback));
    }

    pub fn request_close(&self) {
        let callback = self.close.borrow().clone();
        if let Some(callback) = callback {
            callback();
        }
    }

    pub fn on_bounds_changed(&self, callback: impl Fn(bool) + 'static) {
        *self.bounds_changed.borrow_mut() = Some(Rc::new(callback));
    }

    pub fn bounds_changed(&self, size: Size<Pixels>) {
        let resized = self.size.replace(Some(size)).is_some_and(|old| old != size);
        let callback = self.bounds_changed.borrow().clone();
        if let Some(callback) = callback {
            runtime::dispatch(move || callback(resized));
        }
    }

    pub fn hwnd(&self) -> Option<usize> {
        self.hwnd.get()
    }
    pub fn is_visible(&self) -> bool {
        self.visible.get()
    }
    pub fn refresh(&self) {
        if self.visible.get()
            && let Some(app) = runtime::app()
        {
            let _ = app.refresh();
        }
    }
    #[cfg(test)]
    pub fn size(&self) -> Size<Pixels> {
        self.size.get().unwrap_or_default()
    }
    #[cfg(test)]
    pub fn test_handle(&self) -> AnyWindowHandle {
        self.handle.get().unwrap()
    }

    #[cfg(test)]
    pub fn set_size(&self, width: f32, height: f32) {
        self.size.set(Some(size(px(width), px(height))));
        if let (Some(mut app), Some(handle)) = (runtime::app(), self.handle.get()) {
            let _ = app.update_window(handle, |_, window, _| {
                window.resize(size(px(width), px(height)))
            });
        }
    }

    pub fn show<V: Render>(
        &self,
        title: &'static str,
        initial: (f32, f32),
        minimum: (f32, f32),
        create: impl FnOnce(&mut Window, &mut gpui::Context<V>) -> V + 'static,
    ) -> gpui::Result<()> {
        let Some(app) = runtime::app() else {
            self.visible.set(true);
            return Ok(());
        };
        if self.handle.get().is_none() {
            let handle = app.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(140.), px(140.)),
                        size(px(initial.0), px(initial.1)),
                    ))),
                    titlebar: Some(TitlebarOptions {
                        title: Some(title.into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    window_background: WindowBackgroundAppearance::Opaque,
                    window_decorations: Some(WindowDecorations::Client),
                    window_min_size: Some(size(px(minimum.0), px(minimum.1))),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| create(window, cx));
                    cx.new(|cx| gpui_component::Root::new(view, window, cx))
                },
            )?;
            #[cfg(test)]
            let hwnd = None;
            #[cfg(not(test))]
            let hwnd = app.clone().update_window(handle.into(), |_, window, _| {
                match HasWindowHandle::window_handle(window).ok()?.as_raw() {
                    RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as usize),
                    _ => None,
                }
            })?;
            self.hwnd.set(hwnd);
            self.handle.set(Some(handle.into()));
        }
        let was_visible = self.visible.replace(true);
        if let Some(hwnd) = self.native() {
            unsafe {
                let _ = ShowWindow(hwnd, if was_visible { SW_RESTORE } else { SW_SHOW });
                let _ = SetForegroundWindow(hwnd);
            }
        }
        self.refresh();
        Ok(())
    }

    pub fn hide(&self, destroy: bool) {
        self.visible.set(false);
        if destroy {
            if let (Some(mut app), Some(handle)) = (runtime::app(), self.handle.take()) {
                let _ = app.update_window(handle, |_, window, _| window.remove_window());
            }
            self.hwnd.set(None);
        } else if let Some(hwnd) = self.native() {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
    }

    fn native(&self) -> Option<HWND> {
        self.hwnd.get().map(|value| HWND(value as *mut _))
    }

    pub fn set_topmost(&self, value: bool) {
        if let Some(hwnd) = self.native() {
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    Some(if value { HWND_TOPMOST } else { HWND_NOTOPMOST }),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE,
                );
            }
        }
    }

    pub fn action(&self, action: i32) {
        if action == 0 {
            if let (Some(mut app), Some(handle)) = (runtime::app(), self.handle.get()) {
                let _ = app.update_window(handle, |_, window, _| window.minimize_window());
            }
        } else if let Some(hwnd) = self.native() {
            let hit = match action {
                2 => 2,
                3 => 17,
                _ => return,
            }; // HTCAPTION, HTBOTTOMRIGHT
            unsafe {
                let _ = ReleaseCapture();
                SendMessageW(hwnd, WM_NCLBUTTONDOWN, Some(WPARAM(hit)), Some(LPARAM(0)));
            }
        }
    }
}

pub fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}
