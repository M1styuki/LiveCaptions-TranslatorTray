use super::super::{
    motion::{pointer_feedback, smooth_value},
    runtime,
};
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{
    App, Bounds, ElementId, Entity, InteractiveElement as _, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Role, Stateful,
    StatefulInteractiveElement as _, Styled as _, canvas, div, fill, point, px, rems, size,
};
use std::{cell::Cell, rc::Rc, time::Duration};

const THUMB_RADIUS: f32 = 8.;
const TRACK_INSET: f32 = 10.;
const SLIDER_HEIGHT: f32 = 28.;

#[derive(Default, Clone, Copy)]
struct SliderState {
    hovered: bool,
    dragging: bool,
}

fn slider_fraction(bounds: Bounds<Pixels>, x: Pixels) -> f32 {
    let inset = px(TRACK_INSET);
    let travel = (bounds.size.width - inset * 2.).max(px(1.));
    ((x - bounds.origin.x - inset) / travel).clamp(0., 1.)
}

#[allow(clippy::too_many_arguments)]
pub fn win11_slider(
    id: &'static str,
    debug_name: &'static str,
    aria_label: &'static str,
    value: i32,
    min: i32,
    max: i32,
    step: i32,
    cx: &App,
    on_change: impl Fn(i32) + 'static,
) -> Stateful<gpui_kit::Div> {
    let slot = Rc::new(Cell::new(None::<Bounds<Pixels>>));
    let geometry = slot.clone();
    let slider_bar = cx.theme().slider_bar;
    let slider_track = slider_bar.opacity(0.22);
    let slider_thumb = cx.theme().slider_thumb;
    let thumb_ring = slider_bar.opacity(0.65);
    let on_change = Rc::new(on_change);
    let down_change = on_change.clone();
    let drag_change = on_change.clone();
    let key_change = on_change;

    let snap = move |fraction: f32| {
        let range = i64::from(max) - i64::from(min);
        let step = i64::from(step.max(1));
        (i64::from(min)
            + ((fraction.clamp(0., 1.) as f64 * range as f64 / step as f64).round() as i64) * step)
            .clamp(i64::from(min), i64::from(max)) as i32
    };

    let state_id = ElementId::NamedChild(ElementId::from(id).into(), "__slider-state".into());
    let drag_slot = Rc::new(std::cell::RefCell::new(None::<Entity<SliderState>>));
    let hover_slot = drag_slot.clone();
    let down_drag = drag_slot.clone();

    let track = canvas(
        move |_, window, cx| {
            let state = window.use_keyed_state(state_id.clone(), cx, |_, _| SliderState::default());
            let s = *state.read(cx);
            let target_ratio = if max > min {
                ((value as f64 - min as f64) / (max as f64 - min as f64)).clamp(0., 1.) as f32
            } else {
                0.
            };
            let ratio = smooth_value(
                (ElementId::from(id), "sl-pos"),
                target_ratio,
                if s.dragging {
                    Duration::ZERO
                } else {
                    Duration::from_millis(110)
                },
                window,
                cx,
            );
            let core_t = smooth_value(
                (ElementId::from(id), "sl-core"),
                if s.hovered || s.dragging { 1. } else { 0. },
                Duration::from_millis(110),
                window,
                cx,
            );
            (state, ratio, core_t)
        },
        move |bounds, (state, ratio, core_t), window, _cx| {
            geometry.set(Some(bounds));
            *drag_slot.borrow_mut() = Some(state.clone());

            let inset = px(TRACK_INSET);
            let outer_r = px(THUMB_RADIUS + 0.5 * core_t);
            let inner_r = px((THUMB_RADIUS - 2.5) + 1.0 * core_t);
            let left = bounds.origin.x + inset;
            let width = (bounds.size.width - inset * 2.).max(px(1.));
            let x = left + width * ratio;
            let y = bounds.origin.y + bounds.size.height * 0.5;
            let rail = |length, color| {
                fill(
                    Bounds::new(point(left, y - px(3.)), size(length, px(6.))),
                    color,
                )
                .corner_radii(px(3.))
            };
            window.paint_quad(rail(width, slider_track));
            if x > left {
                window.paint_quad(rail(x - left, slider_bar));
            }
            window.paint_quad(
                fill(
                    Bounds::new(
                        point(x - outer_r, y - outer_r),
                        size(outer_r * 2., outer_r * 2.),
                    ),
                    thumb_ring,
                )
                .corner_radii(outer_r),
            );
            window.paint_quad(
                fill(
                    Bounds::new(
                        point(x - inner_r, y - inner_r),
                        size(inner_r * 2., inner_r * 2.),
                    ),
                    slider_thumb,
                )
                .corner_radii(inner_r),
            );

            let drag_state = state.clone();
            let drag_change = drag_change.clone();
            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                if !phase.bubble() || !ev.dragging() || !drag_state.read(cx).dragging {
                    return;
                }
                let next = snap(slider_fraction(bounds, ev.position.x));
                let drag_change = drag_change.clone();
                runtime::dispatch(move || drag_change(next));
            });

            let up_state = state;
            window.on_mouse_event(move |ev: &MouseUpEvent, _, _, cx| {
                if ev.button == MouseButton::Left && up_state.read(cx).dragging {
                    up_state.update(cx, |active, _| active.dragging = false);
                }
            });
        },
    )
    .size_full();

    let bar = pointer_feedback(
        (ElementId::from(id), "slider-fb"),
        h_flex()
            .id(id)
            .relative()
            .size_full()
            .h(px(SLIDER_HEIGHT))
            .focusable()
            .tab_stop(true)
            .role(Role::Slider)
            .aria_label(aria_label),
        cx.theme().accent.opacity(0.55),
        cx.theme().accent,
        f32::from(cx.theme().radius),
    )
    .on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, window, cx| {
        if let Some(state) = down_drag.borrow().as_ref() {
            state.update(cx, |active, _| active.dragging = true);
        }
        if let Some(bounds) = slot.get() {
            let next = snap(slider_fraction(bounds, ev.position.x));
            let down_change = down_change.clone();
            runtime::dispatch(move || down_change(next));
            window.refresh();
        }
    })
    .on_key_down(move |ev: &KeyDownEvent, _, cx| {
        let delta = match ev.keystroke.key.as_str() {
            "left" | "down" => -step.max(1),
            "right" | "up" => step.max(1),
            _ => return,
        };
        let next = (value + delta).clamp(min, max);
        let key_change = key_change.clone();
        cx.stop_propagation();
        runtime::dispatch(move || key_change(next));
    })
    .child(track);

    div()
        .id(debug_name)
        .debug_selector(|| debug_name.to_owned())
        .w(rems(11.25))
        .flex_shrink_0()
        .on_hover(move |hovered, window, cx| {
            if let Some(state) = hover_slot.borrow().as_ref() {
                state.update(cx, |active, _| active.hovered = *hovered);
                window.refresh();
            }
        })
        .child(bar)
}
