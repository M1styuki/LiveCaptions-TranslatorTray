mod button;
mod card;
mod dropdown;
mod fields;
mod slider;
mod stepper;
mod switch;

pub use button::{danger_ring_button, ghost_button, primary_ring_button, ring_button};
pub use card::{card_rows, hairline, navigation_row, section_card, section_heading, setting_row};
pub use dropdown::Dropdowns;
pub use fields::{Edit, Fields, ring_input, ring_textarea};
pub use slider::win11_slider;
pub use stepper::number_stepper;
pub use switch::win11_switch;

use super::{
    motion::{SmoothScroll, pointer_feedback, smooth_value},
    runtime,
    shape::{HighlightRing, paint_fill},
};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariants as _},
    h_flex,
    scroll::Scrollbar,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AssetSource, CursorStyle, Div, ElementId, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement as _,
    PathBuilder, Role, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _,
    WindowControlArea, canvas, div, point, prelude::FluentBuilder as _, px,
};
use std::{borrow::Cow, rc::Rc, time::Duration};

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        let local = match path {
            "icons/bold.svg" => {
                Some(include_bytes!("../../../../../assets/icons/bold.svg").as_slice())
            }
            "icons/captions.svg" => {
                Some(include_bytes!("../../../../../assets/icons/captions.svg").as_slice())
            }
            "icons/close.svg" => {
                Some(include_bytes!("../../../../../assets/icons/close.svg").as_slice())
            }
            "icons/color.svg" => {
                Some(include_bytes!("../../../../../assets/icons/color.svg").as_slice())
            }
            "icons/delete.svg" => {
                Some(include_bytes!("../../../../../assets/icons/delete.svg").as_slice())
            }
            "icons/download.svg" => {
                Some(include_bytes!("../../../../../assets/icons/download.svg").as_slice())
            }
            "icons/font-decrease.svg" => {
                Some(include_bytes!("../../../../../assets/icons/font-decrease.svg").as_slice())
            }
            "icons/font-increase.svg" => {
                Some(include_bytes!("../../../../../assets/icons/font-increase.svg").as_slice())
            }
            "icons/history.svg" => {
                Some(include_bytes!("../../../../../assets/icons/history.svg").as_slice())
            }
            "icons/home.svg" => {
                Some(include_bytes!("../../../../../assets/icons/home.svg").as_slice())
            }
            "icons/lock.svg" => {
                Some(include_bytes!("../../../../../assets/icons/lock.svg").as_slice())
            }
            "icons/minimize.svg" => {
                Some(include_bytes!("../../../../../assets/icons/minimize.svg").as_slice())
            }
            "icons/next.svg" => {
                Some(include_bytes!("../../../../../assets/icons/next.svg").as_slice())
            }
            "icons/opacity-decrease.svg" => {
                Some(include_bytes!("../../../../../assets/icons/opacity-decrease.svg").as_slice())
            }
            "icons/opacity-increase.svg" => {
                Some(include_bytes!("../../../../../assets/icons/opacity-increase.svg").as_slice())
            }
            "icons/panel-bottom.svg" => {
                Some(include_bytes!("../../../../../assets/icons/panel-bottom.svg").as_slice())
            }
            "icons/pause.svg" => {
                Some(include_bytes!("../../../../../assets/icons/pause.svg").as_slice())
            }
            "icons/pin.svg" => {
                Some(include_bytes!("../../../../../assets/icons/pin.svg").as_slice())
            }
            "icons/play.svg" => {
                Some(include_bytes!("../../../../../assets/icons/play.svg").as_slice())
            }
            "icons/previous.svg" => {
                Some(include_bytes!("../../../../../assets/icons/previous.svg").as_slice())
            }
            "icons/refresh.svg" => {
                Some(include_bytes!("../../../../../assets/icons/refresh.svg").as_slice())
            }
            "icons/search.svg" => {
                Some(include_bytes!("../../../../../assets/icons/search.svg").as_slice())
            }
            "icons/settings.svg" => {
                Some(include_bytes!("../../../../../assets/icons/settings.svg").as_slice())
            }
            "icons/stroke-decrease.svg" => {
                Some(include_bytes!("../../../../../assets/icons/stroke-decrease.svg").as_slice())
            }
            "icons/stroke-increase.svg" => {
                Some(include_bytes!("../../../../../assets/icons/stroke-increase.svg").as_slice())
            }
            "icons/swap.svg" => {
                Some(include_bytes!("../../../../../assets/icons/swap.svg").as_slice())
            }
            "icons/tray-disabled.svg" => {
                Some(include_bytes!("../../../../../assets/icons/tray-disabled.svg").as_slice())
            }
            "icons/tray-enabled.svg" => {
                Some(include_bytes!("../../../../../assets/icons/tray-enabled.svg").as_slice())
            }
            "original-icons/bold.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/bold.svg").as_slice())
            }
            "original-icons/captions-off.svg" => Some(
                include_bytes!("../../../../../assets/original-icons/captions-off.svg").as_slice(),
            ),
            "original-icons/captions.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/captions.svg").as_slice())
            }
            "original-icons/color.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/color.svg").as_slice())
            }
            "original-icons/delete.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/delete.svg").as_slice())
            }
            "original-icons/download.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/download.svg").as_slice())
            }
            "original-icons/font-decrease.svg" => Some(
                include_bytes!("../../../../../assets/original-icons/font-decrease.svg").as_slice(),
            ),
            "original-icons/font-increase.svg" => Some(
                include_bytes!("../../../../../assets/original-icons/font-increase.svg").as_slice(),
            ),
            "original-icons/history-dismiss.svg" => Some(
                include_bytes!("../../../../../assets/original-icons/history-dismiss.svg")
                    .as_slice(),
            ),
            "original-icons/history-title.svg" => Some(
                include_bytes!("../../../../../assets/original-icons/history-title.svg").as_slice(),
            ),
            "original-icons/history.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/history.svg").as_slice())
            }
            "original-icons/home.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/home.svg").as_slice())
            }
            "original-icons/info.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/info.svg").as_slice())
            }
            "original-icons/lock.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/lock.svg").as_slice())
            }
            "original-icons/next.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/next.svg").as_slice())
            }
            "original-icons/number-decrease.svg" => Some(
                include_bytes!("../../../../../assets/original-icons/number-decrease.svg")
                    .as_slice(),
            ),
            "original-icons/number-increase.svg" => Some(
                include_bytes!("../../../../../assets/original-icons/number-increase.svg")
                    .as_slice(),
            ),
            "original-icons/opacity-decrease.svg" => Some(
                include_bytes!("../../../../../assets/original-icons/opacity-decrease.svg")
                    .as_slice(),
            ),
            "original-icons/opacity-increase.svg" => Some(
                include_bytes!("../../../../../assets/original-icons/opacity-increase.svg")
                    .as_slice(),
            ),
            "original-icons/panel-bottom.svg" => Some(
                include_bytes!("../../../../../assets/original-icons/panel-bottom.svg").as_slice(),
            ),
            "original-icons/pause-filled.svg" => Some(
                include_bytes!("../../../../../assets/original-icons/pause-filled.svg").as_slice(),
            ),
            "original-icons/pause.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/pause.svg").as_slice())
            }
            "original-icons/pin-off.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/pin-off.svg").as_slice())
            }
            "original-icons/pin.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/pin.svg").as_slice())
            }
            "original-icons/play.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/play.svg").as_slice())
            }
            "original-icons/previous.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/previous.svg").as_slice())
            }
            "original-icons/prompt.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/prompt.svg").as_slice())
            }
            "original-icons/refresh.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/refresh.svg").as_slice())
            }
            "original-icons/search.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/search.svg").as_slice())
            }
            "original-icons/settings.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/settings.svg").as_slice())
            }
            "original-icons/stroke-decrease.svg" => Some(
                include_bytes!("../../../../../assets/original-icons/stroke-decrease.svg")
                    .as_slice(),
            ),
            "original-icons/stroke-increase.svg" => Some(
                include_bytes!("../../../../../assets/original-icons/stroke-increase.svg")
                    .as_slice(),
            ),
            "original-icons/swap.svg" => {
                Some(include_bytes!("../../../../../assets/original-icons/swap.svg").as_slice())
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

pub fn control_highlight(cx: &App) -> HighlightRing {
    if cx.theme().is_dark() {
        HighlightRing {
            top: cx.theme().switch_thumb.opacity(0.16),
            rest: cx.theme().switch_thumb.opacity(0.08),
            stop: 0.28,
        }
    } else {
        HighlightRing {
            top: cx.theme().foreground.opacity(0.08),
            rest: cx.theme().foreground.opacity(0.14),
            stop: 0.82,
        }
    }
}

pub fn accent_highlight(cx: &App) -> HighlightRing {
    HighlightRing {
        top: cx.theme().switch_thumb.opacity(0.24),
        rest: cx.theme().switch_thumb.opacity(0.10),
        stop: 0.28,
    }
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
    let hover_fill = if selected {
        cx.theme().transparent
    } else {
        cx.theme().sidebar_accent.opacity(0.65)
    };
    let press_fill = if selected {
        cx.theme().transparent
    } else {
        cx.theme().sidebar_accent
    };
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
