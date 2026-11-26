use gpui_kit::base::{Transition, TransitionId, spring, transition};
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::{
    Animation, AnimationExt as _, App, Div, ElementId, Entity, Hsla, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, ease_out_quint, px,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

pub fn smooth_value(
    id: impl Into<TransitionId>,
    target: f32,
    duration: Duration,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    let policy = Transition::new(duration).easing(cx.theme().motion_tokens().easing_move.clone());
    transition(id, target, policy, window, cx)
}

pub fn smooth_spring(
    id: impl Into<TransitionId>,
    target: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> Pixels {
    spring(
        id,
        target,
        cx.theme().motion_tokens().spring_move,
        window,
        cx,
    )
}

#[derive(Default, Clone, Copy)]
struct PointerState {
    hovered: bool,
    pressed: bool,
}

pub fn pointer_feedback(
    id: impl Into<ElementId>,
    control: Stateful<Div>,
    hover_fill: Hsla,
    press_fill: Hsla,
    radius: f32,
) -> Stateful<Div> {
    let id: ElementId = id.into();
    let state_id = ElementId::NamedChild(id.clone().into(), "__pointer-state".into());
    let hover_id = id.clone();
    let press_id = id;
    let slot = Rc::new(RefCell::new(None::<Entity<PointerState>>));
    let hovered = slot.clone();
    let pressed = slot.clone();
    let released = slot.clone();
    let outside = slot.clone();

    let set_press = |slot: &RefCell<Option<Entity<PointerState>>>,
                     down: bool,
                     window: &mut Window,
                     cx: &mut App| {
        if let Some(state) = slot.borrow().as_ref() {
            state.update(cx, |s, _| s.pressed = down);
            window.refresh();
        }
    };

    control
        .child(
            canvas(
                move |_, window, cx| {
                    let state = window.use_keyed_state(state_id.clone(), cx, |_, _| {
                        PointerState::default()
                    });
                    *slot.borrow_mut() = Some(state.clone());
                    let s = *state.read(cx);
                    let hover = smooth_value(
                        (hover_id.clone(), "hover"),
                        if s.hovered { 1. } else { 0. },
                        Duration::from_millis(120),
                        window,
                        cx,
                    );
                    let press = smooth_value(
                        (press_id.clone(), "press"),
                        if s.pressed { 1. } else { 0. },
                        Duration::from_millis(90),
                        window,
                        cx,
                    );
                    (hover, press)
                },
                move |bounds, (hover, press), window, _| {
                    super::shape::paint_fill(bounds, hover_fill.opacity(hover), radius, window);
                    super::shape::paint_fill(bounds, press_fill.opacity(press), radius, window);
                },
            )
            .absolute()
            .top_0()
            .left_0()
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
}

pub fn entrance(id: impl Into<ElementId>, content: impl IntoElement) -> impl IntoElement {
    div().relative().size_full().overflow_hidden().child(
        v_flex()
            .absolute()
            .left_0()
            .size_full()
            .child(content)
            .with_animation(
                id,
                Animation::new(Duration::from_millis(190)).with_easing(ease_out_quint()),
                |div, t| div.opacity(t).top(px(6. * (1. - t))),
            ),
    )
}
