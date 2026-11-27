use super::super::{
    runtime,
    shape::{ContinuousCorners as _, focus_ring_layer},
};
use super::control_highlight;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
};
use gpui_kit::{
    App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, ParentElement as _, Stateful, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Duration,
};

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

    pub(super) fn held(&self, id: &'static str) -> Rc<Cell<(i32, u64)>> {
        self.0[&id].held.clone()
    }

    pub(super) fn reset(&self, id: &'static str) -> Rc<Cell<bool>> {
        self.0[&id].reset.clone()
    }

    fn sync_existing(entry: &Field, value: &str, window: &mut Window, cx: &mut App) -> FocusHandle {
        let focus = entry.state.focus_handle(cx);
        let focused = focus.is_focused(window);
        if entry.pending.get() == 0 {
            let reset = entry.reset.replace(false);
            let model_changed = *entry.applied.borrow() != value;
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

    #[allow(clippy::too_many_arguments)]
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
        let state = if let Some(entry) = self.0.get(id) {
            let focus = Self::sync_existing_with_force(entry, value, force_value, window, cx);
            let FieldKind::Single(state) = &entry.state else {
                unreachable!("field {id} kind mismatch");
            };
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

pub fn ring_input(
    id: &'static str,
    input: Input,
    focus: &FocusHandle,
    window: &Window,
    cx: &App,
) -> Stateful<gpui_kit::Div> {
    let focused = focus.is_focused(window);
    let radius = f32::from(cx.theme().radius);
    h_flex()
        .id((ElementId::from(id), "ring-input"))
        .relative()
        .w_full()
        .h_8()
        .px_2p5()
        .rounded(cx.theme().radius)
        .contour_fill(cx.theme().button, radius)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(input.appearance(false).p_0()),
        )
        .ring_highlight(control_highlight(cx), radius)
        .child(focus_ring_layer(
            (ElementId::from(id), "input-focus"),
            focused,
            cx.theme().ring,
            radius,
            Duration::from_millis(130),
        ))
}

pub fn ring_textarea(
    id: &'static str,
    textarea: Textarea,
    focus: &FocusHandle,
    window: &Window,
    cx: &App,
) -> Stateful<gpui_kit::Div> {
    let focused = focus.is_focused(window);
    let radius = f32::from(cx.theme().radius);
    div()
        .id((ElementId::from(id), "ring-textarea"))
        .relative()
        .w_full()
        .p_2()
        .rounded(cx.theme().radius)
        .contour_fill(cx.theme().button, radius)
        .child(textarea.appearance(false).p_0())
        .ring_highlight(control_highlight(cx), radius)
        .child(focus_ring_layer(
            (ElementId::from(id), "textarea-focus"),
            focused,
            cx.theme().ring,
            radius,
            Duration::from_millis(130),
        ))
}
