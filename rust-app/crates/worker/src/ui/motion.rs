use gpui_kit::base::{Transition, TransitionId, spring, transition};
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::{
    Animation, AnimationExt as _, App, Div, ElementId, Entity, Hsla, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, ScrollEvent, ScrollHandle, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, ease_out_quint, point, px,
};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

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
    pointer_feedback_with(
        id,
        control,
        hover_fill,
        press_fill,
        move |bounds, color, window| {
            super::shape::paint_fill(bounds, color, radius, window);
        },
    )
}

pub fn pointer_feedback_corners(
    id: impl Into<ElementId>,
    control: Stateful<Div>,
    hover_fill: Hsla,
    press_fill: Hsla,
    corners: gpui_kit::Corners<Pixels>,
) -> Stateful<Div> {
    pointer_feedback_with(
        id,
        control,
        hover_fill,
        press_fill,
        move |bounds, color, window| {
            super::shape::paint_fill_corners(bounds, color, corners, window);
        },
    )
}

fn pointer_feedback_with(
    id: impl Into<ElementId>,
    control: Stateful<Div>,
    hover_fill: Hsla,
    press_fill: Hsla,
    paint: impl Fn(gpui_kit::Bounds<Pixels>, Hsla, &mut Window) + 'static,
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
                    let state = window
                        .use_keyed_state(state_id.clone(), cx, |_, _| PointerState::default());
                    *slot.borrow_mut() = Some(state.clone());
                    let s = *state.read(cx);
                    let hover = smooth_value(
                        (hover_id.clone(), "hover"),
                        if s.hovered { 1. } else { 0. },
                        Duration::from_millis(100),
                        window,
                        cx,
                    );
                    let press = smooth_value(
                        (press_id.clone(), "press"),
                        if s.pressed { 1. } else { 0. },
                        Duration::from_millis(75),
                        window,
                        cx,
                    );
                    (hover, press)
                },
                move |bounds, (hover, press), window, _| {
                    paint(bounds, hover_fill.opacity(hover), window);
                    paint(bounds, press_fill.opacity(press), window);
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
                Animation::new(Duration::from_millis(135)).with_easing(ease_out_quint()),
                |div, t| div.opacity(t).top(px(3. * (1. - t))),
            ),
    )
}

pub fn slide_entrance(
    id: impl Into<ElementId>,
    offset_x: f32,
    content: impl IntoElement,
) -> impl IntoElement {
    div().relative().size_full().overflow_hidden().child(
        v_flex()
            .absolute()
            .top_0()
            .size_full()
            .child(content)
            .with_animation(
                id,
                Animation::new(Duration::from_millis(175)).with_easing(ease_out_quint()),
                move |div, t| div.opacity(t).left(px(offset_x * (1. - t))),
            ),
    )
}

pub fn expand_progress(
    id: impl Into<TransitionId>,
    expanded: bool,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    let duration_ms = if expanded { 175 } else { 130 };
    smooth_value(
        id,
        if expanded { 1. } else { 0. },
        Duration::from_millis(duration_ms),
        window,
        cx,
    )
}

pub fn reveal_row(row: Div, progress: f32, max_height: f32) -> Div {
    if progress >= 0.999 {
        return row;
    }
    div()
        .w_full()
        .min_h_0()
        .overflow_hidden()
        .max_h(px(max_height * progress))
        .opacity(progress)
        .child(row.relative().top(px(-4. * (1. - progress))))
}

#[derive(Default)]
struct SmoothScrollState {
    target_y: f32,
    animating: bool,
    last_frame: Option<Instant>,
}

/// Smooth exponential ease-out mouse-wheel scroll controller that preserves
/// 1:1 native scrolling for precision touchpads and scrollbar thumb drags.
#[derive(Clone, Default)]
pub struct SmoothScroll {
    state: Rc<RefCell<SmoothScrollState>>,
}

fn animations_enabled() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut animate = 1i32;
    let _ = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(std::ptr::from_mut(&mut animate).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    animate != 0
}

impl SmoothScroll {
    pub fn reset(&self) {
        let mut state = self.state.borrow_mut();
        state.target_y = 0.;
        state.animating = false;
        state.last_frame = None;
    }

    pub fn wrap(&self, scroll: ScrollHandle, content: Div) -> Div {
        let state = self.state.clone();
        v_flex()
            .w_full()
            .flex_shrink_0()
            .on_scroll(move |event: &ScrollEvent, window, cx| {
                if event.delta.precise() || !animations_enabled() {
                    return;
                }
                let max_y = f32::from(scroll.max_offset().y).max(0.);
                if max_y <= 0. {
                    return;
                }
                let line_height = (window.line_height() * 1.4).max(px(28.));
                let delta_y = f32::from(event.delta.pixel_delta(line_height).y);
                if delta_y.abs() < f32::EPSILON {
                    return;
                }
                cx.stop_propagation();

                let current_y = f32::from(scroll.offset().y);
                let should_start = {
                    let mut s = state.borrow_mut();
                    if !s.animating {
                        s.target_y = current_y;
                    }
                    s.target_y = (s.target_y + delta_y).clamp(-max_y, 0.);
                    if s.animating {
                        false
                    } else {
                        s.animating = true;
                        s.last_frame = Some(Instant::now());
                        true
                    }
                };
                if should_start {
                    schedule_smooth_scroll_frame(state.clone(), scroll.clone(), window);
                }
            })
            .child(content)
    }
}

fn schedule_smooth_scroll_frame(
    state: Rc<RefCell<SmoothScrollState>>,
    scroll: ScrollHandle,
    window: &Window,
) {
    window.on_next_frame(move |window, _| {
        let now = Instant::now();
        let max_y = f32::from(scroll.max_offset().y).max(0.);
        let current_x = scroll.offset().x;
        let current_y = f32::from(scroll.offset().y);

        let (next_y, keep_going) = {
            let mut s = state.borrow_mut();
            if !s.animating {
                return;
            }
            s.target_y = s.target_y.clamp(-max_y, 0.);
            let dt = s
                .last_frame
                .replace(now)
                .map_or(1. / 60., |prev| now.duration_since(prev).as_secs_f32())
                .clamp(0.001, 0.05);
            let (next_y, settled) = smooth_scroll_step(current_y, s.target_y, dt);
            if settled {
                s.animating = false;
                s.last_frame = None;
            }
            (next_y, !settled)
        };

        scroll.set_offset(point(current_x, px(next_y)));
        window.refresh();
        if keep_going {
            schedule_smooth_scroll_frame(state, scroll, window);
        }
    });
}

fn smooth_scroll_step(current: f32, target: f32, dt: f32) -> (f32, bool) {
    let diff = target - current;
    if diff.abs() < 0.5 {
        return (target, true);
    }
    let alpha = 1. - (-18. * dt).exp();
    (current + diff * alpha, false)
}

#[cfg(test)]
mod tests {
    use super::smooth_scroll_step;

    #[test]
    fn smooth_scroll_step_eases_toward_target_and_settles() {
        let mut y = 0.;
        let target = -180.;
        let mut settled = false;
        for _ in 0..60 {
            let (next, done) = smooth_scroll_step(y, target, 1. / 60.);
            assert!(next <= y && next >= target);
            y = next;
            if done {
                settled = true;
                break;
            }
        }
        assert!(settled);
        assert_eq!(y, target);
    }
}
