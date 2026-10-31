//! The layered caption HWND and all rendering resources live on one native
//! message-loop thread. Text updates are coalesced; static captions have no timer.
use crate::overlay_text::{Canvas, ColorPrefix, TextStyle};
use crate::window_geometry;
use captions_core::{
    geometry::{Anchor, AutoHeight, Placement},
    settings::Bounds,
};
use std::{
    cell::{Cell, RefCell},
    io,
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
        Graphics::{
            Direct2D::Common::D2D_RECT_F,
            Gdi::{AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION, ScreenToClient, ValidateRect},
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::WM_MOUSELEAVE,
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{
                ReleaseCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
            },
            WindowsAndMessaging::*,
        },
    },
    core::w,
};

// One compact dock, with gaps separating typography, background and display.
fn toolbar_button_left(width: f32, index: usize) -> f32 {
    (width - 436.0) / 2.0
        + 7.0
        + index as f32 * 34.0
        + if index >= 6 { 7.0 } else { 0.0 }
        + if index >= 9 { 7.0 } else { 0.0 }
}

const UPDATE: u32 = WM_APP + 21;
const PAINT: u32 = WM_APP + 22;
const GEOMETRY: u32 = WM_APP + 23;
const DPI_APPLY: u32 = WM_APP + 24;
const WORKAREA_CHANGED: u32 = WM_APP + 25;
const TOOLTIP_TIMER: usize = 1;
const TOOLBAR_TIMER: usize = 2;
#[cfg(test)]
const TEST_ACTION: u32 = WM_APP + 120;
#[cfg(test)]
const TEST_FENCE: u32 = WM_APP + 121;
#[cfg(test)]
const TEST_COMMAND: u32 = WM_APP + 122;
#[cfg(test)]
enum TestOperation {
    Dpi(RECT),
    Hit(LPARAM),
    Close,
    ResizeDuringPaint(RECT),
    PaintCount,
    ReentrantPaint,
    StaleWorkArea(bool),
    WorkAreaHeight,
    TooltipState,
    TooltipElapsed,
    ToolbarSignature(usize),
    Snapshot(std::path::PathBuf),
}
#[cfg(test)]
struct TestCommand {
    operation: TestOperation,
    reply: mpsc::SyncSender<isize>,
}
#[cfg(test)]
struct TestDpi(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT);
#[cfg(test)]
impl Drop for TestDpi {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(self.0);
        }
    }
}
#[cfg(test)]
fn test_dpi() -> TestDpi {
    let previous = unsafe {
        windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
    };
    assert!(!previous.0.is_null());
    TestDpi(previous)
}
const COLORS: [u32; 8] = [
    0xffffff, 0xffff00, 0x32cd32, 0x00ffff, 0x0000ff, 0xff1493, 0xff0000, 0x000000,
];
const LABELS: [&str; 12] = [
    "A+", "A−", "描+", "描−", "B", "色", "◐+", "◐−", "底", "▤", "↕", "穿",
];
const TOOLTIPS: [&str; 12] = [
    "增大字号",
    "减小字号",
    "加粗描边",
    "减细描边",
    "粗体",
    "字体颜色",
    "提高背景不透明度",
    "降低背景不透明度",
    "背景颜色",
    "仅显示原字幕或译文",
    "切换原字幕与译文顺序",
    "鼠标穿透",
];

fn toolbar_hint(index: usize, settings: &Settings, mode: u8, source_first: bool) -> String {
    match index {
        0 | 1 => format!("{}（当前 {}）", TOOLTIPS[index], settings.font_size),
        2 | 3 => format!("{}（当前 {:.1}）", TOOLTIPS[index], settings.stroke),
        4 => {
            let states = ["关闭", "译文", "原文", "双语"];
            format!(
                "粗体：{}（切换为{}）",
                states[settings.bold as usize],
                states[(settings.bold as usize + 1) % 4]
            )
        }
        6 | 7 => format!(
            "{}（当前 {}%）",
            TOOLTIPS[index],
            u32::from(settings.opacity) * 100 / 255
        ),
        9 => {
            let states = ["双语", "仅译文", "仅原文"];
            format!(
                "显示：{}（切换为{}）",
                states[mode as usize],
                states[(mode as usize + 1) % 3]
            )
        }
        10 => format!(
            "顺序：{}在上（切换顺序）",
            if source_first { "原文" } else { "译文" }
        ),
        11 => "启用鼠标穿透；重新开启字幕后恢复操作".into(),
        _ => TOOLTIPS[index].into(),
    }
}

fn toolbar_active(index: usize, bold: u8, mode: u8, source_first: bool) -> bool {
    match index {
        4 => bold != 0,
        9 => mode != 0,
        10 => source_first,
        _ => false,
    }
}

#[derive(Clone, Default)]
pub struct Content {
    pub source: String,
    pub translation: String,
    pub notice: String,
    pub previous_translation_units: u32,
}
impl Content {
    pub fn set_translation(&mut self, combined: String, current: &str) {
        self.previous_translation_units = combined
            .strip_suffix(current)
            .map_or(0, |prefix| prefix.encode_utf16().count() as u32);
        self.translation = combined;
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub font_size: u8,
    pub font_color: u8,
    pub bold: u8,
    pub opacity: u8,
    pub background: u8,
    pub stroke: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            font_size: 15,
            font_color: 1,
            bold: 0,
            opacity: 150,
            background: 8,
            stroke: 0.0,
        }
    }
}
pub enum Event {
    Closed,
    StyleChanged(Settings),
    Failed(String),
    PlacementChanged(Placement, bool),
    PlacementFailed(String),
}
#[derive(Clone)]
pub struct SavedGeometry {
    pub bounds: Bounds,
    pub anchor: Option<Anchor>,
    pub manual_size: bool,
}
pub type Callback = Arc<dyn Fn(Event) + Send + Sync>;
struct Shared {
    content: Mutex<Option<Content>>,
    callback: Callback,
    window: Mutex<Option<usize>>,
}
impl Shared {
    fn post(&self, message: u32) {
        // Serialize posting with removal before DestroyWindow, so a recycled
        // HWND can never receive our update or close message.
        let window = self.window.lock().unwrap();
        if let Some(window) = *window {
            unsafe {
                let _ = PostMessageW(Some(HWND(window as *mut _)), message, WPARAM(0), LPARAM(0));
            }
        }
    }
}

pub struct Overlay {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}
impl Overlay {
    pub fn open(
        settings: Settings,
        content: Content,
        geometry: SavedGeometry,
        callback: Callback,
    ) -> io::Result<Self> {
        Self::open_with_visibility(settings, content, geometry, callback, true)
    }
    fn open_with_visibility(
        settings: Settings,
        content: Content,
        geometry: SavedGeometry,
        callback: Callback,
        visible: bool,
    ) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            content: Mutex::new(Some(content)),
            callback,
            window: Mutex::new(None),
        });
        let background = shared.clone();
        let (ready, result) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("caption-overlay".into())
            .spawn(move || {
                if let Err(error) = run(settings, geometry, background.clone(), ready, visible) {
                    (background.callback)(Event::Failed(error.to_string()));
                }
            })?;
        match result.recv() {
            Ok(Ok(_window)) => Ok(Self {
                shared,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(io::Error::other(error))
            }
            Err(_) => {
                let _ = thread.join();
                Err(io::Error::other("悬浮字幕窗口无法启动。"))
            }
        }
    }
    pub fn update(&self, content: Content) {
        let mut pending = self.shared.content.lock().unwrap();
        let wake = pending.is_none();
        *pending = Some(content);
        drop(pending);
        if wake {
            self.shared.post(UPDATE);
        }
    }
}
impl Drop for Overlay {
    fn drop(&mut self) {
        // SAFETY: only this window's owning thread destroys its HWND/resources.
        self.shared.post(WM_CLOSE);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct State {
    window: HWND,
    settings: Settings,
    content: Content,
    shared: Arc<Shared>,
    canvas: Option<Canvas>,
    hover: bool,
    toolbar_opacity: f32,
    toolbar_transition: Option<(Instant, f32, f32)>,
    button: Option<usize>,
    tooltip: Option<usize>,
    tooltip_due: Option<Instant>,
    mode: u8,
    source_first: bool,
    click_through: bool,
    both_rect: RECT,
    signals: Rc<Signals>,
    work_area: Bounds,
    #[cfg(test)]
    resize_before_present: Option<RECT>,
    #[cfg(test)]
    paint_count: u64,
}
struct Signals {
    scale: Cell<f32>,
    minimum_height: Cell<f32>,
    manual_size: Cell<bool>,
    moving: Cell<bool>,
    paint_pending: Cell<bool>,
    geometry_pending: Cell<bool>,
    dpi_pending: Cell<bool>,
    dpi: Cell<Option<(u32, RECT)>>,
    revision: Cell<u64>,
}
impl Signals {
    fn post(window: HWND, message: u32, pending: &Cell<bool>) {
        if !pending.replace(true)
            && unsafe { PostMessageW(Some(window), message, WPARAM(0), LPARAM(0)) }.is_err()
        {
            pending.set(false);
        }
    }
    fn paint(&self, window: HWND) {
        Self::post(window, PAINT, &self.paint_pending);
    }
    fn geometry(&self, window: HWND) {
        if !self.moving.get() {
            Self::post(window, GEOMETRY, &self.geometry_pending);
        }
    }
    fn dpi(&self, window: HWND) {
        Self::post(window, DPI_APPLY, &self.dpi_pending);
    }
    fn changed(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
    }
}
struct WindowData {
    state: RefCell<State>,
    signals: Rc<Signals>,
}
impl State {
    fn animate_toolbar(&mut self, visible: bool) {
        let target = if visible { 1.0 } else { 0.0 };
        self.toolbar_transition = Some((Instant::now(), self.toolbar_opacity, target));
        self.toolbar_tick();
    }
    fn toolbar_tick(&mut self) {
        let Some((started, from, target)) = self.toolbar_transition else {
            return;
        };
        let progress = (started.elapsed().as_secs_f32() / 0.18).min(1.0);
        self.toolbar_opacity = from + (target - from) * (1.0 - (1.0 - progress).powi(3));
        unsafe {
            if progress >= 1.0 || self.click_through {
                self.toolbar_opacity = if self.click_through { 0.0 } else { target };
                self.toolbar_transition = None;
                let _ = KillTimer(Some(self.window), TOOLBAR_TIMER);
            } else {
                SetTimer(Some(self.window), TOOLBAR_TIMER, 16, None);
            }
        }
        self.redraw();
    }
    fn clear_tooltip(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.window), TOOLTIP_TIMER);
        }
        self.tooltip = None;
        self.tooltip_due = None;
    }
    fn hover_button(&mut self, button: Option<usize>) {
        if self.button == button {
            return;
        }
        self.clear_tooltip();
        self.button = button;
        if button.is_some() && self.hover && !self.click_through {
            self.tooltip_due = Some(Instant::now() + Duration::from_millis(500));
            unsafe {
                SetTimer(Some(self.window), TOOLTIP_TIMER, 500, None);
            }
        }
    }
    fn tooltip_tick(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.window), TOOLTIP_TIMER);
        }
        let Some(due) = self.tooltip_due else {
            return;
        };
        let remaining = due.saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            unsafe {
                SetTimer(
                    Some(self.window),
                    TOOLTIP_TIMER,
                    remaining.as_millis().max(1) as u32,
                    None,
                );
            }
            return;
        }
        self.tooltip_due = None;
        if self.hover && !self.click_through {
            self.tooltip = self.button;
            self.redraw();
        }
    }
    fn placement(&mut self) -> io::Result<Option<Placement>> {
        let Some(mut value) = window_geometry::snapshot(self.window.0 as usize)? else {
            return Ok(None);
        };
        self.work_area = window_geometry::work_area(self.window.0 as usize)?;
        let scale = unsafe { GetDpiForWindow(self.window) }.max(96) as f32 / 96.0;
        if self.signals.scale.get() != scale {
            self.signals.scale.set(scale);
            self.canvas = None;
        }
        match self.mode {
            1 => {
                value.bounds.y -= 40.0;
                value.bounds.height += 40.0;
                value.anchor.y -= 40.0;
            }
            2 => value.bounds.height += 40.0,
            _ => {}
        }
        self.both_rect = physical_rect(Bounds {
            x: value.bounds.x * f64::from(scale),
            y: value.bounds.y * f64::from(scale),
            width: value.bounds.width * f64::from(scale),
            height: value.bounds.height * f64::from(scale),
        });
        Ok(Some(value))
    }
    fn publish_geometry(&mut self) {
        match self.placement() {
            Ok(Some(value)) => (self.shared.callback)(Event::PlacementChanged(
                value,
                self.signals.manual_size.get(),
            )),
            Ok(None) => {}
            Err(error) => (self.shared.callback)(Event::PlacementFailed(error.to_string())),
        }
    }
    fn repair_geometry(&mut self) -> io::Result<()> {
        let Some(value) = self.placement()? else {
            return Ok(());
        };
        let restored = window_geometry::restore(value.bounds, Some(&value.anchor), (650.0, 135.0))?;
        self.signals.scale.set(restored.scale as f32);
        self.work_area = restored.work_area;
        self.both_rect = physical_rect(restored.physical);
        window_geometry::apply(
            self.window.0 as usize,
            rect_bounds(mode_rect(
                self.both_rect,
                self.mode,
                self.signals.scale.get(),
            )),
        )?;
        self.canvas = None;
        self.publish_geometry();
        self.redraw();
        Ok(())
    }
    fn redraw(&self) {
        self.signals.paint(self.window);
    }
    fn paint(&mut self) -> windows::core::Result<()> {
        #[cfg(test)]
        {
            self.paint_count += 1;
        }
        if self.signals.dpi.get().is_some() {
            self.signals.dpi(self.window);
            self.redraw();
            return Ok(());
        }
        let revision = self.signals.revision.get();
        let mut rect = RECT::default();
        unsafe {
            GetClientRect(self.window, &mut rect)?;
        }
        let (width, height) = (rect.right.max(1), rect.bottom.max(1));
        if self
            .canvas
            .as_ref()
            .is_none_or(|canvas| canvas.width != width || canvas.height != height)
        {
            self.canvas = Some(Canvas::new(width, height, self.signals.scale.get())?);
        }
        let canvas = self.canvas.as_mut().unwrap();
        let logical_width = width as f32 / self.signals.scale.get();
        let logical_height = height as f32 / self.signals.scale.get();
        canvas.begin(palette(self.settings.background), self.settings.opacity);
        let result = (|| -> windows::core::Result<f32> {
            let source_style = TextStyle {
                size: self.settings.font_size as f32,
                bold: self.settings.bold & 2 != 0,
                rgb: palette(self.settings.font_color),
                stroke: self.settings.stroke,
                prefix: None,
            };
            let translated_style = TextStyle {
                size: (self.settings.font_size as f32 * 1.25).floor(),
                bold: self.settings.bold & 1 != 0,
                rgb: translation_color(source_style.rgb),
                stroke: self.settings.stroke,
                prefix: Some(ColorPrefix {
                    utf16_len: self.content.previous_translation_units,
                    rgb: source_style.rgb,
                }),
            };
            let mut top = 12.0;
            if !self.content.notice.is_empty() {
                top += canvas.text(
                    3,
                    &self.content.notice,
                    13.0,
                    top,
                    (logical_width - 26.0).max(1.0),
                    &source_style,
                )? + 8.0;
            }
            let order = if self.source_first {
                [true, false]
            } else {
                [false, true]
            };
            for source in order {
                if (source && self.mode == 1) || (!source && self.mode == 2) {
                    continue;
                }
                let (slot, text, style) = if source {
                    (0, &self.content.source, &source_style)
                } else {
                    (1, &self.content.translation, &translated_style)
                };
                top += canvas
                    .text(
                        slot,
                        text,
                        13.0,
                        top,
                        (logical_width - 26.0).max(1.0),
                        style,
                    )?
                    .max(style.size * 1.3)
                    + 8.0;
            }
            if self.toolbar_opacity > 0.0 && !self.click_through {
                let opacity = (self.toolbar_opacity * 255.0).round() as u8;
                let style = TextStyle {
                    size: 12.0,
                    bold: false,
                    rgb: 0xffffff,
                    stroke: 0.0,
                    prefix: None,
                };
                let dock_left = toolbar_button_left(logical_width, 0) - 7.0;
                canvas.rounded_rectangle(
                    D2D_RECT_F {
                        left: dock_left,
                        top: logical_height - 40.0,
                        right: dock_left + 436.0,
                        bottom: logical_height - 4.0,
                    },
                    10.0,
                    0x20242c,
                    (self.toolbar_opacity * 238.0).round() as u8,
                )?;
                for group in [6, 9] {
                    let x = toolbar_button_left(logical_width, group) - 5.0;
                    canvas.rectangle(
                        D2D_RECT_F {
                            left: x,
                            top: logical_height - 30.0,
                            right: x + 1.0,
                            bottom: logical_height - 14.0,
                        },
                        0xffffff,
                        (self.toolbar_opacity * 40.0).round() as u8,
                    )?;
                }
                for index in 0..LABELS.len() {
                    let left = toolbar_button_left(logical_width, index);
                    let selected =
                        toolbar_active(index, self.settings.bold, self.mode, self.source_first);
                    if self.button == Some(index) || selected {
                        canvas.rounded_rectangle(
                            D2D_RECT_F {
                                left,
                                top: logical_height - 36.0,
                                right: left + 32.0,
                                bottom: logical_height - 8.0,
                            },
                            6.0,
                            if selected { 0x83d7ff } else { 0xffffff },
                            (self.toolbar_opacity * 42.0).round() as u8,
                        )?;
                    }
                    let icon_color = if selected { 0x83d7ff } else { 0xffffff };
                    let top = logical_height - 30.0;
                    if index == 9 {
                        // Two caption rows: preserve source/translation order, and
                        // dim the row excluded by the current display mode.
                        for row in 0..2 {
                            let source = (row == 0) == self.source_first;
                            let shown = if source {
                                self.mode != 1
                            } else {
                                self.mode != 2
                            };
                            canvas.rounded_rectangle(
                                D2D_RECT_F {
                                    left: left + 8.0,
                                    top: top + row as f32 * 8.0 + 1.0,
                                    right: left + 24.0,
                                    bottom: top + row as f32 * 8.0 + 6.0,
                                },
                                1.5,
                                icon_color,
                                if shown { opacity } else { opacity / 5 },
                            )?;
                        }
                    } else {
                        canvas.icon(index, left + 8.0, top, 16.0, opacity, icon_color)?;
                    }
                    if index == 4 {
                        // The two marks distinguish all four bold states.
                        for row in 0..2 {
                            let source = (row == 0) == self.source_first;
                            let enabled = self.settings.bold & (if source { 2 } else { 1 }) != 0;
                            canvas.rectangle(
                                D2D_RECT_F {
                                    left: left + 25.0,
                                    top: top + row as f32 * 8.0 + 3.0,
                                    right: left + 28.0,
                                    bottom: top + row as f32 * 8.0 + 6.0,
                                },
                                0x83d7ff,
                                if enabled { opacity } else { opacity / 5 },
                            )?;
                        }
                    }
                    if index == 5 || index == 8 {
                        canvas.rectangle(
                            D2D_RECT_F {
                                left: left + 10.0,
                                top: top + 18.0,
                                right: left + 22.0,
                                bottom: top + 20.0,
                            },
                            palette(if index == 5 {
                                self.settings.font_color
                            } else {
                                self.settings.background
                            }),
                            opacity,
                        )?;
                    }
                }
                if let Some(index) = self.tooltip {
                    let text = toolbar_hint(index, &self.settings, self.mode, self.source_first);
                    let width =
                        (text.chars().count() as f32 * 12.0 + 16.0).min(logical_width - 16.0);
                    let left = (toolbar_button_left(logical_width, index) + 16.0 - width / 2.0)
                        .clamp(8.0, logical_width - width - 8.0);
                    canvas.rectangle(
                        D2D_RECT_F {
                            left,
                            top: logical_height - 70.0,
                            right: left + width,
                            bottom: logical_height - 42.0,
                        },
                        0x252525,
                        255,
                    )?;
                    canvas.text(
                        2,
                        &text,
                        left + 8.0,
                        logical_height - 65.0,
                        width - 16.0,
                        &style,
                    )?;
                }
            }
            Ok(top + 12.0)
        })();
        let finished = canvas.finish();
        let required_height = result?;
        finished?;
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        if self.signals.revision.get() != revision {
            self.redraw();
            return Ok(());
        }
        let size = SIZE {
            cx: width,
            cy: height,
        };
        let origin = POINT::default();
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcSrc: canvas.dc,
            pptSrc: &origin,
            psize: &size,
            pblend: &blend,
            dwFlags: UPDATE_LAYERED_WINDOW_FLAGS(ULW_ALPHA.0 | ULW_EX_NORESIZE.0),
            ..Default::default()
        };
        #[cfg(test)]
        if let Some(rect) = self.resize_before_present.take() {
            unsafe {
                SetWindowPos(
                    self.window,
                    None,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )?;
            }
        }
        let presented = unsafe { UpdateLayeredWindowIndirect(self.window, &info) };
        if self.signals.revision.get() != revision {
            self.redraw();
            return Ok(());
        }
        presented.ok()?;
        let minimum = if self.mode == 0 { 135.0 } else { 95.0 };
        let available = (self.work_area.height / f64::from(self.signals.scale.get())).max(minimum);
        let required = f64::from(required_height).max(minimum).min(available);
        let desired = AutoHeight::new(self.signals.manual_size.get()).adjusted(
            f64::from(height) / f64::from(self.signals.scale.get()),
            required,
        );
        let desired = (desired * f64::from(self.signals.scale.get())).ceil() as i32;
        if !self.signals.moving.get() && (desired - height).abs() > 1 {
            let mut position = RECT::default();
            unsafe {
                GetWindowRect(self.window, &mut position)?;
            }
            let mut top = position.top;
            // Keep automatically growing, previously on-screen content and its
            // controls reachable. Do not move intentionally straddling bounds.
            if desired > height
                && f64::from(position.top) >= self.work_area.y
                && f64::from(position.bottom) <= self.work_area.y + self.work_area.height
                && f64::from(position.top + desired) > self.work_area.y + self.work_area.height
            {
                top =
                    (self.work_area.y + self.work_area.height - f64::from(desired)).round() as i32;
            }
            unsafe {
                SetWindowPos(
                    self.window,
                    None,
                    position.left,
                    top,
                    width,
                    desired,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )?;
            }
            self.publish_geometry();
            self.redraw();
        }
        Ok(())
    }
    fn pointer_button(&self, x: i32, y: i32) -> Option<usize> {
        if !self.hover || self.click_through {
            return None;
        }
        let mut rect = RECT::default();
        unsafe {
            let _ = GetClientRect(self.window, &mut rect);
        }
        let width = rect.right as f32 / self.signals.scale.get();
        let height = rect.bottom as f32 / self.signals.scale.get();
        let (x, y) = (
            x as f32 / self.signals.scale.get(),
            y as f32 / self.signals.scale.get(),
        );
        if y < height - 36.0 || y > height - 8.0 {
            return None;
        }
        (0..LABELS.len()).find(|&index| {
            let left = toolbar_button_left(width, index);
            x >= left && x < left + 32.0
        })
    }
    fn action(&mut self, index: usize) {
        match index {
            0 => self.settings.font_size = (self.settings.font_size + 1).min(40),
            1 => self.settings.font_size = self.settings.font_size.saturating_sub(1).max(8),
            2 => self.settings.stroke = (self.settings.stroke + 1.5).min(7.5),
            3 => self.settings.stroke = (self.settings.stroke - 1.5).max(0.0),
            4 => self.settings.bold = (self.settings.bold + 1) % 4,
            5 => self.settings.font_color = self.settings.font_color % 8 + 1,
            6 => self.settings.opacity = self.settings.opacity.saturating_add(25).min(251),
            7 => self.settings.opacity = self.settings.opacity.saturating_sub(25).max(1),
            8 => self.settings.background = self.settings.background % 8 + 1,
            9 => self.change_mode(),
            10 => self.source_first = !self.source_first,
            11 => {
                self.clear_tooltip();
                self.click_through = true;
                self.hover = false;
                self.button = None;
                unsafe {
                    let style = GetWindowLongPtrW(self.window, GWL_EXSTYLE);
                    SetWindowLongPtrW(
                        self.window,
                        GWL_EXSTYLE,
                        style | WS_EX_TRANSPARENT.0 as isize,
                    );
                }
            }
            _ => return,
        }
        (self.shared.callback)(Event::StyleChanged(self.settings.clone()));
        self.redraw();
    }
    fn change_mode(&mut self) {
        unsafe {
            if self.mode == 0 {
                let _ = GetWindowRect(self.window, &mut self.both_rect);
            }
            self.mode = (self.mode + 1) % 3;
            self.signals
                .minimum_height
                .set(if self.mode == 0 { 135.0 } else { 95.0 });
            let rect = mode_rect(self.both_rect, self.mode, self.signals.scale.get());
            let _ = SetWindowPos(
                self.window,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

fn palette(value: u8) -> u32 {
    COLORS[(value.clamp(1, 8) - 1) as usize]
}
pub fn translation_color(rgb: u32) -> u32 {
    let (r, g, b) = (
        ((rgb >> 16) & 255) as f32,
        ((rgb >> 8) & 255) as f32,
        (rgb & 255) as f32,
    );
    let target = if 0.299 * r + 0.587 * g + 0.114 * b > 127.0 {
        0.0
    } else {
        255.0
    };
    let channel =
        |value: f32, amount: f32| (value + (target - value) * amount).clamp(0.0, 255.0) as u32;
    channel(r, 0.3) << 16 | channel(g, 0.4) << 8 | channel(b, 0.3)
}
fn mode_rect(both: RECT, mode: u8, scale: f32) -> RECT {
    let delta = (40.0 * scale).round() as i32;
    match mode {
        1 => RECT {
            top: both.top + delta,
            ..both
        },
        2 => RECT {
            bottom: both.bottom - delta,
            ..both
        },
        _ => both,
    }
}
fn physical_rect(bounds: Bounds) -> RECT {
    RECT {
        left: bounds.x.round() as i32,
        top: bounds.y.round() as i32,
        right: (bounds.x + bounds.width).round() as i32,
        bottom: (bounds.y + bounds.height).round() as i32,
    }
}
fn rect_bounds(rect: RECT) -> Bounds {
    Bounds {
        x: f64::from(rect.left),
        y: f64::from(rect.top),
        width: f64::from(rect.right - rect.left),
        height: f64::from(rect.bottom - rect.top),
    }
}
fn platform_error(error: io::Error) -> windows::core::Error {
    windows::core::Error::new(
        windows::core::HRESULT(0x80004005u32 as i32),
        error.to_string(),
    )
}
fn point(parameter: LPARAM) -> (i32, i32) {
    (
        (parameter.0 as u16 as i16) as i32,
        ((parameter.0 >> 16) as u16 as i16) as i32,
    )
}
fn hit_test(window: HWND, parameter: LPARAM, scale: f32) -> LRESULT {
    let (x, y) = point(parameter);
    let mut rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(window, &mut rect);
    }
    let border = (5.0 * scale).round() as i32;
    let hit = match (
        x < rect.left + border,
        x >= rect.right - border,
        y < rect.top + border,
        y >= rect.bottom - border,
    ) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, _, _, _) => HTLEFT,
        (_, true, _, _) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ => HTCLIENT,
    };
    LRESULT(hit as isize)
}

fn run(
    settings: Settings,
    geometry: SavedGeometry,
    shared: Arc<Shared>,
    ready: mpsc::SyncSender<Result<usize, String>>,
    visible: bool,
) -> io::Result<()> {
    // Unit-test binaries have no EXE manifest; match the product's existing
    // PerMonitorV2 context on this thread only, restoring it when the test ends.
    #[cfg(test)]
    let _dpi = test_dpi();
    // A callback or early-return failure must always release the startup receiver.
    let result = (|| -> windows::core::Result<()> {
        let placement =
            window_geometry::restore(geometry.bounds, geometry.anchor.as_ref(), (650.0, 135.0))
                .map_err(platform_error)?;
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }?.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
            lpszClassName: w!("LiveCaptionsTray.Overlay"),
            ..Default::default()
        };
        let signals = Rc::new(Signals {
            scale: Cell::new(placement.scale as f32),
            minimum_height: Cell::new(135.0),
            manual_size: Cell::new(geometry.manual_size),
            moving: Cell::new(false),
            paint_pending: Cell::new(false),
            geometry_pending: Cell::new(false),
            dpi_pending: Cell::new(false),
            dpi: Cell::new(None),
            revision: Cell::new(0),
        });
        let mut state = Box::new(WindowData {
            signals: signals.clone(),
            state: RefCell::new(State {
                window: HWND::default(),
                settings,
                content: Content::default(),
                shared,
                canvas: None,
                hover: false,
                toolbar_opacity: 0.0,
                toolbar_transition: None,
                button: None,
                tooltip: None,
                tooltip_due: None,
                mode: 0,
                source_first: false,
                click_through: false,
                both_rect: physical_rect(placement.physical),
                signals,
                work_area: placement.work_area,
                #[cfg(test)]
                resize_before_present: None,
                #[cfg(test)]
                paint_count: 0,
            }),
        });
        let window = unsafe {
            // This class may already exist after closing and reopening an overlay.
            RegisterClassW(&class);
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class.lpszClassName,
                w!("悬浮字幕窗口"),
                WS_POPUP,
                placement.physical.x.round() as i32,
                placement.physical.y.round() as i32,
                placement.physical.width.round() as i32,
                placement.physical.height.round() as i32,
                None,
                None,
                Some(instance),
                Some((&mut *state as *mut WindowData).cast()),
            )?
        };
        let startup = (|| -> windows::core::Result<()> {
            let mut state = state.state.borrow_mut();
            state.window = window;
            state
                .signals
                .scale
                .set((unsafe { GetDpiForWindow(window) }.max(96) as f32) / 96.0);
            unsafe {
                SetWindowPos(
                    window,
                    Some(HWND_TOPMOST),
                    placement.physical.x.round() as i32,
                    placement.physical.y.round() as i32,
                    placement.physical.width.round() as i32,
                    placement.physical.height.round() as i32,
                    SWP_NOACTIVATE,
                )?;
            }
            let pending = state.shared.content.lock().unwrap().take();
            if let Some(content) = pending {
                state.content = content;
            }
            state.paint()?;
            *state.shared.window.lock().unwrap() = Some(window.0 as usize);
            if visible {
                unsafe {
                    let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
                }
            }
            Ok(())
        })();
        if let Err(error) = startup {
            unsafe {
                let _ = DestroyWindow(window);
            }
            return Err(error);
        }
        let _ = ready.send(Ok(window.0 as usize));
        let mut message = MSG::default();
        unsafe {
            loop {
                let status = GetMessageW(&mut message, None, 0, 0).0;
                if status == 0 {
                    break;
                }
                if status < 0 {
                    state.state.borrow().shared.window.lock().unwrap().take();
                    let _ = DestroyWindow(window);
                    return Err(windows::core::Error::from_thread());
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        let message = error.to_string();
        let _ = ready.send(Err(message.clone()));
        return Err(io::Error::other(message));
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: userdata points to run()'s stable RefCell until the native loop ends.
    // try_borrow_mut prevents aliasing when Win32 synchronously re-enters us.
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        if message == WM_DESTROY {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *const WindowData;
        if pointer.is_null() {
            return DefWindowProcW(window, message, wparam, lparam);
        }
        let data = &*pointer;
        match message {
            WM_SETTINGCHANGE if wparam.0 == SPI_SETWORKAREA.0 as usize => {
                // Never retain or repost the system message's borrowed string.
                let _ = PostMessageW(Some(window), WORKAREA_CHANGED, WPARAM(0), LPARAM(0));
                return LRESULT(0);
            }
            WM_NCHITTEST => return hit_test(window, lparam, data.signals.scale.get()),
            WM_SETCURSOR
                if data.signals.moving.get()
                    && matches!(lparam.0 as u16 as u32, HTCLIENT | HTCAPTION) =>
            {
                if let Ok(cursor) = LoadCursorW(None, IDC_SIZEALL) {
                    SetCursor(Some(cursor));
                }
                return LRESULT(1);
            }
            #[cfg(test)]
            TEST_FENCE => {
                let sender = Box::from_raw(lparam.0 as *mut mpsc::SyncSender<()>);
                let _ = sender.send(());
                return LRESULT(0);
            }
            WM_SIZING => {
                data.signals.manual_size.set(true);
                return LRESULT(1);
            }
            WM_ENTERSIZEMOVE => {
                data.signals.moving.set(true);
                return LRESULT(0);
            }
            WM_EXITSIZEMOVE => {
                data.signals.moving.set(false);
                data.signals.geometry(window);
                data.signals.paint(window);
                return LRESULT(0);
            }
            WM_MOVE | WM_SIZE => {
                data.signals.changed();
                data.signals.geometry(window);
                if message == WM_SIZE {
                    data.signals.paint(window);
                }
                return LRESULT(0);
            }
            WM_DPICHANGED => {
                let dpi = wparam.0 as u16 as u32;
                data.signals.scale.set(dpi as f32 / 96.0);
                data.signals
                    .dpi
                    .set(Some((dpi, *(lparam.0 as *const RECT))));
                data.signals.changed();
                data.signals.dpi(window);
                return LRESULT(0);
            }
            WM_GETMINMAXINFO => {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                info.ptMinTrackSize = POINT {
                    x: (650.0 * data.signals.scale.get()) as i32,
                    y: (data.signals.minimum_height.get() * data.signals.scale.get()) as i32,
                };
                return LRESULT(0);
            }
            _ => {}
        }
        let Ok(mut state) = data.state.try_borrow_mut() else {
            match message {
                WM_CLOSE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_MOUSEMOVE | WM_MOUSELEAVE
                | UPDATE | WM_DISPLAYCHANGE | WORKAREA_CHANGED => {
                    let _ = PostMessageW(Some(window), message, wparam, lparam);
                }
                WM_TIMER if matches!(wparam.0, TOOLTIP_TIMER | TOOLBAR_TIMER) => {
                    let _ = KillTimer(Some(window), wparam.0);
                    let _ = PostMessageW(Some(window), message, wparam, LPARAM(0));
                }
                #[cfg(test)]
                TEST_ACTION | TEST_COMMAND => {
                    let _ = PostMessageW(Some(window), message, wparam, lparam);
                }
                DPI_APPLY => {
                    data.signals.dpi_pending.set(false);
                    data.signals.dpi(window);
                }
                GEOMETRY => {
                    data.signals.geometry_pending.set(false);
                    data.signals.geometry(window);
                }
                PAINT | WM_PAINT => {
                    data.signals.paint_pending.set(false);
                    let _ = ValidateRect(Some(window), None);
                    data.signals.paint(window);
                }
                _ => return DefWindowProcW(window, message, wparam, lparam),
            }
            return LRESULT(0);
        };
        {
            if state.window.0.is_null() {
                state.window = window;
            }
            match message {
                #[cfg(test)]
                TEST_ACTION => state.action(wparam.0),
                #[cfg(test)]
                TEST_COMMAND => {
                    let command = Box::from_raw(lparam.0 as *mut TestCommand);
                    let result = match command.operation {
                        TestOperation::Dpi(mut rect) => {
                            let dpi = GetDpiForWindow(window);
                            SendMessageW(
                                window,
                                WM_DPICHANGED,
                                Some(WPARAM((dpi as usize) << 16 | dpi as usize)),
                                Some(LPARAM((&mut rect as *mut RECT) as isize)),
                            )
                            .0
                        }
                        TestOperation::Hit(parameter) => {
                            SendMessageW(window, WM_NCHITTEST, None, Some(parameter)).0
                        }
                        TestOperation::Close => SendMessageW(window, WM_CLOSE, None, None).0,
                        TestOperation::ResizeDuringPaint(rect) => {
                            data.signals.manual_size.set(true);
                            state.resize_before_present = Some(rect);
                            state.redraw();
                            0
                        }
                        TestOperation::PaintCount => state.paint_count as isize,
                        TestOperation::ReentrantPaint => {
                            data.signals.paint_pending.set(true);
                            SendMessageW(window, PAINT, None, None).0
                        }
                        TestOperation::StaleWorkArea(notification) => {
                            state.work_area.height = 12345.0;
                            if notification {
                                SendMessageW(
                                    window,
                                    WM_SETTINGCHANGE,
                                    Some(WPARAM(SPI_SETWORKAREA.0 as usize)),
                                    Some(LPARAM(w!("WorkArea").as_ptr() as isize)),
                                );
                            } else {
                                SendMessageW(window, GEOMETRY, None, None);
                            }
                            0
                        }
                        TestOperation::WorkAreaHeight => state.work_area.height as isize,
                        TestOperation::TooltipState => {
                            (state.tooltip.map_or(0, |index| index as isize + 1))
                                | if state.tooltip_due.is_some() { 256 } else { 0 }
                                | if state.click_through { 512 } else { 0 }
                        }
                        TestOperation::TooltipElapsed => {
                            if state.tooltip_due.is_some() {
                                state.tooltip_due = Some(Instant::now());
                                state.tooltip_tick();
                            }
                            0
                        }
                        TestOperation::ToolbarSignature(index) => {
                            use std::hash::{Hash, Hasher};
                            state.hover = true;
                            state.toolbar_opacity = 1.0;
                            state.paint().unwrap();
                            let scale = state.signals.scale.get();
                            let canvas = state.canvas.as_ref().unwrap();
                            let left = (toolbar_button_left(canvas.width as f32 / scale, index)
                                * scale)
                                .round() as usize;
                            let top = canvas.height as usize - (32.0 * scale).round() as usize;
                            let right = left + (32.0 * scale).round() as usize;
                            let bottom = canvas.height as usize - (8.0 * scale).round() as usize;
                            let mut hash = std::hash::DefaultHasher::new();
                            for row in top..bottom {
                                let offset = row * canvas.width as usize;
                                canvas.pixels()[(offset + left) * 4..(offset + right) * 4]
                                    .hash(&mut hash);
                            }
                            hash.finish() as isize
                        }
                        TestOperation::Snapshot(path) => {
                            state.paint().unwrap();
                            let canvas = state.canvas.as_ref().unwrap();
                            let mut bmp = Vec::new();
                            bmp.extend_from_slice(b"BM");
                            bmp.extend_from_slice(
                                &(54 + canvas.pixels().len() as u32).to_le_bytes(),
                            );
                            bmp.extend_from_slice(&[0; 4]);
                            bmp.extend_from_slice(&54u32.to_le_bytes());
                            bmp.extend_from_slice(&40u32.to_le_bytes());
                            bmp.extend_from_slice(&canvas.width.to_le_bytes());
                            bmp.extend_from_slice(&(-canvas.height).to_le_bytes());
                            bmp.extend_from_slice(&1u16.to_le_bytes());
                            bmp.extend_from_slice(&32u16.to_le_bytes());
                            bmp.extend_from_slice(&[0; 24]);
                            bmp.extend_from_slice(canvas.pixels());
                            std::fs::write(path, bmp).unwrap();
                            0
                        }
                    };
                    let _ = command.reply.send(result);
                }
                UPDATE => {
                    let content = state.shared.content.lock().unwrap().take();
                    if let Some(content) = content {
                        state.content = content;
                        state.redraw();
                    }
                }
                GEOMETRY => {
                    data.signals.geometry_pending.set(false);
                    state.publish_geometry();
                }
                PAINT | WM_PAINT => {
                    data.signals.paint_pending.set(false);
                    let _ = ValidateRect(Some(window), None);
                    if let Err(error) = state.paint() {
                        (state.shared.callback)(Event::Failed(error.to_string()));
                    }
                }
                WM_ERASEBKGND => return LRESULT(1),
                WM_SETCURSOR if lparam.0 as u16 as u32 == HTCLIENT => {
                    let mut position = POINT::default();
                    let on_button = GetCursorPos(&mut position).is_ok()
                        && ScreenToClient(window, &mut position).as_bool()
                        && state.pointer_button(position.x, position.y).is_some();
                    let resource = if on_button { IDC_HAND } else { IDC_ARROW };
                    if let Ok(cursor) = LoadCursorW(None, resource) {
                        SetCursor(Some(cursor));
                    }
                    return LRESULT(1);
                }
                WM_MOUSEMOVE => {
                    let was = state.hover;
                    state.hover = true;
                    if !was {
                        state.animate_toolbar(true);
                    }
                    let (x, y) = point(lparam);
                    let previous = state.button;
                    let button = state.pointer_button(x, y);
                    state.hover_button(button);
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: window,
                        ..Default::default()
                    };
                    if IsWindowVisible(window).as_bool() {
                        let _ = TrackMouseEvent(&mut track);
                    }
                    if !was || previous != state.button {
                        state.redraw();
                    }
                }
                WM_MOUSELEAVE => {
                    state.hover = false;
                    state.clear_tooltip();
                    state.button = None;
                    state.animate_toolbar(false);
                    state.redraw();
                }
                WM_LBUTTONUP => {
                    let (x, y) = point(lparam);
                    if let Some(index) = state.pointer_button(x, y) {
                        state.action(index);
                    }
                }
                WM_TIMER if wparam.0 == TOOLTIP_TIMER => state.tooltip_tick(),
                WM_TIMER if wparam.0 == TOOLBAR_TIMER => state.toolbar_tick(),
                WM_LBUTTONDOWN => {
                    let (x, y) = point(lparam);
                    if state.pointer_button(x, y).is_none() {
                        // Release the RefCell borrow before the modal native
                        // move loop re-enters paint and resize callbacks.
                        drop(state);
                        let _ = ReleaseCapture();
                        if let Ok(cursor) = LoadCursorW(None, IDC_SIZEALL) {
                            SetCursor(Some(cursor));
                        }
                        SendMessageW(
                            window,
                            WM_NCLBUTTONDOWN,
                            Some(WPARAM(HTCAPTION as usize)),
                            Some(lparam),
                        );
                        if let Ok(cursor) = LoadCursorW(None, IDC_ARROW) {
                            SetCursor(Some(cursor));
                        }
                        return LRESULT(0);
                    }
                }
                DPI_APPLY => {
                    data.signals.dpi_pending.set(false);
                    if let Some((_, rect)) = data.signals.dpi.take() {
                        state.canvas = None;
                        let _ = SetWindowPos(
                            window,
                            None,
                            rect.left,
                            rect.top,
                            rect.right - rect.left,
                            rect.bottom - rect.top,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                        if let Err(error) = state.repair_geometry() {
                            (state.shared.callback)(Event::PlacementFailed(error.to_string()));
                        }
                    }
                }
                WM_DISPLAYCHANGE | WORKAREA_CHANGED => {
                    if let Err(error) = state.repair_geometry() {
                        (state.shared.callback)(Event::PlacementFailed(error.to_string()));
                    }
                }
                WM_CLOSE => {
                    state.clear_tooltip();
                    state.publish_geometry();
                    state.shared.window.lock().unwrap().take();
                    let _ = DestroyWindow(window);
                    (state.shared.callback)(Event::Closed);
                }
                _ => return DefWindowProcW(window, message, wparam, lparam),
            }
            LRESULT(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toolbar_pixels_change_for_each_bold_and_display_state() {
        let _dpi = test_dpi();
        let errors: Arc<Mutex<Vec<String>>> = Arc::default();
        let failed = errors.clone();
        let overlay = Overlay::open_with_visibility(
            Settings::default(),
            short_content(),
            hidden_geometry(true),
            Arc::new(move |event| {
                if let Event::Failed(error) = event {
                    failed.lock().unwrap().push(error);
                }
            }),
            false,
        )
        .unwrap();
        let window = HWND(overlay.shared.window.lock().unwrap().unwrap() as *mut _);
        flush_paint(window);
        assert_eq!(
            unsafe { GetClassLongPtrW(window, GCLP_HCURSOR) },
            unsafe { LoadCursorW(None, IDC_ARROW).unwrap() }.0 as usize,
            "client area must reset the cursor after leaving a resize edge"
        );
        for (index, count) in [(4, 4), (9, 3)] {
            let first = command(window, TestOperation::ToolbarSignature(index));
            let mut signatures = std::collections::HashSet::from([first]);
            for _ in 1..count {
                unsafe { SendMessageW(window, TEST_ACTION, Some(WPARAM(index)), None) };
                flush_paint(window);
                assert!(signatures.insert(command(window, TestOperation::ToolbarSignature(index))));
            }
            unsafe { SendMessageW(window, TEST_ACTION, Some(WPARAM(index)), None) };
            flush_paint(window);
            assert_eq!(
                command(window, TestOperation::ToolbarSignature(index)),
                first
            );
        }
        drop(overlay);
        assert!(errors.lock().unwrap().is_empty());
    }
    #[test]
    fn previous_translation_range_uses_utf16_and_resets_for_new_content() {
        let mut content = Content::default();
        content.set_translation("前文😀 MMM 当前NNN".into(), "当前NNN");
        assert_eq!(content.previous_translation_units, 9);
        content.set_translation("当前NNN".into(), "当前NNN");
        assert_eq!(content.previous_translation_units, 0);
        content.set_translation(String::new(), "");
        assert_eq!(content.previous_translation_units, 0);
        content.set_translation("Unexpected content".into(), "different current");
        assert_eq!(content.previous_translation_units, 0);
    }
    #[test]
    fn hidden_toolbar_tooltips_cancel_on_leave_and_click_through_resets_on_reopen() {
        let _dpi = test_dpi();
        for pass in 0..2 {
            let styles: Arc<Mutex<Vec<Settings>>> = Arc::default();
            let errors: Arc<Mutex<Vec<String>>> = Arc::default();
            let (changed, failed) = (styles.clone(), errors.clone());
            let overlay = Overlay::open_with_visibility(
                Settings::default(),
                short_content(),
                hidden_geometry(true),
                Arc::new(move |event| match event {
                    Event::StyleChanged(style) => changed.lock().unwrap().push(style),
                    Event::Failed(error) => failed.lock().unwrap().push(error),
                    _ => {}
                }),
                false,
            )
            .unwrap();
            let window = HWND(overlay.shared.window.lock().unwrap().unwrap() as *mut _);
            flush_paint(window);
            assert_eq!(command(window, TestOperation::TooltipState), 0);
            assert_eq!(
                unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } & WS_EX_TRANSPARENT.0 as isize,
                0
            );
            let point = |index: usize| {
                let bounds = rect(window);
                let scale = unsafe { GetDpiForWindow(window) }.max(96) as f64 / 96.0;
                let x = f64::from(toolbar_button_left(
                    (f64::from(bounds.right - bounds.left) / scale) as f32,
                    index,
                )) + 19.0;
                let y = f64::from(bounds.bottom - bounds.top) / scale - 16.0;
                LPARAM(
                    (x.mul_add(scale, 0.0).round() as u16 as usize
                        | ((y.mul_add(scale, 0.0).round() as u16 as usize) << 16))
                        as isize,
                )
            };
            for index in 0..12 {
                unsafe {
                    SendMessageW(window, WM_MOUSEMOVE, None, Some(point(index)));
                }
                assert_eq!(command(window, TestOperation::TooltipState), 256);
                command(window, TestOperation::TooltipElapsed);
                assert_eq!(
                    command(window, TestOperation::TooltipState),
                    index as isize + 1
                );
                flush_paint(window); // real DirectWrite tooltip rendering, no visible HWND
                unsafe {
                    SendMessageW(window, WM_MOUSELEAVE, None, None);
                }
                assert_eq!(command(window, TestOperation::TooltipState), 0);
            }
            unsafe {
                SendMessageW(window, WM_MOUSEMOVE, None, Some(point(0)));
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            while command(window, TestOperation::TooltipState) != 1 {
                assert!(
                    Instant::now() < deadline,
                    "native hover timer must show the tooltip"
                );
                thread::sleep(Duration::from_millis(10));
            }
            flush_paint(window);
            let count = command(window, TestOperation::PaintCount);
            thread::sleep(Duration::from_millis(80));
            assert_eq!(
                command(window, TestOperation::PaintCount),
                count,
                "displayed tooltip has no periodic repaint"
            );
            if pass == 0
                && let Some(directory) = std::env::var_os("CAPTIONS_UI_EVIDENCE")
            {
                command(
                    window,
                    TestOperation::Snapshot(
                        std::path::PathBuf::from(directory).join("overlay-toolbar.bmp"),
                    ),
                );
            }
            unsafe {
                SendMessageW(window, WM_LBUTTONUP, None, Some(point(0)));
                SendMessageW(window, WM_MOUSEMOVE, None, Some(point(11)));
                SendMessageW(window, WM_LBUTTONUP, None, Some(point(11)));
            }
            assert_eq!(command(window, TestOperation::TooltipState), 512);
            assert_ne!(
                unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } & WS_EX_TRANSPARENT.0 as isize,
                0
            );
            assert_eq!(
                styles.lock().unwrap().last().unwrap().font_size,
                16,
                "toolbar click executes its action rather than dragging"
            );
            assert!(!unsafe { IsWindowVisible(window) }.as_bool());
            drop(overlay);
            assert!(errors.lock().unwrap().is_empty());
            println!(
                "hidden toolbar pass {pass}: 12 Chinese tooltips, native one-shot delay, leave cancellation, font action and click-through/reopen state PASS"
            );
        }
    }
    #[test]
    fn visible_mode_geometry_uses_the_same_both_baseline_at_each_dpi() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let both = RECT {
                left: 100,
                top: 200,
                right: 1400,
                bottom: 600,
            };
            for _ in 0..100 {
                let translation = mode_rect(both, 1, scale);
                let original = mode_rect(both, 2, scale);
                assert_eq!(translation.top - both.top, (40.0 * scale) as i32);
                assert_eq!(original.top, both.top);
                assert_eq!(translation.bottom, both.bottom);
                assert_eq!(mode_rect(both, 0, scale), both);
            }
        }
        assert_eq!(translation_color(0xffffff), 0xb299b2);
        assert_eq!(translation_color(0), 0x4c664c);
    }
    fn hidden_geometry(manual: bool) -> SavedGeometry {
        SavedGeometry {
            bounds: Bounds {
                x: 100.0,
                y: 100.0,
                width: 650.0,
                height: 135.0,
            },
            anchor: None,
            manual_size: manual,
        }
    }
    fn rect(window: HWND) -> RECT {
        let mut result = RECT::default();
        unsafe {
            GetWindowRect(window, &mut result).unwrap();
        }
        result
    }
    fn flush_paint(window: HWND) {
        unsafe {
            PostMessageW(Some(window), UPDATE, WPARAM(0), LPARAM(0)).unwrap();
            PostMessageW(Some(window), PAINT, WPARAM(0), LPARAM(0)).unwrap();
            PostMessageW(Some(window), GEOMETRY, WPARAM(0), LPARAM(0)).unwrap();
            let (sender, receiver) = mpsc::sync_channel::<()>(1);
            let pointer = Box::into_raw(Box::new(sender));
            if let Err(error) = PostMessageW(
                Some(window),
                TEST_FENCE,
                WPARAM(0),
                LPARAM(pointer as isize),
            ) {
                drop(Box::from_raw(pointer));
                panic!("failed native test fence: {error}");
            }
            receiver
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap();
        }
    }
    fn short_content() -> Content {
        Content {
            source: "public test".into(),
            translation: "公开测试".into(),
            ..Default::default()
        }
    }
    fn command(window: HWND, operation: TestOperation) -> isize {
        let (reply, receiver) = mpsc::sync_channel(1);
        let pointer = Box::into_raw(Box::new(TestCommand { operation, reply }));
        if let Err(error) = unsafe {
            PostMessageW(
                Some(window),
                TEST_COMMAND,
                WPARAM(0),
                LPARAM(pointer as isize),
            )
        } {
            unsafe {
                drop(Box::from_raw(pointer));
            }
            panic!("failed native command: {error}");
        }
        receiver
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
    }
    #[test]
    fn hidden_overlay_defers_reentrant_dpi_close_and_keeps_new_size_during_presentation() {
        let _dpi = test_dpi();
        let positions: Arc<Mutex<Vec<Placement>>> = Arc::default();
        let closed: Arc<Mutex<usize>> = Arc::default();
        let errors: Arc<Mutex<Vec<String>>> = Arc::default();
        let (saved, ended, failed) = (positions.clone(), closed.clone(), errors.clone());
        let callback = Arc::new(move |event| match event {
            Event::PlacementChanged(value, _) => saved.lock().unwrap().push(value),
            Event::Closed => *ended.lock().unwrap() += 1,
            Event::Failed(error) | Event::PlacementFailed(error) => {
                failed.lock().unwrap().push(error)
            }
            _ => {}
        });
        let overlay = Overlay::open_with_visibility(
            Settings::default(),
            short_content(),
            hidden_geometry(true),
            callback,
            false,
        )
        .unwrap();
        let shared = overlay.shared.clone();
        let window = HWND(shared.window.lock().unwrap().unwrap() as *mut _);
        flush_paint(window);
        let scale = f64::from(unsafe { GetDpiForWindow(window) }.max(96)) / 96.0;
        let target = physical_rect(Bounds {
            x: 250.0 * scale,
            y: 220.0 * scale,
            width: 800.0 * scale,
            height: 220.0 * scale,
        });
        command(window, TestOperation::Dpi(target));
        flush_paint(window);
        let actual = rect(window);
        assert_eq!(
            (actual.left, actual.top, actual.right, actual.bottom),
            (target.left, target.top, target.right, target.bottom),
            "DPI payload must survive its original stack frame and a busy render-state borrow"
        );
        let parameter = LPARAM(
            ((actual.left + 1) as u16 as usize | (((actual.top + 1) as u16 as usize) << 16))
                as isize,
        );
        assert_eq!(
            command(window, TestOperation::Hit(parameter)),
            HTTOPLEFT as isize
        );
        let target = physical_rect(Bounds {
            x: 80.0 * scale,
            y: 90.0 * scale,
            width: 900.0 * scale,
            height: 350.0 * scale,
        });
        command(window, TestOperation::ResizeDuringPaint(target));
        flush_paint(window);
        let actual = rect(window);
        assert_eq!(
            (actual.left, actual.top, actual.right, actual.bottom),
            (target.left, target.top, target.right, target.bottom),
            "an old surface must not restore the old rectangle after a resize during presentation"
        );
        let count = command(window, TestOperation::PaintCount);
        thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(
            command(window, TestOperation::PaintCount),
            count,
            "static overlay must not schedule a self-sustaining paint loop"
        );
        command(window, TestOperation::ReentrantPaint);
        assert!(
            command(window, TestOperation::PaintCount) > count,
            "busy paint must queue a real replacement rather than leave the pending flag stuck"
        );
        let work_height = window_geometry::work_area(window.0 as usize)
            .unwrap()
            .height as isize;
        for notification in [false, true] {
            command(window, TestOperation::StaleWorkArea(notification));
            assert_eq!(
                command(window, TestOperation::WorkAreaHeight),
                work_height,
                "geometry/work-area notification={notification} must replace the previous monitor cache"
            );
        }
        flush_paint(window);
        assert!(!unsafe { IsWindowVisible(window) }.as_bool());
        command(window, TestOperation::Close);
        drop(overlay);
        assert!(shared.window.lock().unwrap().is_none());
        assert_eq!(*closed.lock().unwrap(), 1);
        let last = positions.lock().unwrap().last().cloned().unwrap();
        for (actual, expected) in [
            (last.bounds.x, 80.0),
            (last.bounds.y, 90.0),
            (last.bounds.width, 900.0),
            (last.bounds.height, 350.0),
        ] {
            assert!((actual - expected).abs() < 1.1);
        }
        assert!(errors.lock().unwrap().is_empty());
    }
    #[test]
    fn hidden_overlay_keeps_both_baseline_after_mode_move_resize_and_final_close() {
        let _dpi = test_dpi();
        let reported: Arc<Mutex<Vec<(Placement, bool)>>> = Arc::default();
        let errors: Arc<Mutex<Vec<String>>> = Arc::default();
        let positions = reported.clone();
        let failures = errors.clone();
        let callback = Arc::new(move |event| match event {
            Event::PlacementChanged(placement, manual) => {
                positions.lock().unwrap().push((placement, manual))
            }
            Event::Failed(message) | Event::PlacementFailed(message) => {
                failures.lock().unwrap().push(message)
            }
            _ => {}
        });
        let overlay = Overlay::open_with_visibility(
            Settings::default(),
            short_content(),
            hidden_geometry(true),
            callback,
            false,
        )
        .unwrap();
        let window = HWND(overlay.shared.window.lock().unwrap().unwrap() as *mut _);
        flush_paint(window);
        let base = rect(window);
        let scale = unsafe { GetDpiForWindow(window) }.max(96) as f64 / 96.0;
        let delta = (40.0 * scale).round() as i32;
        for _ in 0..10 {
            for mode in 1..=3 {
                unsafe {
                    SendMessageW(window, TEST_ACTION, Some(WPARAM(9)), None);
                }
                flush_paint(window);
                let actual = rect(window);
                let expected = mode_rect(base, mode % 3, scale as f32);
                assert_eq!(
                    (actual.left, actual.top, actual.right, actual.bottom),
                    (expected.left, expected.top, expected.right, expected.bottom)
                );
            }
        }
        unsafe {
            SendMessageW(window, TEST_ACTION, Some(WPARAM(9)), None); // translation only
            SendMessageW(window, WM_ENTERSIZEMOVE, None, None);
            let mut proposed = rect(window);
            SendMessageW(
                window,
                WM_SIZING,
                Some(WPARAM(WMSZ_BOTTOMRIGHT as usize)),
                Some(LPARAM((&mut proposed as *mut RECT) as isize)),
            );
            SetWindowPos(
                window,
                None,
                (-100.0 * scale).round() as i32,
                (200.0 * scale).round() as i32,
                (800.0 * scale).round() as i32,
                (150.0 * scale).round() as i32,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .unwrap();
            SendMessageW(window, WM_EXITSIZEMOVE, None, None);
        }
        flush_paint(window);
        let saved = reported.lock().unwrap().last().cloned().unwrap();
        assert!(saved.1);
        for (actual, expected) in [
            (saved.0.bounds.x, -100.0),
            (saved.0.bounds.y, 160.0),
            (saved.0.bounds.width, 800.0),
            (saved.0.bounds.height, 190.0),
        ] {
            assert!((actual - expected).abs() < 1.1);
        }
        unsafe {
            SendMessageW(window, TEST_ACTION, Some(WPARAM(9)), None); // source only
            SendMessageW(window, TEST_ACTION, Some(WPARAM(9)), None); // both
        }
        let both = rect(window);
        assert_eq!(both.top, (200.0 * scale).round() as i32 - delta);
        assert_eq!(
            both.bottom - both.top,
            (150.0 * scale).round() as i32 + delta
        );
        unsafe {
            // No EXITSIZEMOVE: the Close barrier must publish this last rectangle.
            SendMessageW(window, WM_ENTERSIZEMOVE, None, None);
            SetWindowPos(
                window,
                None,
                (50.0 * scale).round() as i32,
                (80.0 * scale).round() as i32,
                (900.0 * scale).round() as i32,
                (220.0 * scale).round() as i32,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .unwrap();
        }
        assert!(!unsafe { IsWindowVisible(window) }.as_bool());
        drop(overlay);
        let final_value = reported.lock().unwrap().last().cloned().unwrap().0;
        for (actual, expected) in [
            (final_value.bounds.x, 50.0),
            (final_value.bounds.y, 80.0),
            (final_value.bounds.width, 900.0),
            (final_value.bounds.height, 220.0),
        ] {
            assert!((actual - expected).abs() < 1.1);
        }
        assert!(errors.lock().unwrap().is_empty());
    }
    #[test]
    fn hidden_overlay_auto_height_shrinks_only_before_manual_sizing() {
        let _dpi = test_dpi();
        let errors: Arc<Mutex<Vec<String>>> = Arc::default();
        let failures = errors.clone();
        let callback = Arc::new(move |event| {
            if let Event::Failed(message) | Event::PlacementFailed(message) = event {
                failures.lock().unwrap().push(message);
            }
        });
        let overlay = Overlay::open_with_visibility(
            Settings::default(),
            short_content(),
            hidden_geometry(false),
            callback,
            false,
        )
        .unwrap();
        let window = HWND(overlay.shared.window.lock().unwrap().unwrap() as *mut _);
        let height = || {
            let value = rect(window);
            value.bottom - value.top
        };
        flush_paint(window);
        let small = height();
        let mut notice = short_content();
        notice.notice = "翻译失败：服务暂时不可用。".repeat(20);
        overlay.update(notice);
        flush_paint(window);
        assert!(
            height() > small,
            "a wrapped status must participate in real DirectWrite auto-height"
        );
        overlay.update(short_content());
        flush_paint(window);
        assert!((height() - small).abs() <= 1);
        let long = Content {
            source: "Public caption text for native wrapping. ".repeat(40),
            translation: "公开字幕排版测试。".repeat(45),
            ..Default::default()
        };
        overlay.update(long.clone());
        flush_paint(window);
        let grown = height();
        assert!(
            grown > small,
            "real DirectWrite wrapping must grow the HWND"
        );
        overlay.update(short_content());
        flush_paint(window);
        assert!((height() - small).abs() <= 1);
        let scale = unsafe { GetDpiForWindow(window) }.max(96) as f64 / 96.0;
        let manual = (350.0 * scale).round() as i32;
        unsafe {
            SendMessageW(window, WM_ENTERSIZEMOVE, None, None);
            let mut proposed = rect(window);
            SendMessageW(
                window,
                WM_SIZING,
                Some(WPARAM(WMSZ_BOTTOMRIGHT as usize)),
                Some(LPARAM((&mut proposed as *mut RECT) as isize)),
            );
            let position = rect(window);
            SetWindowPos(
                window,
                None,
                position.left,
                position.top,
                position.right - position.left,
                manual,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .unwrap();
            SendMessageW(window, WM_EXITSIZEMOVE, None, None);
        }
        overlay.update(short_content());
        flush_paint(window);
        assert_eq!(height(), manual);
        overlay.update(long);
        flush_paint(window);
        let necessary = height();
        assert!(necessary >= manual);
        overlay.update(short_content());
        flush_paint(window);
        assert_eq!(
            height(),
            necessary,
            "manual sizing must survive necessary growth"
        );
        assert!(!unsafe { IsWindowVisible(window) }.as_bool());
        drop(overlay);
        assert!(errors.lock().unwrap().is_empty());
    }
    #[test]
    fn hidden_native_overlay_reopens_coalesces_updates_and_releases_its_window_thread() {
        let events: Arc<Mutex<Vec<String>>> = Arc::default();
        for _ in 0..5 {
            let reported = events.clone();
            let callback = Arc::new(move |event| {
                if let Event::Failed(message) = event {
                    reported.lock().unwrap().push(message);
                }
            });
            let overlay = Overlay::open_with_visibility(
                Settings {
                    stroke: 3.0,
                    ..Settings::default()
                },
                Content {
                    source: "English 中文 日本語 مرحبا 😀".into(),
                    translation: "真实字形描边与透明背景".into(),
                    ..Default::default()
                },
                SavedGeometry {
                    bounds: Bounds {
                        x: 100.0,
                        y: 100.0,
                        width: 650.0,
                        height: 135.0,
                    },
                    anchor: None,
                    manual_size: false,
                },
                callback,
                false,
            )
            .unwrap();
            let shared = overlay.shared.clone();
            let window = HWND(shared.window.lock().unwrap().unwrap() as *mut _);
            assert!(!unsafe { IsWindowVisible(window) }.as_bool());
            for revision in 0..1000 {
                overlay.update(Content {
                    source: format!("test {revision}"),
                    translation: format!("测试 {revision}"),
                    ..Default::default()
                });
            }
            flush_paint(window);
            assert!(shared.content.lock().unwrap().is_none());
            drop(overlay); // Posts close, then joins the owning native thread.
            assert!(shared.window.lock().unwrap().is_none());
        }
        assert!(
            events.lock().unwrap().is_empty(),
            "rendering errors: {:?}",
            events.lock().unwrap()
        );
    }
}
