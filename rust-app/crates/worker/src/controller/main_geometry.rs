use super::Controller;
use captions_core::{
    geometry::{self, AutoHeight, Placement},
    settings::WindowKind,
};
use slint::{
    ComponentHandle,
    winit_030::{
        EventResult, WinitWindowAccessor,
        winit::{
            event::WindowEvent,
            raw_window_handle::{HasWindowHandle, RawWindowHandle},
        },
    },
};
use std::{cell::RefCell, io, rc::Rc, time::Duration};
use windows_platform::window_geometry;

#[derive(Default)]
struct State {
    active: bool,
    restored: bool,
    auto: AutoHeight,
    expected: Option<(f64, f64)>,
    resized: bool,
    timer: slint::Timer,
}
impl State {
    fn observe_resize(&mut self, bounds: captions_core::settings::Bounds) {
        if self.restored
            && self.resized
            && self.expected.is_some_and(|(width, height)| {
                (width - bounds.width).abs() > 1.1 || (height - bounds.height).abs() > 1.1
            })
        {
            self.auto.mark_manual();
        }
        self.resized = false;
    }
}
#[derive(Default)]
pub(super) struct Geometry {
    state: Rc<RefCell<State>>,
    #[cfg(test)]
    fixture: RefCell<Option<Fixture>>,
}
#[cfg(test)]
struct Fixture {
    physical: captions_core::settings::Bounds,
    monitor: geometry::Monitor,
    fail_apply: bool,
}

pub(super) fn hwnd(ui: &crate::MainWindow) -> Option<usize> {
    window_hwnd(ui.window())
}
fn window_hwnd(window: &slint::Window) -> Option<usize> {
    window
        .with_winit_window(|window| match window.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as usize),
            _ => None,
        })
        .flatten()
}

fn round_frame(window: &slint::Window, dark: bool) -> bool {
    use slint::winit_030::winit::platform::windows::WindowExtWindows;
    // Preserve the original system shadow and the non-client edge DWM needs
    // for a custom title bar. Winit owns the corresponding frame calculation.
    window.with_winit_window(|window| window.set_undecorated_shadow(true));
    if let Some(window) = window_hwnd(window) {
        // Cosmetic DWM policy must not make a usable settings window fail.
        let _ = window_geometry::round_corners(window);
        return window_geometry::mica_backdrop(window, dark).is_ok();
    }
    false
}

pub(super) fn style_main(ui: &crate::MainWindow) {
    ui.set_native_material(round_frame(ui.window(), ui.get_frame_dark()));
}

pub(super) fn style_api(ui: &crate::ApiWindow) {
    ui.set_native_material(round_frame(ui.window(), ui.get_frame_dark()));
}

pub(super) fn center_api(ui: &crate::ApiWindow) {
    if let Some(window) = window_hwnd(ui.window()) {
        let _ = window_geometry::center_on_cursor_monitor(window);
    }
}

impl Geometry {
    fn monitors(&self) -> io::Result<Vec<geometry::Monitor>> {
        #[cfg(test)]
        if let Some(fixture) = &*self.fixture.borrow() {
            return Ok(vec![fixture.monitor.clone()]);
        }
        window_geometry::monitors()
    }
    fn snapshot(&self, ui: &crate::MainWindow) -> io::Result<Option<Placement>> {
        #[cfg(test)]
        if let Some(fixture) = &*self.fixture.borrow() {
            return Ok(geometry::capture(fixture.physical, &fixture.monitor));
        }
        let Some(window) = hwnd(ui) else {
            return Ok(None);
        };
        window_geometry::snapshot(window)
    }
    fn apply(&self, ui: &crate::MainWindow, desired: &geometry::Restored) -> io::Result<()> {
        #[cfg(test)]
        if let Some(fixture) = &mut *self.fixture.borrow_mut() {
            if fixture.fail_apply {
                fixture.fail_apply = false;
                return Err(io::Error::other("controlled native placement failure"));
            }
            fixture.physical = desired.physical;
            ui.window().set_size(slint::LogicalSize::new(
                desired.logical.bounds.width as f32,
                desired.logical.bounds.height as f32,
            ));
            return Ok(());
        }
        if let Some(window) = hwnd(ui) {
            window_geometry::apply(window, desired.physical)?;
        }
        Ok(())
    }
}

impl Controller {
    pub fn bind_main_geometry(owner: &Rc<RefCell<Self>>) {
        let Some(ui) = owner.borrow().ui.upgrade() else {
            return;
        };
        let weak_ui = ui.as_weak();
        ui.on_frame_theme_changed(move || {
            if let Some(ui) = weak_ui.upgrade() {
                style_main(&ui);
            }
        });
        let state = owner.borrow().geometry.state.clone();
        let weak = Rc::downgrade(owner);
        ui.window().on_winit_window_event(move |_, event| {
            let resized =
                matches!(event, WindowEvent::Resized(size) if size.width > 0 && size.height > 0);
            let changed = resized
                || matches!(
                    event,
                    WindowEvent::Moved(_) | WindowEvent::ScaleFactorChanged { .. }
                );
            if changed {
                let mut state = state.borrow_mut();
                if state.active {
                    state.resized |= resized;
                    let weak = weak.clone();
                    state.timer.start(
                        slint::TimerMode::SingleShot,
                        Duration::from_millis(150),
                        move || {
                            if let Some(owner) = weak.upgrade() {
                                owner.borrow_mut().main_geometry_changed();
                            }
                        },
                    );
                }
            }
            EventResult::Propagate
        });
    }

    pub(super) fn sync_main_geometry(&self) {
        self.geometry.state.borrow_mut().active = self.main_open && !self.exiting;
        if !self.main_open || self.exiting {
            return;
        }
        if let Err(error) = self.fit_main_geometry() {
            self.message(&format!("窗口位置暂时无法恢复：{error}"));
        }
    }
    fn fit_main_geometry(&self) -> io::Result<()> {
        let (Some(ui), Some(settings)) = (self.ui.upgrade(), self.settings.as_ref()) else {
            return Ok(());
        };
        let Some(current) = self.geometry.snapshot(&ui)? else {
            return Ok(());
        };
        let monitors = self.geometry.monitors()?;
        let restored = {
            let mut state = self.geometry.state.borrow_mut();
            // A settings/page command can arrive before the debounce fires.
            // Consume the user's resize before replacing our expected size.
            state.observe_resize(current.bounds);
            state.restored
        };
        let (bounds, anchor) = if restored {
            (current.bounds, Some(current.anchor.clone()))
        } else {
            (
                settings.window_bounds(WindowKind::Main),
                settings.window_anchor(WindowKind::Main),
            )
        };
        let base = geometry::restore(bounds, anchor.as_ref(), &monitors, (800.0, 240.0))
            .ok_or_else(|| io::Error::other("无法找到可用显示器。"))?;
        let available = monitors
            .iter()
            .find(|monitor| monitor.name == base.logical.anchor.monitor)
            .map(|monitor| monitor.work_area.height / monitor.scale)
            .unwrap_or(240.0)
            .max(240.0);
        ui.set_available_window_height(available as f32);
        let required = if ui.get_page() == 1 {
            600.0
        } else if ui.get_page() == 2 {
            420.0
        } else if ui.get_page() == 0 && ui.get_caption_log_enabled() {
            (110.0 * f64::from(ui.get_display_sentences() + 1)).max(240.0)
        } else {
            240.0
        };
        let mut state = self.geometry.state.borrow_mut();
        if !state.restored {
            state.auto = AutoHeight::new(settings.window_manual_size(WindowKind::Main));
        }
        let height = state
            .auto
            .adjusted(base.logical.bounds.height, required.min(available));
        drop(state);
        let mut anchor = base.logical.anchor.clone();
        if height > base.logical.bounds.height {
            // Expanding the compact caption window into settings must keep
            // the newly added controls above the monitor's taskbar.
            let bottom = base.physical.y + height * base.scale;
            let overflow = (bottom - base.work_area.y - base.work_area.height).max(0.0);
            let shift = overflow.min((base.physical.y - base.work_area.y).max(0.0));
            anchor.y -= shift / base.scale;
        }
        let desired = geometry::restore(
            captions_core::settings::Bounds {
                height,
                ..base.logical.bounds
            },
            Some(&anchor),
            &monitors,
            (800.0, required.min(available)),
        )
        .ok_or_else(|| io::Error::other("窗口尺寸无效。"))?;
        let mut state = self.geometry.state.borrow_mut();
        state.expected = Some((desired.logical.bounds.width, desired.logical.bounds.height));
        drop(state);
        if [
            current.bounds.x - desired.logical.bounds.x,
            current.bounds.y - desired.logical.bounds.y,
            current.bounds.width - desired.logical.bounds.width,
            current.bounds.height - desired.logical.bounds.height,
        ]
        .into_iter()
        .any(|delta| delta.abs() > 0.75)
            || current.anchor.monitor != desired.logical.anchor.monitor
        {
            self.geometry.apply(&ui, &desired)?;
        }
        self.geometry.state.borrow_mut().restored = true;
        Ok(())
    }
    fn main_geometry_changed(&mut self) {
        if !self.main_open || self.exiting {
            return;
        }
        self.sync_main_geometry_if_first();
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        match self.geometry.snapshot(&ui) {
            Ok(Some(placement)) => self.store_main_geometry(placement),
            Ok(None) => {}
            Err(error) => self.message(&format!("窗口位置暂时无法保存：{error}")),
        }
    }
    fn sync_main_geometry_if_first(&self) {
        if !self.geometry.state.borrow().restored {
            self.sync_main_geometry();
        }
    }
    fn store_main_geometry(&mut self, placement: Placement) {
        let mut state = self.geometry.state.borrow_mut();
        if !state.restored {
            return;
        }
        state.observe_resize(placement.bounds);
        state.expected = Some((placement.bounds.width, placement.bounds.height));
        let manual = state.auto.is_manual();
        drop(state);
        if let Some(settings) = &self.settings
            && let Ok((settings, _)) =
                settings.positioned(self.screen, WindowKind::Main, &placement, manual)
        {
            self.save_settings(settings);
        }
    }
    pub(super) fn flush_main_geometry(&mut self) {
        self.geometry.state.borrow_mut().timer.stop();
        self.main_geometry_changed();
        let mut state = self.geometry.state.borrow_mut();
        state.active = false;
        state.timer.stop();
        // Slint can destroy the HWND when hidden and recreate it with old
        // physical attributes. Reopen must restore saved logical bounds, even
        // within the same controller after a monitor/DPI change.
        state.restored = false;
        state.expected = None;
        state.resized = false;
    }
    #[cfg(test)]
    pub fn test_main_geometry_fixture(&mut self) {
        *self.geometry.fixture.borrow_mut() = Some(Fixture {
            fail_apply: false,
            physical: captions_core::settings::Bounds {
                x: 100.0,
                y: 100.0,
                width: 900.0,
                height: 500.0,
            },
            monitor: geometry::Monitor {
                name: "fixture-main".into(),
                scale: 1.0,
                work_area: captions_core::settings::Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1040.0,
                },
            },
        });
    }
    #[cfg(test)]
    pub fn test_main_fail_next_apply(&mut self) {
        self.geometry
            .fixture
            .borrow_mut()
            .as_mut()
            .unwrap()
            .fail_apply = true;
    }
    #[cfg(test)]
    pub fn test_main_work_height(&mut self, height: f64) {
        self.geometry
            .fixture
            .borrow_mut()
            .as_mut()
            .unwrap()
            .monitor
            .work_area
            .height = height;
        self.sync_main_geometry();
    }
    #[cfg(test)]
    pub fn test_main_recreated_native(&mut self, scale: f64) {
        let mut fixture = self.geometry.fixture.borrow_mut();
        let fixture = fixture.as_mut().unwrap();
        // Simulate winit reusing old physical creation attributes after hiding;
        // physical size stays unchanged while the new monitor DPI differs.
        fixture.monitor.scale = scale;
        fixture.monitor.work_area.width = 1920.0 * scale;
        fixture.monitor.work_area.height = 1040.0 * scale;
    }
    #[cfg(test)]
    pub fn test_main_geometry(&self) -> Placement {
        self.geometry
            .snapshot(&self.ui.upgrade().unwrap())
            .unwrap()
            .unwrap()
    }
    #[cfg(test)]
    pub fn test_main_native_resize(
        &mut self,
        bounds: captions_core::settings::Bounds,
        flush: bool,
    ) {
        self.geometry
            .fixture
            .borrow_mut()
            .as_mut()
            .unwrap()
            .physical = bounds;
        self.geometry.state.borrow_mut().resized = true;
        self.ui
            .upgrade()
            .unwrap()
            .window()
            .set_size(slint::LogicalSize::new(
                bounds.width as f32,
                bounds.height as f32,
            ));
        if flush {
            self.main_geometry_changed();
        }
    }
}
