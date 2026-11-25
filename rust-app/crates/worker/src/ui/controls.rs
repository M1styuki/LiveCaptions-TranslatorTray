use super::motion::{animate, pointer_feedback};
use super::{
    runtime,
    shape::{ContinuousCorners, layer},
};
use gpui::{
    Animation, AnimationExt, AnyElement, AssetSource, Bounds, Context, Div, Entity, FocusHandle,
    Focusable, Hsla, KeyDownEvent, MouseButton, PathBuilder, Pixels, Render, ScrollHandle,
    SharedString, Stateful, Subscription, Window, anchored, canvas, deferred, div, point,
    prelude::*, px, rgb, rgba, svg,
};
use gpui_component::input::{Input, InputEvent, InputState};
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Duration,
};

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(match path {
            "icons/bold.svg" => {
                Some(include_bytes!("../../../../assets/icons/bold.svg").as_slice())
            }
            "icons/captions.svg" => {
                Some(include_bytes!("../../../../assets/icons/captions.svg").as_slice())
            }
            "icons/close.svg" => {
                Some(include_bytes!("../../../../assets/icons/close.svg").as_slice())
            }
            "icons/color.svg" => {
                Some(include_bytes!("../../../../assets/icons/color.svg").as_slice())
            }
            "icons/delete.svg" => {
                Some(include_bytes!("../../../../assets/icons/delete.svg").as_slice())
            }
            "icons/download.svg" => {
                Some(include_bytes!("../../../../assets/icons/download.svg").as_slice())
            }
            "icons/font-decrease.svg" => {
                Some(include_bytes!("../../../../assets/icons/font-decrease.svg").as_slice())
            }
            "icons/font-increase.svg" => {
                Some(include_bytes!("../../../../assets/icons/font-increase.svg").as_slice())
            }
            "icons/history.svg" => {
                Some(include_bytes!("../../../../assets/icons/history.svg").as_slice())
            }
            "icons/home.svg" => {
                Some(include_bytes!("../../../../assets/icons/home.svg").as_slice())
            }
            "icons/lock.svg" => {
                Some(include_bytes!("../../../../assets/icons/lock.svg").as_slice())
            }
            "icons/minimize.svg" => {
                Some(include_bytes!("../../../../assets/icons/minimize.svg").as_slice())
            }
            "icons/next.svg" => {
                Some(include_bytes!("../../../../assets/icons/next.svg").as_slice())
            }
            "icons/opacity-decrease.svg" => {
                Some(include_bytes!("../../../../assets/icons/opacity-decrease.svg").as_slice())
            }
            "icons/opacity-increase.svg" => {
                Some(include_bytes!("../../../../assets/icons/opacity-increase.svg").as_slice())
            }
            "icons/panel-bottom.svg" => {
                Some(include_bytes!("../../../../assets/icons/panel-bottom.svg").as_slice())
            }
            "icons/pause.svg" => {
                Some(include_bytes!("../../../../assets/icons/pause.svg").as_slice())
            }
            "icons/pin.svg" => Some(include_bytes!("../../../../assets/icons/pin.svg").as_slice()),
            "icons/play.svg" => {
                Some(include_bytes!("../../../../assets/icons/play.svg").as_slice())
            }
            "icons/previous.svg" => {
                Some(include_bytes!("../../../../assets/icons/previous.svg").as_slice())
            }
            "icons/refresh.svg" => {
                Some(include_bytes!("../../../../assets/icons/refresh.svg").as_slice())
            }
            "icons/search.svg" => {
                Some(include_bytes!("../../../../assets/icons/search.svg").as_slice())
            }
            "icons/settings.svg" => {
                Some(include_bytes!("../../../../assets/icons/settings.svg").as_slice())
            }
            "icons/stroke-decrease.svg" => {
                Some(include_bytes!("../../../../assets/icons/stroke-decrease.svg").as_slice())
            }
            "icons/stroke-increase.svg" => {
                Some(include_bytes!("../../../../assets/icons/stroke-increase.svg").as_slice())
            }
            "icons/swap.svg" => {
                Some(include_bytes!("../../../../assets/icons/swap.svg").as_slice())
            }
            "icons/tray-disabled.svg" => {
                Some(include_bytes!("../../../../assets/icons/tray-disabled.svg").as_slice())
            }
            "icons/tray-enabled.svg" => {
                Some(include_bytes!("../../../../assets/icons/tray-enabled.svg").as_slice())
            }
            "original-icons/bold.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/bold.svg").as_slice())
            }
            "original-icons/captions-off.svg" => Some(
                include_bytes!("../../../../assets/original-icons/captions-off.svg").as_slice(),
            ),
            "original-icons/captions.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/captions.svg").as_slice())
            }
            "original-icons/color.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/color.svg").as_slice())
            }
            "original-icons/delete.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/delete.svg").as_slice())
            }
            "original-icons/download.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/download.svg").as_slice())
            }
            "original-icons/font-decrease.svg" => Some(
                include_bytes!("../../../../assets/original-icons/font-decrease.svg").as_slice(),
            ),
            "original-icons/font-increase.svg" => Some(
                include_bytes!("../../../../assets/original-icons/font-increase.svg").as_slice(),
            ),
            "original-icons/history-dismiss.svg" => Some(
                include_bytes!("../../../../assets/original-icons/history-dismiss.svg").as_slice(),
            ),
            "original-icons/history-title.svg" => Some(
                include_bytes!("../../../../assets/original-icons/history-title.svg").as_slice(),
            ),
            "original-icons/history.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/history.svg").as_slice())
            }
            "original-icons/home.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/home.svg").as_slice())
            }
            "original-icons/info.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/info.svg").as_slice())
            }
            "original-icons/lock.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/lock.svg").as_slice())
            }
            "original-icons/next.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/next.svg").as_slice())
            }
            "original-icons/number-decrease.svg" => Some(
                include_bytes!("../../../../assets/original-icons/number-decrease.svg").as_slice(),
            ),
            "original-icons/number-increase.svg" => Some(
                include_bytes!("../../../../assets/original-icons/number-increase.svg").as_slice(),
            ),
            "original-icons/opacity-decrease.svg" => Some(
                include_bytes!("../../../../assets/original-icons/opacity-decrease.svg").as_slice(),
            ),
            "original-icons/opacity-increase.svg" => Some(
                include_bytes!("../../../../assets/original-icons/opacity-increase.svg").as_slice(),
            ),
            "original-icons/panel-bottom.svg" => Some(
                include_bytes!("../../../../assets/original-icons/panel-bottom.svg").as_slice(),
            ),
            "original-icons/pause-filled.svg" => Some(
                include_bytes!("../../../../assets/original-icons/pause-filled.svg").as_slice(),
            ),
            "original-icons/pause.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/pause.svg").as_slice())
            }
            "original-icons/pin-off.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/pin-off.svg").as_slice())
            }
            "original-icons/pin.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/pin.svg").as_slice())
            }
            "original-icons/play.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/play.svg").as_slice())
            }
            "original-icons/previous.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/previous.svg").as_slice())
            }
            "original-icons/prompt.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/prompt.svg").as_slice())
            }
            "original-icons/refresh.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/refresh.svg").as_slice())
            }
            "original-icons/search.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/search.svg").as_slice())
            }
            "original-icons/settings.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/settings.svg").as_slice())
            }
            "original-icons/stroke-decrease.svg" => Some(
                include_bytes!("../../../../assets/original-icons/stroke-decrease.svg").as_slice(),
            ),
            "original-icons/stroke-increase.svg" => Some(
                include_bytes!("../../../../assets/original-icons/stroke-increase.svg").as_slice(),
            ),
            "original-icons/swap.svg" => {
                Some(include_bytes!("../../../../assets/original-icons/swap.svg").as_slice())
            }
            _ => None,
        }
        .map(Cow::Borrowed))
    }
    fn list(&self, _: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub window_background: Hsla,
    pub titlebar: Hsla,
    pub pane_divider: Hsla,
    pub background: Hsla,
    pub foreground: Hsla,
    pub control: Hsla,
    pub border: Hsla,
    pub accent: Hsla,
    pub accent_text: Hsla,
    pub surface: Hsla,
    pub surface_border: Hsla,
}

impl Palette {
    pub fn for_window(dark: bool, mica: bool) -> Self {
        let mut palette = Self::new(dark);
        if mica {
            palette.titlebar = palette.titlebar.opacity(0.20);
            palette.window_background = palette.window_background.opacity(0.55);
        }
        palette
    }

    pub fn new(dark: bool) -> Self {
        let theme = |light, shade| rgb(if dark { shade } else { light }).into();
        let alpha = |light, shade| rgba(if dark { shade } else { light }).into();
        Self {
            dark,
            window_background: theme(0xf4f4f4, 0x262626),
            titlebar: theme(0xeeeeee, 0x202020),
            pane_divider: theme(0xc9c9c9, 0x454545),
            background: theme(0xf9f9f9, 0x2c2c2c),
            foreground: alpha(0x000000e6, 0xffffffff),
            control: alpha(0xffffff66, 0xffffff0e),
            border: alpha(0x00000073, 0xffffff1e),
            accent: theme(0x0057a8, 0x54bff3),
            accent_text: theme(0xffffff, 0x000000),
            surface: alpha(0xffffff80, 0xffffff08),
            surface_border: alpha(0x00000018, 0xffffff18),
        }
    }
}

pub fn col() -> Div {
    div().flex().flex_col()
}
pub fn row() -> Div {
    div().flex().items_center()
}
pub fn label(text: impl Into<SharedString>) -> Div {
    div().text_size(px(14.)).child(text.into())
}
pub fn note(text: impl Into<SharedString>) -> Div {
    label(text)
        .text_size(px(11.))
        .line_height(px(15.))
        .opacity(0.65)
}
pub fn heading(text: impl Into<SharedString>) -> Div {
    label(text)
        .text_size(px(24.))
        .line_height(px(32.))
        .font_weight(gpui::FontWeight::BOLD)
}
pub fn surface(p: Palette) -> Div {
    col().contour(Some(p.surface), Some(p.surface_border), 10.)
}
pub fn divider(p: Palette) -> Div {
    div().w_full().h(px(1.)).flex_shrink_0().bg(p.border)
}

pub fn icon(path: &'static str, size: f32, color: Hsla) -> impl IntoElement {
    svg()
        .path(path)
        .size(px(size))
        .text_color(color)
        .flex_shrink_0()
}

struct Tip(SharedString, Palette);
impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(px(12.))
            .text_color(self.1.foreground)
            .contour(Some(self.1.background), Some(self.1.border), 5.)
            .px(px(8.))
            .py(px(5.))
            .child(self.0.clone())
    }
}

pub fn tip(div: Stateful<Div>, text: impl Into<SharedString>, p: Palette) -> Stateful<Div> {
    let text = text.into();
    div.tooltip(move |_, cx| cx.new(|_| Tip(text.clone(), p)).into())
}

pub fn button(
    id: impl Into<gpui::ElementId>,
    p: Palette,
    active: bool,
    outlined: bool,
    enabled: bool,
    primary: bool,
    radius: f32,
    callback: impl Fn() + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .child(layer(
            if active || outlined {
                Some(if primary { p.accent } else { p.control })
            } else {
                None
            },
            if outlined && !primary {
                Some(p.border)
            } else {
                None
            },
            radius,
        ))
        .when(enabled, |d| {
            let (hover, press) = if primary {
                (p.accent_text.opacity(0.08), p.accent_text.opacity(0.16))
            } else {
                (
                    if active || outlined {
                        p.foreground.opacity(0.055)
                    } else {
                        p.control
                    },
                    p.foreground.opacity(0.09),
                )
            };
            pointer_feedback(d, hover, press, radius)
        })
        .when(enabled, |d| activate(d, p, callback))
        .when(!enabled, |d| d.opacity(0.4))
}

fn activate(control: Stateful<Div>, p: Palette, callback: impl Fn() + 'static) -> Stateful<Div> {
    let callback = Rc::new(callback);
    let click = callback.clone();
    control
        .focusable()
        .tab_stop(true)
        .cursor_pointer()
        .focus(|s| s.text_color(p.accent))
        .on_click(move |_, _, _| {
            let click = click.clone();
            runtime::dispatch(move || click());
        })
        .on_key_down(move |e: &KeyDownEvent, _, cx| {
            if e.keystroke.key == "enter" || e.keystroke.key == "space" {
                cx.stop_propagation();
                let callback = callback.clone();
                runtime::dispatch(move || callback());
            }
        })
}

pub fn icon_button(
    id: &'static str,
    path: &'static str,
    title: &str,
    p: Palette,
    active: bool,
    outlined: bool,
    enabled: bool,
    w: f32,
    h: f32,
    image_size: f32,
    callback: impl Fn() + 'static,
) -> Stateful<Div> {
    tip(
        button(id, p, active, outlined, enabled, false, 6., callback)
            .w(px(w))
            .h(px(h))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(icon(
                path,
                image_size,
                if active { p.accent } else { p.foreground },
            )),
        title.to_owned(),
        p,
    )
}

pub fn action(
    id: &'static str,
    title: &str,
    hint: &str,
    p: Palette,
    primary: bool,
    enabled: bool,
    callback: impl Fn() + 'static,
) -> Stateful<Div> {
    tip(
        button(id, p, true, true, enabled, primary, 6., callback)
            .h(px(34.))
            .px(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .text_color(if primary { p.accent_text } else { p.foreground })
            .child(title.to_owned()),
        hint.to_owned(),
        p,
    )
}

pub fn nav(
    id: impl Into<gpui::ElementId>,
    title: &'static str,
    path: Option<&'static str>,
    selected: bool,
    p: Palette,
    callback: impl Fn() + 'static,
) -> Stateful<Div> {
    button(id, p, false, false, true, false, 7., callback)
        .w_full()
        .h(px(42.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(10.))
        .pl(px(if path.is_some() { 14. } else { 16. }))
        .child(
            canvas(
                move |_, window, cx| {
                    animate(
                        "nav-selection",
                        if selected { 1. } else { 0. },
                        Duration::from_millis(160),
                        window,
                        cx,
                    )
                },
                move |bounds, amount, window, _| {
                    super::shape::paint_fill(bounds, p.control.opacity(amount), 7., window);
                    let marker = Bounds::new(
                        point(bounds.left(), bounds.center().y - px(9.)),
                        gpui::size(px(3.), px(18.)),
                    );
                    super::shape::paint_fill(marker, p.accent.opacity(amount), 2., window);
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .children(path.map(|path| icon(path, 16., p.foreground)))
        .child(title)
}

pub fn toggle(
    id: &'static str,
    title: &'static str,
    checked: bool,
    p: Palette,
    callback: impl Fn() + 'static,
) -> Stateful<Div> {
    tip(
        activate(div().id(id), p, callback)
            .h(px(32.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .child(
                canvas(
                    move |_, window, cx| {
                        animate(
                            "switch-slide",
                            if checked { 1. } else { 0. },
                            Duration::from_millis(180),
                            window,
                            cx,
                        )
                    },
                    move |bounds, phase, window, _| {
                        window.paint_quad(gpui::quad(
                            bounds,
                            px(11.),
                            p.control.opacity(1. - phase),
                            px(0.),
                            p.border,
                            gpui::BorderStyle::Solid,
                        ));
                        window.paint_quad(gpui::quad(
                            bounds,
                            px(11.),
                            p.accent.opacity(phase),
                            px(1.),
                            p.border,
                            gpui::BorderStyle::Solid,
                        ));
                        let radius = (bounds.size.height * 0.5 - px(4.)).max(px(0.));
                        let cx = bounds.left()
                            + bounds.size.height * 0.5
                            + (bounds.size.width - bounds.size.height) * phase;
                        let cy = bounds.center().y;
                        let left = point(cx - radius, cy);
                        let right = point(cx + radius, cy);
                        let radii = point(radius, radius);
                        let mut path = gpui::PathBuilder::fill();
                        path.move_to(left);
                        path.arc_to(radii, px(0.), false, true, right);
                        path.arc_to(radii, px(0.), false, true, left);
                        path.close();
                        if let Ok(disc) = path.build() {
                            window.paint_path(
                                disc,
                                if checked { p.accent_text } else { p.foreground },
                            );
                        }
                    },
                )
                .w(px(42.))
                .h(px(22.))
                .flex_shrink_0(),
            ),
        format!("{title}：{}", if checked { "已开启" } else { "已关闭" }),
        p,
    )
}

pub fn titlebar(
    p: Palette,
    title: &'static str,
    child: Option<AnyElement>,
    close_title: &'static str,
    callback: impl Fn(i32) + Clone + 'static,
) -> Div {
    let minimize = callback.clone();
    let close = callback;
    row()
        .h(px(30.))
        .w_full()
        .flex_shrink_0()
        .bg(p.titlebar)
        .child(
            row()
                .flex_1()
                .h_full()
                .pl(px(14.))
                .text_size(px(12.))
                .opacity(0.8)
                .window_control_area(gpui::WindowControlArea::Drag)
                .child(title),
        )
        .children(child)
        .child(titlebar_button(
            p,
            "minimize",
            Some(gpui::WindowControlArea::Min),
            "icons/minimize.svg",
            "最小化",
            false,
            move || minimize(0),
        ))
        .child(titlebar_button(
            p,
            "close",
            Some(gpui::WindowControlArea::Close),
            "icons/close.svg",
            close_title,
            false,
            move || close(1),
        ))
}

pub fn titlebar_button(
    p: Palette,
    id: &'static str,
    area: Option<gpui::WindowControlArea>,
    path: &'static str,
    title: &'static str,
    selected: bool,
    callback: impl Fn() + 'static,
) -> Stateful<Div> {
    tip(
        activate(
            pointer_feedback(
                row()
                    .id(id)
                    .relative()
                    .w(px(38.))
                    .h_full()
                    .flex_shrink_0()
                    .justify_center()
                    .when_some(area, |d, area| d.window_control_area(area))
                    .when(selected, |d| d.bg(p.control)),
                p.foreground.opacity(0.10),
                p.foreground.opacity(0.14),
                0.,
            ),
            p,
            callback,
        )
        .child(icon(
            path,
            if area.is_some() { 14. } else { 16. },
            if selected { p.accent } else { p.foreground },
        )),
        title,
        p,
    )
}

pub fn grip() -> Div {
    div()
        .absolute()
        .right_0()
        .bottom_0()
        .size(px(12.))
        .cursor(gpui::CursorStyle::ResizeUpLeftDownRight)
        .on_mouse_down(MouseButton::Left, |_, window, _| {
            window.start_window_resize(gpui::ResizeEdge::BottomRight);
        })
}

#[derive(Clone)]
pub enum Edit {
    Change(String),
    Submit,
    Blur,
}

struct Field {
    state: Entity<InputState>,
    observed: Rc<RefCell<String>>,
    applied: RefCell<String>,
    pending: Rc<Cell<usize>>,
    held: Rc<Cell<(i32, u64)>>,
    reset: Rc<Cell<bool>>,
    _events: Subscription,
}

impl Drop for Field {
    fn drop(&mut self) {
        self.held.set((0, 0));
    }
}

#[derive(Default)]
pub struct Fields(HashMap<&'static str, Field>);

impl Fields {
    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn input<T: 'static>(
        &mut self,
        id: &'static str,
        value: &str,
        placeholder: &'static str,
        multiline: bool,
        secret: bool,
        force_value: bool,
        window: &mut Window,
        cx: &mut Context<T>,
        callback: impl Fn(Edit) + 'static,
    ) -> (Input, FocusHandle) {
        let (entry, existed) = if let Some(entry) = self.0.get(id) {
            (entry, true)
        } else {
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(multiline)
                    .rows(if multiline { 8 } else { 1 })
                    .placeholder(placeholder)
                    .masked(secret)
                    .default_value(value.to_owned())
            });
            let observed = Rc::new(RefCell::new(value.to_owned()));
            let observed_in = observed.clone();
            let pending = Rc::new(Cell::new(0));
            let pending_event = pending.clone();
            let callback = Rc::new(callback);
            let reset = Rc::new(Cell::new(false));
            let reset_for_event = reset.clone();
            let events = cx.subscribe(&state, move |_, input, event, cx| {
                let edit = match event {
                    InputEvent::Change => {
                        let value = input.read(cx).value().to_string();
                        if *observed_in.borrow() == value {
                            return;
                        }
                        *observed_in.borrow_mut() = value.clone();
                        Edit::Change(value)
                    }
                    InputEvent::PressEnter { .. } if !multiline => Edit::Submit,
                    InputEvent::Blur => Edit::Blur,
                    _ => return,
                };
                if !force_value && matches!(edit, Edit::Blur | Edit::Submit) {
                    reset_for_event.set(true);
                }
                let callback = callback.clone();
                let pending = pending_event.clone();
                pending.set(pending.get() + 1);
                runtime::dispatch(move || {
                    callback(edit);
                    pending.set(pending.get() - 1);
                });
            });
            let entry = self.0.entry(id).or_insert(Field {
                state,
                observed,
                applied: RefCell::new(value.to_owned()),
                pending,
                held: Rc::default(),
                reset,
                _events: events,
            });
            (entry as &Field, false)
        };

        let focus = entry.state.focus_handle(cx);
        let focused = focus.is_focused(window);
        if existed && entry.pending.get() == 0 {
            let reset = entry.reset.replace(false);
            let model_changed = *entry.applied.borrow() != value;
            // A re-render with the previous model must not overwrite a draft
            // the editor just emitted. Only apply new model values or a reset.
            if reset || !focused || (force_value && model_changed) {
                *entry.applied.borrow_mut() = value.to_owned();
                if *entry.observed.borrow() != value {
                    *entry.observed.borrow_mut() = value.to_owned();
                    entry.state.update(cx, |state, cx| {
                        state.set_value(value.to_owned(), window, cx)
                    });
                }
            }
        }
        let input = Input::new(&entry.state)
            .appearance(false)
            .p_0()
            .text_size(px(13.3));
        (if multiline { input.h_full() } else { input }, focus)
    }
}

pub fn field_shell(
    id: &'static str,
    p: Palette,
    focus: &FocusHandle,
    window: &Window,
    h: f32,
) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(|| id.to_owned())
        .h(px(h))
        .w_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .contour(Some(p.control), None, 4.)
        .child({
            let focused = focus.is_focused(window);
            canvas(
                move |_, window, cx| {
                    animate(
                        "input-focus",
                        if focused { 1. } else { 0. },
                        Duration::from_millis(130),
                        window,
                        cx,
                    )
                },
                move |bounds, amount, window, _| {
                    super::shape::paint_outline(bounds, p.border.opacity(1. - amount), 4., window);
                    super::shape::paint_outline(bounds, p.accent.opacity(amount), 4., window);
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        })
}

#[derive(Default, Clone)]
pub struct Dropdowns {
    opened: Rc<Cell<Option<(&'static str, usize)>>>,
    bounds: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
    menu_scroll: ScrollHandle,
}

impl Dropdowns {
    pub fn close(&self) {
        self.opened.set(None);
        self.menu_scroll.set_offset(point(px(0.), px(0.)));
    }
    pub fn show(
        &self,
        id: &'static str,
        p: Palette,
        value: &str,
        values: Vec<String>,
        edit: Option<Input>,
        hint: &str,
        height: f32,
        window: &Window,
        select: impl Fn(usize, String) + 'static,
    ) -> Stateful<Div> {
        let selected = values.iter().position(|item| item == value);
        let open = self.opened.get().filter(|(key, _)| *key == id);
        let bounds = self.bounds.clone();
        let select = Rc::new(select);
        let toggle = self.opened.clone();
        let toggle_scroll = self.menu_scroll.clone();
        let current = selected.unwrap_or(0);
        let can_open = !values.is_empty();
        let editable = edit.is_some();
        let uses_arrow = editable && can_open;
        let mut control = pointer_feedback(
            div()
                .id(id)
                .debug_selector(|| id.to_owned())
                .h(px(height))
                .relative()
                .w_full()
                .flex_shrink_0()
                .flex()
                .items_center()
                .contour(
                    Some(p.control),
                    Some(if open.is_some() { p.accent } else { p.border }),
                    6.,
                ),
            p.foreground.opacity(0.05),
            p.foreground.opacity(0.09),
            6.,
        )
        .child(
            canvas(
                move |bounds_now, _, _| {
                    bounds.borrow_mut().insert(id, bounds_now);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .when(!editable, |d| d.focusable().tab_stop(true).cursor_pointer())
        .when(!editable && can_open, |d| {
            d.on_click(move |_, window, _| {
                toggle_scroll.set_offset(point(px(0.), px(0.)));
                toggle.set(if toggle.get().is_some_and(|(key, _)| key == id) {
                    None
                } else {
                    Some((id, current))
                });
                window.refresh();
            })
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pl(px(10.))
                .overflow_hidden()
                .flex()
                .items_center()
                .child(match edit {
                    Some(edit) => edit.into_any_element(),
                    None => div().truncate().child(value.to_owned()).into_any_element(),
                }),
        );
        let arrow = chevron(p.foreground, open.is_some());
        let arrow_state = self.opened.clone();
        let arrow_scroll = self.menu_scroll.clone();
        control = control.child(
            row()
                .id(SharedString::from(format!("{id}-arrow")))
                .w(px(30.))
                .h_full()
                .justify_center()
                .flex_shrink_0()
                .when(!can_open, |d| d.opacity(0.4))
                .when(uses_arrow, |d| {
                    d.focusable()
                        .tab_stop(true)
                        .cursor_pointer()
                        .on_click(move |_, window, _| {
                            arrow_scroll.set_offset(point(px(0.), px(0.)));
                            arrow_state.set(
                                if arrow_state.get().is_some_and(|(key, _)| key == id) {
                                    None
                                } else {
                                    Some((id, current))
                                },
                            );
                            window.refresh();
                        })
                })
                .child(arrow),
        );
        let state = self.opened.clone();
        let keyboard_values = values.clone();
        let keyboard_select = select.clone();
        let keyboard_scroll = self.menu_scroll.clone();
        control = control.on_key_down(move |event: &KeyDownEvent, window, cx| {
            let current_open = state.get().filter(|(key, _)| *key == id);
            match event.keystroke.key.as_str() {
                "escape" if current_open.is_some() => state.set(None),
                "enter" | "space" if current_open.is_some() => {
                    let index = current_open.unwrap().1;
                    if let Some(value) = keyboard_values.get(index).cloned() {
                        let select = keyboard_select.clone();
                        runtime::dispatch(move || select(index, value));
                    }
                    state.set(None);
                }
                "enter" | "space" if !editable && can_open => state.set(Some((id, current))),
                "down" | "up" if can_open => {
                    let index = current_open.map_or(current, |(_, index)| index);
                    let index = if event.keystroke.key == "down" {
                        (index + 1).min(keyboard_values.len() - 1)
                    } else {
                        index.saturating_sub(1)
                    };
                    state.set(Some((id, index)));
                    keyboard_scroll.scroll_to_item(index);
                }
                _ => return,
            }
            cx.stop_propagation();
            window.refresh();
        });

        if let Some((_, highlight)) = open
            && let Some(bounds) = self.bounds.borrow().get(id).copied()
        {
            let state = self.opened.clone();
            let items = values.into_iter().enumerate().map(|(index, value)| {
                let select = select.clone();
                let state = state.clone();
                let clicked_value = value.clone();
                pointer_feedback(
                    row()
                        .id(("option", index))
                        .debug_selector(|| format!("{id}-option-{index}"))
                        .h(px(29.))
                        .px(px(8.))
                        .w_full()
                        .cursor_pointer()
                        .gap(px(6.))
                        .flex_shrink_0()
                        .when(index == highlight || Some(index) == selected, |d| {
                            d.contour(Some(p.control), None, 4.)
                        }),
                    p.foreground.opacity(0.07),
                    p.foreground.opacity(0.12),
                    4.,
                )
                .child(
                    div()
                        .w(px(13.))
                        .text_color(p.accent)
                        .child(if Some(index) == selected { "✓" } else { "" }),
                )
                .child(div().truncate().child(value))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    state.set(None);
                    let select = select.clone();
                    let value = clicked_value.clone();
                    runtime::dispatch(move || select(index, value));
                    window.refresh();
                })
            });
            let state = self.opened.clone();
            let available = window.viewport_size().height;
            let max_height = px(340.).min(available - px(16.));
            let desired = (px(10.) + px(29.) * items.len() as f32).min(max_height);
            let y = if bounds.bottom() + px(4.) + desired > available && bounds.origin.y > desired {
                bounds.origin.y - px(4.) - desired
            } else {
                bounds.bottom() + px(4.)
            };
            control = control.child(
                deferred(
                    anchored()
                        .position(point(bounds.origin.x, y))
                        .snap_to_window_with_margin(px(4.))
                        .child(
                            col()
                                .id(SharedString::from(format!("{id}-popup")))
                                .debug_selector(|| format!("{id}-popup"))
                                .relative()
                                .w(bounds.size.width)
                                .max_h(max_height)
                                .p(px(5.))
                                .contour(Some(p.background), Some(p.border), 6.)
                                .occlude()
                                .on_mouse_down_out(move |event, window, _| {
                                    if !bounds.contains(&event.position) {
                                        state.set(None);
                                        window.refresh();
                                    }
                                })
                                .child(
                                    col()
                                        .id(SharedString::from(format!("{id}-options")))
                                        .overflow_y_scroll()
                                        .track_scroll(&self.menu_scroll)
                                        .min_h_0()
                                        .children(items),
                                )
                                .child(gpui_component::scroll::Scrollbar::vertical(
                                    &self.menu_scroll,
                                ))
                                .with_animation(
                                    SharedString::from(format!("{id}-open")),
                                    Animation::new(Duration::from_millis(140))
                                        .with_easing(gpui::ease_out_quint()),
                                    |d, t| d.opacity(t),
                                ),
                        ),
                )
                .with_priority(2),
            );
        }
        if open.is_some() {
            control
        } else {
            tip(control, hint.to_owned(), p)
        }
    }
}

pub fn entrance(id: impl Into<gpui::ElementId>, content: impl IntoElement) -> impl IntoElement {
    div().relative().size_full().overflow_hidden().child(
        col()
            .absolute()
            .left_0()
            .size_full()
            .child(content)
            .with_animation(
                id,
                Animation::new(Duration::from_millis(220)).with_easing(gpui::ease_out_quint()),
                |div, t| div.opacity(t).top(px(8. * (1. - t))),
            ),
    )
}

pub fn sync_theme(window: Option<&mut Window>, cx: &mut gpui::App) {
    gpui_component::Theme::sync_system_appearance(window, cx);
    let theme = gpui_component::Theme::global_mut(cx);
    let palette = Palette::new(theme.is_dark());
    // Each window paints its own fallback or content layer over the DWM backdrop.
    theme.background = gpui::transparent_black();
    theme.foreground = palette.foreground;
    theme.primary = palette.accent;
}

fn chevron(color: Hsla, up: bool) -> impl IntoElement {
    canvas(
        move |_, window, cx| {
            animate(
                "rotate-chevron",
                if up { 1. } else { 0. },
                Duration::from_millis(180),
                window,
                cx,
            )
        },
        move |bounds, phase, window, _| {
            let center = bounds.center();
            let (sin, cos) = (phase * std::f32::consts::PI).sin_cos();
            let rotate = |x: f32, y: f32| {
                point(
                    center.x + px(x * cos - y * sin),
                    center.y + px(x * sin + y * cos),
                )
            };
            let mut path = PathBuilder::stroke(px(1.2));
            path.move_to(rotate(-4., -1.7));
            path.line_to(rotate(0., 1.7));
            path.line_to(rotate(4., -1.7));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size(px(14.))
}

pub fn scroller(id: &'static str, scroll: &ScrollHandle, content: Div) -> impl IntoElement {
    col()
        .relative()
        .flex_1()
        .min_h_0()
        .child(
            col()
                .id(id)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .child(content.flex_shrink_0()),
        )
        .child(gpui_component::scroll::Scrollbar::vertical(scroll))
}

pub fn number<T: 'static>(
    fields: &mut Fields,
    id: &'static str,
    title: &'static str,
    hint: &'static str,
    value: &str,
    p: Palette,
    int: Option<(i32, i32)>,
    window: &mut Window,
    cx: &mut Context<T>,
    callback: impl Fn(String) + 'static,
    step: impl Fn(i32) + 'static,
) -> Stateful<Div> {
    let typed = callback;
    let (input, focus) = fields.input(
        id,
        value,
        "",
        false,
        false,
        false,
        window,
        cx,
        move |edit| match edit {
            Edit::Change(text) => {
                if let Some((min, max)) = int {
                    if let Ok(value) = text.parse::<f64>()
                        && value.fract() == 0.
                        && value >= min as f64
                        && value <= max as f64
                    {
                        typed((value as i32).to_string());
                    }
                } else {
                    typed(text);
                }
            }
            Edit::Blur | Edit::Submit if int.is_some() => {
                if let Some(app) = runtime::app() {
                    let _ = app.refresh();
                }
            }
            _ => {}
        },
    );
    let held = fields.0[&id].held.clone();
    let reset = fields.0[&id].reset.clone();
    let step = Rc::new(move |delta| {
        reset.set(true);
        step(delta);
    });
    let wheel = step.clone();
    let wheel_focus = focus.clone();
    let key = step.clone();
    tip(
        field_shell(title, p, &focus, window, 30.)
            .w(px(130.))
            .pr(px(3.))
            .on_key_down(move |event, _, cx| {
                if event.keystroke.key == "up" || event.keystroke.key == "down" {
                    let key = key.clone();
                    let delta = if event.keystroke.key == "up" { 1 } else { -1 };
                    cx.stop_propagation();
                    runtime::dispatch(move || key(delta));
                }
            })
            .on_scroll_wheel(move |event, window, cx| {
                let y = event.delta.pixel_delta(px(20.)).y;
                if y != px(0.) && wheel_focus.is_focused(window) {
                    let wheel = wheel.clone();
                    cx.stop_propagation();
                    runtime::dispatch(move || wheel(if y > px(0.) { 1 } else { -1 }));
                }
            })
            .child(row().flex_1().min_w_0().pl(px(10.)).child(input))
            .children([1, -1].map(|delta| {
                let step = step.clone();
                let focus = focus.clone();
                let tooltip = format!("{}{title}", if delta > 0 { "增加" } else { "减少" });
                let held = held.clone();
                let pressed = held.clone();
                let released = held.clone();
                let leave = held;
                tip(
                    pointer_feedback(
                        row()
                            .id(("step", delta as i32 as usize))
                            .debug_selector(|| format!("{id}-{delta}"))
                            .relative()
                            .h(px(24.))
                            .w(px(26.))
                            .flex_shrink_0()
                            .justify_center(),
                        p.foreground.opacity(0.07),
                        p.foreground.opacity(0.12),
                        4.,
                    )
                    .child(chevron(p.foreground, delta > 0))
                    .on_mouse_down(MouseButton::Left, move |_, window, _| {
                        focus.focus(window);
                        let step = step.clone();
                        let held = pressed.clone();
                        let press = (delta, held.get().1.wrapping_add(1));
                        held.set(press);
                        runtime::dispatch(move || {
                            step(delta);
                            if let Some(app) = runtime::app() {
                                let clock = app.background_executor().clone();
                                app.spawn(async move |_| {
                                    clock.timer(Duration::from_millis(500)).await;
                                    while held.get() == press {
                                        step(delta);
                                        clock.timer(Duration::from_millis(50)).await;
                                    }
                                })
                                .detach();
                            }
                        });
                    })
                    .on_mouse_up(MouseButton::Left, move |_, _, _| {
                        released.set((0, released.get().1))
                    })
                    .on_mouse_up_out(MouseButton::Left, move |_, _, _| {
                        leave.set((0, leave.get().1))
                    }),
                    tooltip,
                    p,
                )
            })),
        hint,
        p,
    )
}
