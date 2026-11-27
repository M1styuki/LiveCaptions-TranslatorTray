use super::super::{
    motion::pointer_feedback,
    runtime,
    shape::{ContinuousCorners as _, focus_ring_layer},
};
use super::{Edit, Fields, control_highlight};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex};
use gpui_kit::{
    AnyElement, Context, ElementId, InteractiveElement as _, IntoElement as _, MouseButton,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, px,
    rems,
};
use std::{rc::Rc, time::Duration};

#[allow(clippy::too_many_arguments)]
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

    let held = fields.held(id);
    let reset = fields.reset(id);
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
        .contour_fill(cx.theme().button, radius)
        .ring_highlight(control_highlight(cx), radius)
        .child(focus_ring_layer(
            (ElementId::from(id), "stepper-focus"),
            focused,
            focus_border,
            radius,
            Duration::from_millis(130),
        ))
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
