use gpui::{App, Div, Hsla, MouseButton, Stateful, Window, canvas, prelude::*};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Tween {
    from: f32,
    target: f32,
    started: Option<Instant>,
}

impl Tween {
    fn set(&mut self, target: f32, duration: Duration) {
        if target != self.target {
            self.from = self.value(duration);
            self.target = target;
            self.started = Some(Instant::now());
        }
    }

    fn value(&self, duration: Duration) -> f32 {
        let t = self.started.map_or(1., |start| {
            (start.elapsed().as_secs_f32() / duration.as_secs_f32()).min(1.)
        });
        let eased = 1. - (1. - t).powi(3);
        self.from + (self.target - self.from) * eased
    }

    fn paint(&mut self, duration: Duration, window: &Window) -> f32 {
        let value = self.value(duration);
        if self.started.is_some_and(|start| start.elapsed() < duration) {
            window.request_animation_frame();
        } else {
            self.started = None;
        }
        value
    }
}

/// Retains the current position when the target changes mid-transition.
pub fn animate(
    id: &'static str,
    value: f32,
    duration: Duration,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    let tween = window.use_keyed_state(id, cx, |_, _| Tween {
        from: value,
        target: value,
        started: None,
    });
    tween.update(cx, |tween, _| {
        tween.set(value, duration);
        tween.paint(duration, window)
    })
}

const HOVER_MS: Duration = Duration::from_millis(140);
const PRESS_MS: Duration = Duration::from_millis(90);

#[derive(Default)]
struct Feedback {
    hover: Tween,
    press: Tween,
}

pub fn pointer_feedback(
    control: Stateful<Div>,
    hover_fill: Hsla,
    press_fill: Hsla,
    radius: f32,
) -> Stateful<Div> {
    let painted = Rc::new(RefCell::new(None::<gpui::Entity<Feedback>>));
    let hovered = painted.clone();
    let pressed = painted.clone();
    let released = painted.clone();
    let outside = painted.clone();
    let update_press = |state: &RefCell<Option<gpui::Entity<Feedback>>>,
                        down: bool,
                        window: &mut Window,
                        cx: &mut App| {
        if let Some(state) = state.borrow().as_ref() {
            state.update(cx, |feedback, _| {
                feedback.press.set(if down { 1. } else { 0. }, PRESS_MS)
            });
            window.refresh();
        }
    };
    control
        .child(
            canvas(
                move |_, window, cx| {
                    let state =
                        window.use_keyed_state("pointer-feedback", cx, |_, _| Feedback::default());
                    *painted.borrow_mut() = Some(state.clone());
                    state.update(cx, |state, _| {
                        (
                            state.hover.paint(HOVER_MS, window),
                            state.press.paint(PRESS_MS, window),
                        )
                    })
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
                state.update(cx, |feedback, _| {
                    feedback.hover.set(if *hover { 1. } else { 0. }, HOVER_MS)
                });
                window.refresh();
            }
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            update_press(&pressed, true, window, cx)
        })
        .on_mouse_up(MouseButton::Left, move |_, window, cx| {
            update_press(&released, false, window, cx)
        })
        .on_mouse_up_out(MouseButton::Left, move |_, window, cx| {
            update_press(&outside, false, window, cx)
        })
}
