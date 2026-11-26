use super::motion::{SmoothScroll, pointer_feedback, smooth_spring, smooth_value};
use super::runtime;
use super::shape::{ContinuousCorners as _, paint_fill, paint_outline};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    scroll::Scrollbar,
    v_flex,
};
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, AssetSource, BorderStyle,
    Bounds, Context, CursorStyle, Div, ElementId, Entity, FocusHandle, Focusable as _, FontWeight,
    Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement as _,
    PathBuilder, Pixels, Role, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, WindowControlArea, anchored, canvas, deferred, div,
    ease_out_quint, fill, point, prelude::FluentBuilder as _, px, quad, rems, size,
};
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Duration,
};

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        let local = match path {
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
        };
        if let Some(bytes) = local {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}

pub fn app_icon(path: &'static str) -> Icon {
    Icon::empty().path(path)
}

pub fn hairline(cx: &App) -> Div {
    div()
        .w_full()
        .h(px(1.))
        .flex_shrink_0()
        .bg(cx.theme().border.opacity(0.65))
}

pub fn section_card(cx: &App) -> Div {
    let radius = f32::from(cx.theme().radius_lg);
    v_flex()
        .relative()
        .w_full()
        .rounded(cx.theme().radius_lg)
        .contour(Some(cx.theme().group_box), Some(cx.theme().border), radius)
}

pub fn card_rows(rows: impl IntoIterator<Item = Div>, cx: &App) -> Div {
    let rows: Vec<Div> = rows.into_iter().collect();
    let count = rows.len();
    let radius = cx.theme().radius_lg;
    let surface = cx.theme().group_box;
    let border = cx.theme().border;
    v_flex()
        .relative()
        .w_full()
        .child(
            v_flex()
                .w_full()
                .gap(px(2.))
                .children(rows.into_iter().enumerate().map(|(i, row)| {
                    let tile = row.w_full().bg(surface);
                    match (i == 0, i + 1 == count) {
                        (true, true) => tile.rounded(radius),
                        (true, false) => tile.rounded_t(radius),
                        (false, true) => tile.rounded_b(radius),
                        (false, false) => tile,
                    }
                })),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .rounded(radius)
                .border_1()
                .border_color(border),
        )
}

pub fn animated_chevron(id: impl Into<ElementId>, color: Hsla, up: bool) -> impl IntoElement {
    let id: ElementId = id.into();
    canvas(
        move |_, window, cx| {
            let duration = cx.theme().motion_tokens().duration_normal;
            smooth_value(
                (id.clone(), "chevron"),
                if up { 1. } else { 0. },
                duration,
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
            let mut path = PathBuilder::stroke(px(1.35));
            path.move_to(rotate(-3.75, -1.6));
            path.line_to(rotate(0., 1.8));
            path.line_to(rotate(3.75, -1.6));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size(px(14.))
}

#[allow(clippy::too_many_arguments)]
pub fn nav_item(
    id: impl Into<ElementId>,
    label: &'static str,
    icon: Option<&'static str>,
    compact: bool,
    selected: bool,
    draw_selection_bg: bool,
    cx: &App,
    on_click: impl Fn() + 'static,
) -> impl IntoElement {
    let id: ElementId = id.into();
    let radius = f32::from(cx.theme().radius);
    let selection_bg = cx.theme().sidebar_accent;
    let hover_fill = cx.theme().sidebar_accent.opacity(0.65);
    let press_fill = cx.theme().sidebar_accent;
    let text_color = if selected {
        cx.theme().sidebar_accent_foreground
    } else {
        cx.theme().sidebar_foreground
    };
    let icon_color = if selected {
        cx.theme().primary
    } else {
        cx.theme().muted_foreground
    };

    let callback = Rc::new(on_click);
    let click_cb = callback.clone();
    let key_cb = callback;

    let sel_id = id.clone();
    let base = h_flex()
        .id(id.clone())
        .relative()
        .w_full()
        .h_9()
        .flex_shrink_0()
        .items_center()
        .when(compact, |d| d.justify_center())
        .when(!compact, |d| d.justify_start().px_2p5().gap_2())
        .focusable()
        .tab_stop(true)
        .role(Role::Button)
        .aria_label(label)
        .aria_selected(selected)
        .when(draw_selection_bg, |d| {
            d.child(
                canvas(
                    move |_, window, cx| {
                        smooth_value(
                            (sel_id.clone(), "nav-select"),
                            if selected { 1. } else { 0. },
                            Duration::from_millis(160),
                            window,
                            cx,
                        )
                    },
                    move |bounds, amount, window, _| {
                        paint_fill(bounds, selection_bg.opacity(amount), radius, window);
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
        });

    pointer_feedback((id, "nav-fb"), base, hover_fill, press_fill, radius)
        .when_some(icon, |d, path| {
            d.child(app_icon(path).small().text_color(icon_color))
        })
        .when(!compact, |d| {
            d.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_weight(if selected {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(text_color)
                    .child(label),
            )
        })
        .when(compact, |d| {
            d.tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(label).build(window, cx)
            })
        })
        .on_click(move |_, _, _| {
            let click_cb = click_cb.clone();
            runtime::dispatch(move || click_cb());
        })
        .on_key_down(move |e: &KeyDownEvent, _, cx| {
            if e.keystroke.key == "enter" || e.keystroke.key == "space" {
                cx.stop_propagation();
                let key_cb = key_cb.clone();
                runtime::dispatch(move || key_cb());
            }
        })
}

pub fn contour_button(
    id: &'static str,
    label: &'static str,
    tooltip: &'static str,
    cx: &App,
    on_click: impl Fn() + 'static,
) -> impl IntoElement {
    let radius = f32::from(cx.theme().radius);
    let callback = Rc::new(on_click);
    let click_cb = callback.clone();
    let key_cb = callback;

    let base = h_flex()
        .id(id)
        .debug_selector(|| id.to_owned())
        .relative()
        .h_8()
        .min_w(rems(4.5))
        .px_3p5()
        .flex_shrink_0()
        .justify_center()
        .items_center()
        .rounded(cx.theme().radius)
        .contour(Some(cx.theme().button), Some(cx.theme().input), radius)
        .focusable()
        .tab_stop(true)
        .role(Role::Button)
        .aria_label(label);

    pointer_feedback(
        (ElementId::from(id), "btn-fb"),
        base,
        cx.theme().button_hover,
        cx.theme().button_active,
        radius,
    )
    .tooltip(move |window, cx| {
        gpui_kit::component::tooltip::Tooltip::new(tooltip).build(window, cx)
    })
    .child(
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .text_color(cx.theme().button_foreground)
            .child(label),
    )
    .on_click(move |_, _, _| {
        let click_cb = click_cb.clone();
        runtime::dispatch(move || click_cb());
    })
    .on_key_down(move |e: &KeyDownEvent, _, cx| {
        if e.keystroke.key == "enter" || e.keystroke.key == "space" {
            cx.stop_propagation();
            let key_cb = key_cb.clone();
            runtime::dispatch(move || key_cb());
        }
    })
}

pub fn titlebar(
    title: &'static str,
    trailing: Option<AnyElement>,
    close_title: &'static str,
    callback: impl Fn(i32) + Clone + 'static,
    cx: &App,
) -> Div {
    let minimize = callback.clone();
    let close = callback;
    let has_trailing = trailing.is_some();

    h_flex()
        .h_9()
        .w_full()
        .flex_shrink_0()
        .bg(cx.theme().title_bar)
        .child(
            h_flex()
                .flex_1()
                .h_full()
                .px_3p5()
                .gap_2()
                .window_control_area(WindowControlArea::Drag)
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(cx.theme().muted_foreground)
                        .child(title),
                ),
        )
        .when_some(trailing, |d, item| d.child(h_flex().px_1().child(item)))
        .when(has_trailing, |d| {
            d.child(div().w(px(1.)).h_4().bg(cx.theme().border).mx_1())
        })
        .child(
            div()
                .h_full()
                .window_control_area(WindowControlArea::Min)
                .child(
                    Button::new("minimize")
                        .ghost()
                        .compact()
                        .h_full()
                        .w_11()
                        .rounded(ButtonRounded::None)
                        .icon(app_icon("icons/minimize.svg").small())
                        .accessibility_label("最小化")
                        .tooltip("最小化")
                        .on_click(move |_, _, _| {
                            let minimize = minimize.clone();
                            runtime::dispatch(move || minimize(0));
                        }),
                ),
        )
        .child(
            div()
                .h_full()
                .window_control_area(WindowControlArea::Close)
                .child(
                    Button::new("close")
                        .custom(
                            ButtonCustomVariant::new(cx)
                                .color(cx.theme().transparent)
                                .foreground(cx.theme().foreground)
                                .hover(cx.theme().danger.opacity(0.22))
                                .active(cx.theme().danger.opacity(0.35)),
                        )
                        .compact()
                        .h_full()
                        .w_11()
                        .rounded(ButtonRounded::None)
                        .icon(app_icon("icons/close.svg").small())
                        .accessibility_label(close_title)
                        .tooltip(close_title)
                        .on_click(move |_, _, _| {
                            let close = close.clone();
                            runtime::dispatch(move || close(1));
                        }),
                ),
        )
}

pub fn grip() -> Div {
    div()
        .absolute()
        .right_0()
        .bottom_0()
        .size_3()
        .cursor(CursorStyle::ResizeUpLeftDownRight)
        .on_mouse_down(MouseButton::Left, |_, window, _| {
            window.start_window_resize(gpui_kit::ResizeEdge::BottomRight);
        })
}

pub fn scroller(
    id: &'static str,
    scroll: &ScrollHandle,
    smooth: &SmoothScroll,
    content: Div,
) -> impl IntoElement {
    v_flex()
        .relative()
        .flex_1()
        .h_full()
        .min_h_0()
        .child(
            v_flex()
                .id(id)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .child(smooth.wrap(scroll.clone(), content.flex_shrink_0())),
        )
        .child(Scrollbar::vertical(scroll))
}

#[derive(Default, Clone, Copy)]
struct SwitchPointerState {
    hovered: bool,
    pressed: bool,
}

/// Authentic Windows 11 40×20px pill ToggleSwitch with 1px track border,
/// hover thumb growth (12->14px), press stretch (17x14px), and no focus ring.
pub fn win11_switch(
    id: impl Into<ElementId>,
    checked: bool,
    tooltip: &'static str,
    aria_label: &'static str,
    cx: &App,
    on_change: impl Fn(bool) + 'static,
) -> impl IntoElement {
    let id: ElementId = id.into();
    let state_id = ElementId::NamedChild(id.clone().into(), "__win11-switch-state".into());
    let hover_id = id.clone();
    let press_id = id.clone();
    let slide_id = id.clone();
    let on_id = id.clone();
    let slot = Rc::new(RefCell::new(None::<Entity<SwitchPointerState>>));
    let hovered = slot.clone();
    let pressed = slot.clone();
    let released = slot.clone();
    let outside = slot.clone();

    let transparent = cx.theme().transparent;
    let off_border = cx.theme().muted_foreground.opacity(0.75);
    let off_border_hover = cx.theme().foreground.opacity(0.65);
    let off_fill_hover = cx.theme().accent;
    let off_fill_press = cx.theme().secondary_active;
    let on_fill = cx.theme().primary;
    let on_fill_hover = cx.theme().primary_hover;
    let on_fill_press = cx.theme().primary_active;
    let off_thumb = cx.theme().muted_foreground;
    let off_thumb_hover = cx.theme().foreground.opacity(0.85);
    let on_thumb = cx.theme().switch_thumb;

    let set_press = |slot: &RefCell<Option<Entity<SwitchPointerState>>>,
                     down: bool,
                     window: &mut Window,
                     cx: &mut App| {
        if let Some(state) = slot.borrow().as_ref() {
            state.update(cx, |s, _| s.pressed = down);
            window.refresh();
        }
    };

    h_flex()
        .id(id)
        .relative()
        .w(px(40.))
        .h(px(20.))
        .flex_shrink_0()
        .items_center()
        .role(Role::Switch)
        .aria_selected(checked)
        .aria_label(aria_label)
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tooltip).build(window, cx)
        })
        .child(
            canvas(
                move |_, window, cx| {
                    let state = window.use_keyed_state(state_id.clone(), cx, |_, _| {
                        SwitchPointerState::default()
                    });
                    *slot.borrow_mut() = Some(state.clone());
                    let s = *state.read(cx);
                    let hover = smooth_value(
                        (hover_id.clone(), "sw-hover"),
                        if s.hovered { 1. } else { 0. },
                        Duration::from_millis(120),
                        window,
                        cx,
                    );
                    let press = smooth_value(
                        (press_id.clone(), "sw-press"),
                        if s.pressed { 1. } else { 0. },
                        Duration::from_millis(90),
                        window,
                        cx,
                    );
                    let slide = f32::from(smooth_spring(
                        (slide_id.clone(), "sw-slide"),
                        px(if checked { 100. } else { 0. }),
                        window,
                        cx,
                    )) / 100.;
                    let on_mix = smooth_value(
                        (on_id.clone(), "sw-on"),
                        if checked { 1. } else { 0. },
                        Duration::from_millis(150),
                        window,
                        cx,
                    );
                    (hover, press, slide.clamp(0., 1.), on_mix)
                },
                move |bounds, (hover, press, slide, on_mix), window, _| {
                    let radius = f32::from(bounds.size.height) / 2.;
                    let off_bg = transparent
                        .blend(off_fill_hover.opacity(hover))
                        .blend(off_fill_press.opacity(press));
                    let active_bg = on_fill
                        .blend(on_fill_hover.opacity(hover))
                        .blend(on_fill_press.opacity(press));
                    let track_bg = off_bg.blend(active_bg.opacity(on_mix));
                    let off_stroke = off_border.blend(off_border_hover.opacity(hover));
                    let track_border = off_stroke.blend(active_bg.opacity(on_mix));
                    window.paint_quad(quad(
                        bounds,
                        px(radius),
                        track_bg,
                        px(1.),
                        track_border,
                        BorderStyle::Solid,
                    ));

                    let thumb_h = 12. + 2. * hover.max(press);
                    let thumb_w = thumb_h + 3. * press;
                    let pad = (f32::from(bounds.size.height) - thumb_h) / 2.;
                    let travel = (f32::from(bounds.size.width) - 2. * pad - thumb_w).max(0.);
                    let thumb_x = f32::from(bounds.origin.x) + pad + travel * slide;
                    let thumb_y = f32::from(bounds.origin.y) + pad;
                    let thumb_color = off_thumb
                        .blend(off_thumb_hover.opacity(hover))
                        .blend(on_thumb.opacity(on_mix));
                    window.paint_quad(
                        fill(
                            Bounds::new(
                                point(px(thumb_x), px(thumb_y)),
                                size(px(thumb_w), px(thumb_h)),
                            ),
                            thumb_color,
                        )
                        .corner_radii(px(thumb_h / 2.)),
                    );
                },
            )
            .size_full(),
        )
        .on_hover(move |hover, window, cx| {
            if let Some(state) = hovered.borrow().as_ref() {
                state.update(cx, |s, _| s.hovered = *hover);
                window.refresh();
            }
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            set_press(&pressed, true, window, cx);
        })
        .on_mouse_up(MouseButton::Left, move |_, window, cx| {
            set_press(&released, false, window, cx);
        })
        .on_mouse_up_out(MouseButton::Left, move |_, window, cx| {
            set_press(&outside, false, window, cx);
        })
        .on_click(move |_, _, _| on_change(!checked))
}

#[derive(Clone)]
pub enum Edit {
    Change(String),
    Submit,
    Blur,
}

enum FieldKind {
    Single(Entity<InputState>),
    Multi(Entity<TextareaState>),
}

impl FieldKind {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            FieldKind::Single(state) => state.focus_handle(cx),
            FieldKind::Multi(state) => state.focus_handle(cx),
        }
    }

    fn set_value(&self, value: String, window: &mut Window, cx: &mut App) {
        match self {
            FieldKind::Single(state) => state.update(cx, |s, cx| s.set_value(value, window, cx)),
            FieldKind::Multi(state) => state.update(cx, |s, cx| s.set_value(value, window, cx)),
        }
    }
}

struct Field {
    state: FieldKind,
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

    fn sync_existing(entry: &Field, value: &str, window: &mut Window, cx: &mut App) -> FocusHandle {
        let focus = entry.state.focus_handle(cx);
        let focused = focus.is_focused(window);
        if entry.pending.get() == 0 {
            let reset = entry.reset.replace(false);
            let model_changed = *entry.applied.borrow() != value;
            // A re-render with the previous model must not overwrite a draft
            // the editor just emitted. Only apply new model values or a reset.
            if reset || !focused || model_changed {
                *entry.applied.borrow_mut() = value.to_owned();
                if *entry.observed.borrow() != value {
                    *entry.observed.borrow_mut() = value.to_owned();
                    entry.state.set_value(value.to_owned(), window, cx);
                }
            }
        }
        focus
    }

    pub fn input<T: 'static>(
        &mut self,
        id: &'static str,
        value: &str,
        placeholder: &'static str,
        secret: bool,
        force_value: bool,
        window: &mut Window,
        cx: &mut Context<T>,
        callback: impl Fn(Edit) + 'static,
    ) -> (Input, FocusHandle) {
        let existed = self.0.contains_key(id);
        let state = if let Some(entry) = self.0.get(id) {
            let focus = Self::sync_existing_with_force(entry, value, force_value, window, cx);
            let FieldKind::Single(state) = &entry.state else {
                unreachable!("field {id} kind mismatch");
            };
            let _ = existed;
            let input = Input::new(state).small().when(secret, |d| d.mask_toggle());
            return (input, focus);
        } else {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .masked(secret)
                    .default_value(value.to_owned())
            })
        };

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
                InputEvent::PressEnter { .. } => Edit::Submit,
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

        let focus = state.focus_handle(cx);
        let input = Input::new(&state).small().when(secret, |d| d.mask_toggle());
        self.0.insert(
            id,
            Field {
                state: FieldKind::Single(state),
                observed,
                applied: RefCell::new(value.to_owned()),
                pending,
                held: Rc::default(),
                reset,
                _events: events,
            },
        );
        (input, focus)
    }

    fn sync_existing_with_force(
        entry: &Field,
        value: &str,
        force_value: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> FocusHandle {
        let focus = entry.state.focus_handle(cx);
        let focused = focus.is_focused(window);
        if entry.pending.get() == 0 {
            let reset = entry.reset.replace(false);
            let model_changed = *entry.applied.borrow() != value;
            if reset || !focused || (force_value && model_changed) {
                *entry.applied.borrow_mut() = value.to_owned();
                if *entry.observed.borrow() != value {
                    *entry.observed.borrow_mut() = value.to_owned();
                    entry.state.set_value(value.to_owned(), window, cx);
                }
            }
        }
        focus
    }

    pub fn textarea<T: 'static>(
        &mut self,
        id: &'static str,
        value: &str,
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<T>,
        callback: impl Fn(Edit) + 'static,
    ) -> (Textarea, FocusHandle) {
        if let Some(entry) = self.0.get(id) {
            let focus = Self::sync_existing(entry, value, window, cx);
            let FieldKind::Multi(state) = &entry.state else {
                unreachable!("field {id} kind mismatch");
            };
            return (Textarea::new(state).small(), focus);
        }

        let state = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(8, 18)
                .placeholder(placeholder)
                .default_value(value.to_owned())
        });
        let observed = Rc::new(RefCell::new(value.to_owned()));
        let observed_in = observed.clone();
        let pending = Rc::new(Cell::new(0));
        let pending_event = pending.clone();
        let callback = Rc::new(callback);
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
                InputEvent::Blur => Edit::Blur,
                _ => return,
            };
            let callback = callback.clone();
            let pending = pending_event.clone();
            pending.set(pending.get() + 1);
            runtime::dispatch(move || {
                callback(edit);
                pending.set(pending.get() - 1);
            });
        });

        let focus = state.focus_handle(cx);
        let textarea = Textarea::new(&state).small();
        self.0.insert(
            id,
            Field {
                state: FieldKind::Multi(state),
                observed,
                applied: RefCell::new(value.to_owned()),
                pending,
                held: Rc::default(),
                reset: Rc::new(Cell::new(false)),
                _events: events,
            },
        );
        (textarea, focus)
    }
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
        value: &str,
        values: Vec<String>,
        edit: Option<Input>,
        hint: &str,
        window: &Window,
        cx: &App,
        select: impl Fn(usize, String) + 'static,
    ) -> AnyElement {
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

        let is_open = open.is_some();
        let radius = f32::from(cx.theme().radius);
        let popup_radius = f32::from(cx.theme().radius_lg);
        let rest_border = cx.theme().input;
        let open_border = cx.theme().ring;

        let mut control = h_flex()
            .id(id)
            .debug_selector(|| id.to_owned())
            .relative()
            .w_full()
            .h_8()
            .flex_shrink_0()
            .rounded(cx.theme().radius)
            .contour(Some(cx.theme().input_background()), None, radius)
            .child(
                canvas(
                    move |_, window, cx| {
                        smooth_value(
                            (id, "dropdown-open"),
                            if is_open { 1. } else { 0. },
                            Duration::from_millis(150),
                            window,
                            cx,
                        )
                    },
                    move |bounds, amount, window, _| {
                        paint_outline(bounds, rest_border.opacity(1. - amount), radius, window);
                        paint_outline(bounds, open_border.opacity(amount), radius, window);
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        if !is_open {
            control = pointer_feedback(
                (ElementId::from(id), "dropdown-fb"),
                control,
                cx.theme().secondary_hover,
                cx.theme().secondary_active,
                radius,
            );
        }
        let mut control = control
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
            .when(!editable, |d| d.focusable().tab_stop(true))
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
                    .pl_2p5()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .child(match edit {
                        Some(edit) => edit.appearance(false).p_0().into_any_element(),
                        None => div()
                            .text_sm()
                            .truncate()
                            .child(value.to_owned())
                            .into_any_element(),
                    }),
            );

        let arrow_state = self.opened.clone();
        let arrow_scroll = self.menu_scroll.clone();
        control = control.child(
            h_flex()
                .id(SharedString::from(format!("{id}-arrow")))
                .w_7()
                .h_full()
                .justify_center()
                .flex_shrink_0()
                .when(!can_open, |d| d.opacity(0.4))
                .when(uses_arrow, |d| {
                    d.focusable().tab_stop(true).on_click(move |_, window, _| {
                        arrow_scroll.set_offset(point(px(0.), px(0.)));
                        arrow_state.set(if arrow_state.get().is_some_and(|(key, _)| key == id) {
                            None
                        } else {
                            Some((id, current))
                        });
                        window.refresh();
                    })
                })
                .child(animated_chevron(id, cx.theme().muted_foreground, is_open)),
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
            let accent = cx.theme().accent;
            let items = values.into_iter().enumerate().map(|(index, value)| {
                let select = select.clone();
                let state = state.clone();
                let clicked_value = value.clone();
                let active = index == highlight || Some(index) == selected;
                let row = h_flex()
                    .id(("option", index))
                    .debug_selector(|| format!("{id}-option-{index}"))
                    .relative()
                    .h_7()
                    .px_2()
                    .w_full()
                    .gap_1p5()
                    .flex_shrink_0()
                    .when(active, |d| d.contour(Some(accent), None, 4.));
                pointer_feedback(
                    (SharedString::from(format!("{id}-opt-fb")), index),
                    row,
                    accent.opacity(0.65),
                    accent,
                    4.,
                )
                .child(
                    h_flex()
                        .w_3p5()
                        .justify_center()
                        .when(Some(index) == selected, |d| {
                            d.child(
                                Icon::new(IconName::Check)
                                    .xsmall()
                                    .text_color(cx.theme().primary),
                            )
                        }),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().popover_foreground)
                        .truncate()
                        .child(value),
                )
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
            let max_height = rems(21.)
                .to_pixels(window.rem_size())
                .min(available - px(16.));
            let desired = (px(10.) + px(28.) * items.len() as f32).min(max_height);
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
                            v_flex()
                                .id(SharedString::from(format!("{id}-popup")))
                                .debug_selector(|| format!("{id}-popup"))
                                .relative()
                                .w(bounds.size.width)
                                .max_h(max_height)
                                .p_1()
                                .text_color(cx.theme().popover_foreground)
                                .rounded(cx.theme().radius_lg)
                                .contour(
                                    Some(cx.theme().popover),
                                    Some(cx.theme().border),
                                    popup_radius,
                                )
                                .shadow_md()
                                .occlude()
                                .on_mouse_down_out(move |event, window, _| {
                                    if !bounds.contains(&event.position) {
                                        state.set(None);
                                        window.refresh();
                                    }
                                })
                                .child(
                                    v_flex()
                                        .id(SharedString::from(format!("{id}-options")))
                                        .overflow_y_scroll()
                                        .track_scroll(&self.menu_scroll)
                                        .min_h_0()
                                        .children(items),
                                )
                                .child(Scrollbar::vertical(&self.menu_scroll))
                                .with_animation(
                                    SharedString::from(format!("{id}-open")),
                                    Animation::new(Duration::from_millis(140))
                                        .with_easing(ease_out_quint()),
                                    |d, t| d.opacity(t).top(px(-4. * (1. - t))),
                                ),
                        ),
                )
                .with_priority(2),
            );
        }

        if open.is_some() {
            control.into_any_element()
        } else {
            let hint: SharedString = hint.to_owned().into();
            control
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(hint.clone()).build(window, cx)
                })
                .into_any_element()
        }
    }
}

pub fn number_stepper<T: 'static>(
    fields: &mut Fields,
    id: &'static str,
    title: &'static str,
    hint: &'static str,
    value: &str,
    int: Option<(i32, i32)>,
    window: &mut Window,
    cx: &mut Context<T>,
    callback: impl Fn(String) + 'static,
    step: impl Fn(i32) + 'static,
) -> AnyElement {
    let typed = callback;
    let (input, focus) =
        fields.input(
            id,
            value,
            "",
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
                        app.refresh();
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
    let focused = focus.is_focused(window);
    let hint_text: SharedString = hint.to_owned().into();
    let radius = f32::from(cx.theme().radius);
    let rest_border = cx.theme().input;
    let focus_border = cx.theme().ring;

    h_flex()
        .id(ElementId::Name(title.into()))
        .debug_selector(|| title.to_owned())
        .relative()
        .w(rems(8.125))
        .h_8()
        .pr_0p5()
        .flex_shrink_0()
        .rounded(cx.theme().radius)
        .contour(Some(cx.theme().input_background()), None, radius)
        .child(
            canvas(
                move |_, window, cx| {
                    smooth_value(
                        (id, "stepper-focus"),
                        if focused { 1. } else { 0. },
                        Duration::from_millis(130),
                        window,
                        cx,
                    )
                },
                move |bounds, amount, window, _| {
                    paint_outline(bounds, rest_border.opacity(1. - amount), radius, window);
                    paint_outline(bounds, focus_border.opacity(amount), radius, window);
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(hint_text.clone()).build(window, cx)
        })
        .on_key_down(move |event, _, cx| {
            if event.keystroke.key == "up" || event.keystroke.key == "down" {
                let key = key.clone();
                let delta = if event.keystroke.key == "up" { 1 } else { -1 };
                cx.stop_propagation();
                runtime::dispatch(move || key(delta));
            }
        })
        .on_scroll(move |event, window, cx| {
            let y = event.delta.pixel_delta(px(20.)).y;
            if y != px(0.) && wheel_focus.is_focused(window) {
                let wheel = wheel.clone();
                cx.stop_propagation();
                runtime::dispatch(move || wheel(if y > px(0.) { 1 } else { -1 }));
            }
        })
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .pl_2p5()
                .child(input.appearance(false).p_0()),
        )
        .children([1, -1].map(|delta| {
            let step = step.clone();
            let focus = focus.clone();
            let tooltip_text: SharedString =
                format!("{}{title}", if delta > 0 { "增加" } else { "减少" }).into();
            let held = held.clone();
            let pressed = held.clone();
            let released = held.clone();
            let leave = held;

            let btn = h_flex()
                .id(("step", delta as i32 as usize))
                .debug_selector(|| format!("{id}-{delta}"))
                .relative()
                .h_6()
                .w_6()
                .flex_shrink_0()
                .justify_center();

            pointer_feedback(
                (ElementId::from(id), if delta > 0 { "up" } else { "down" }),
                btn,
                cx.theme().accent,
                cx.theme().secondary_active,
                4.,
            )
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tooltip_text.clone()).build(window, cx)
            })
            .child(
                Icon::new(if delta > 0 {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                })
                .xsmall()
                .text_color(cx.theme().muted_foreground),
            )
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                focus.focus(window, cx);
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
                                clock.timer(Duration::from_millis(65)).await;
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
            })
        }))
        .into_any_element()
}
