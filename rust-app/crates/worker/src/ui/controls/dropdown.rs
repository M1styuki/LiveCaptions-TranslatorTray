use super::super::{
    motion::pointer_feedback,
    runtime,
    shape::{ContinuousCorners as _, focus_ring_layer, offset_radius},
};
use super::{animated_chevron, control_highlight};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, input::Input, scroll::Scrollbar, v_flex,
};
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, Bounds, ElementId, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Pixels, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, anchored, canvas, deferred, div,
    ease_out_quint, point, prelude::FluentBuilder as _, px, rems,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone)]
struct ClosingMenu {
    id: &'static str,
    origin: gpui_kit::Point<Pixels>,
    max_height: Pixels,
    width: Pixels,
    items: Vec<(String, bool, bool)>,
    seq: u64,
    started: Instant,
}

#[derive(Default, Clone)]
pub struct Dropdowns {
    /// `(id, active_index, keyboard_focus)`
    opened: Rc<Cell<Option<(&'static str, usize, bool)>>>,
    closing: Rc<RefCell<Option<ClosingMenu>>>,
    close_seq: Rc<Cell<u64>>,
    bounds: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
    menu_scroll: ScrollHandle,
}

#[allow(clippy::too_many_arguments)]
fn snapshot_close(
    id: &'static str,
    bounds: Bounds<Pixels>,
    viewport_height: Pixels,
    values: &[String],
    selected: Option<usize>,
    active_idx: usize,
    closing_cell: &RefCell<Option<ClosingMenu>>,
    seq_cell: &Cell<u64>,
) {
    if values.is_empty() {
        return;
    }
    let available = viewport_height;
    let max_height = px(336.).min(available - px(16.));
    let desired = (px(10.) + px(28.) * values.len() as f32).min(max_height);
    let y = if bounds.bottom() + px(4.) + desired > available && bounds.origin.y > desired {
        bounds.origin.y - px(4.) - desired
    } else {
        bounds.bottom() + px(4.)
    };
    let seq = seq_cell.get().wrapping_add(1);
    seq_cell.set(seq);
    let items = values
        .iter()
        .enumerate()
        .map(|(i, value)| (value.clone(), Some(i) == selected, i == active_idx))
        .collect();
    *closing_cell.borrow_mut() = Some(ClosingMenu {
        id,
        origin: point(bounds.origin.x, y),
        max_height,
        width: bounds.size.width,
        items,
        seq,
        started: Instant::now(),
    });
}

impl Dropdowns {
    pub fn close(&self) {
        self.opened.set(None);
        self.menu_scroll.set_offset(point(px(0.), px(0.)));
    }

    #[allow(clippy::too_many_arguments)]
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
        let open = self.opened.get().filter(|(key, _, _)| *key == id);
        let bounds = self.bounds.clone();
        let select = Rc::new(select);
        let toggle = self.opened.clone();
        let toggle_closing = self.closing.clone();
        let toggle_seq = self.close_seq.clone();
        let toggle_bounds = self.bounds.clone();
        let toggle_values = values.clone();
        let toggle_scroll = self.menu_scroll.clone();
        let current = selected.unwrap_or(0);
        let can_open = !values.is_empty();
        let editable = edit.is_some();
        let uses_arrow = editable && can_open;

        let is_open = open.is_some();
        let radius = f32::from(cx.theme().radius);
        let popup_radius = f32::from(cx.theme().radius_lg);
        let open_border = cx.theme().ring;

        let mut control = h_flex()
            .id(id)
            .debug_selector(|| id.to_owned())
            .relative()
            .w_full()
            .h_8()
            .flex_shrink_0()
            .contour_fill(cx.theme().button, radius);
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
            .ring_highlight(control_highlight(cx), radius)
            .child(focus_ring_layer(
                (ElementId::from(id), "dropdown-open"),
                is_open,
                open_border,
                radius,
                Duration::from_millis(150),
            ))
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
                    if let Some((_, active_idx, _)) = toggle.get().filter(|(key, _, _)| *key == id)
                    {
                        if let Some(tb) = toggle_bounds.borrow().get(id).copied() {
                            snapshot_close(
                                id,
                                tb,
                                window.viewport_size().height,
                                &toggle_values,
                                selected,
                                active_idx,
                                &toggle_closing,
                                &toggle_seq,
                            );
                        }
                        toggle.set(None);
                    } else {
                        *toggle_closing.borrow_mut() = None;
                        toggle.set(Some((id, current, false)));
                    }
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
        let arrow_closing = self.closing.clone();
        let arrow_seq = self.close_seq.clone();
        let arrow_bounds = self.bounds.clone();
        let arrow_values = values.clone();
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
                        if let Some((_, active_idx, _)) =
                            arrow_state.get().filter(|(key, _, _)| *key == id)
                        {
                            if let Some(tb) = arrow_bounds.borrow().get(id).copied() {
                                snapshot_close(
                                    id,
                                    tb,
                                    window.viewport_size().height,
                                    &arrow_values,
                                    selected,
                                    active_idx,
                                    &arrow_closing,
                                    &arrow_seq,
                                );
                            }
                            arrow_state.set(None);
                        } else {
                            *arrow_closing.borrow_mut() = None;
                            arrow_state.set(Some((id, current, false)));
                        }
                        window.refresh();
                    })
                })
                .child(animated_chevron(id, cx.theme().muted_foreground, is_open)),
        );

        let state = self.opened.clone();
        let key_closing = self.closing.clone();
        let key_seq = self.close_seq.clone();
        let key_bounds = self.bounds.clone();
        let keyboard_values = values.clone();
        let keyboard_select = select.clone();
        let keyboard_scroll = self.menu_scroll.clone();
        control = control.on_key_down(move |event: &KeyDownEvent, window, cx| {
            let current_open = state.get().filter(|(key, _, _)| *key == id);
            match event.keystroke.key.as_str() {
                "escape" if current_open.is_some() => {
                    if let Some(tb) = key_bounds.borrow().get(id).copied() {
                        snapshot_close(
                            id,
                            tb,
                            window.viewport_size().height,
                            &keyboard_values,
                            selected,
                            current_open.unwrap().1,
                            &key_closing,
                            &key_seq,
                        );
                    }
                    state.set(None);
                }
                "enter" | "space" if current_open.is_some() => {
                    let index = current_open.unwrap().1;
                    if let Some(tb) = key_bounds.borrow().get(id).copied() {
                        snapshot_close(
                            id,
                            tb,
                            window.viewport_size().height,
                            &keyboard_values,
                            selected,
                            index,
                            &key_closing,
                            &key_seq,
                        );
                    }
                    if let Some(value) = keyboard_values.get(index).cloned() {
                        let select = keyboard_select.clone();
                        runtime::dispatch(move || select(index, value));
                    }
                    state.set(None);
                }
                "enter" | "space" if !editable && can_open => {
                    *key_closing.borrow_mut() = None;
                    state.set(Some((id, current, true)));
                }
                "down" | "up" if can_open => {
                    *key_closing.borrow_mut() = None;
                    let index = current_open.map_or(current, |(_, index, _)| index);
                    let index = if event.keystroke.key == "down" {
                        (index + 1).min(keyboard_values.len() - 1)
                    } else {
                        index.saturating_sub(1)
                    };
                    state.set(Some((id, index, true)));
                    keyboard_scroll.scroll_to_item(index);
                }
                _ => return,
            }
            cx.stop_propagation();
            window.refresh();
        });

        if let Some((_, active_idx, keyboard_focus)) = open
            && let Some(bounds) = self.bounds.borrow().get(id).copied()
        {
            let state = self.opened.clone();
            let row_closing = self.closing.clone();
            let row_seq = self.close_seq.clone();
            let row_values = values.clone();
            let out_values = values.clone();
            let accent = cx.theme().accent;
            let primary = cx.theme().primary;
            const MENU_PADDING: f32 = 5.;
            let option_radius = offset_radius(popup_radius, MENU_PADDING);
            let items = values.into_iter().enumerate().map(|(index, value)| {
                let select = select.clone();
                let state = state.clone();
                let hover_state = state.clone();
                let click_closing = row_closing.clone();
                let click_seq = row_seq.clone();
                let click_values = row_values.clone();
                let clicked_value = value.clone();
                let is_selected = Some(index) == selected;
                let is_pointed = index == active_idx;
                let is_kbd_focused = is_pointed && keyboard_focus;
                let base = h_flex()
                    .id(("option", index))
                    .debug_selector(|| format!("{id}-option-{index}"))
                    .relative()
                    .h_7()
                    .px_2()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .flex_shrink_0();
                let row = if is_pointed {
                    base.contour_fill(accent, option_radius)
                } else {
                    pointer_feedback(
                        (SharedString::from(format!("{id}-opt-fb")), index),
                        base,
                        accent.opacity(0.65),
                        accent,
                        option_radius,
                    )
                };
                let row = row
                    .when(is_kbd_focused, |d| {
                        d.child(focus_ring_layer(
                            (
                                ElementId::from(id),
                                SharedString::from(format!("opt-kbd-{index}")),
                            ),
                            true,
                            open_border,
                            option_radius,
                            Duration::from_millis(90),
                        ))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .text_color(cx.theme().popover_foreground)
                            .truncate()
                            .child(value),
                    )
                    .child(
                        h_flex()
                            .size(px(14.))
                            .flex_shrink_0()
                            .justify_center()
                            .items_center()
                            .when(is_selected, |d| {
                                d.child(Icon::new(IconName::Check).xsmall().text_color(primary))
                            }),
                    )
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        snapshot_close(
                            id,
                            bounds,
                            window.viewport_size().height,
                            &click_values,
                            selected,
                            index,
                            &click_closing,
                            &click_seq,
                        );
                        state.set(None);
                        let select = select.clone();
                        let value = clicked_value.clone();
                        runtime::dispatch(move || select(index, value));
                        window.refresh();
                    });

                div()
                    .id((
                        ElementId::from(id),
                        SharedString::from(format!("opt-hover-{index}")),
                    ))
                    .w_full()
                    .flex_shrink_0()
                    .on_hover(move |hovered, window, _| {
                        if *hovered && hover_state.get() != Some((id, index, false)) {
                            hover_state.set(Some((id, index, false)));
                            window.refresh();
                        }
                    })
                    .child(row)
            });
            let state = self.opened.clone();
            let out_closing = self.closing.clone();
            let out_seq = self.close_seq.clone();
            let available = window.viewport_size().height;
            let max_height = rems(21.)
                .to_pixels(window.rem_size())
                .min(available - px(16.));
            let desired = (px(10.) + px(28.) * items.len() as f32).min(max_height);
            let open_above =
                bounds.bottom() + px(4.) + desired > available && bounds.origin.y > desired;
            let y = if open_above {
                bounds.origin.y - px(4.) - desired
            } else {
                bounds.bottom() + px(4.)
            };
            let dy = if open_above { 3. } else { -3. };
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
                                .p(px(MENU_PADDING))
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
                                        snapshot_close(
                                            id,
                                            bounds,
                                            window.viewport_size().height,
                                            &out_values,
                                            selected,
                                            active_idx,
                                            &out_closing,
                                            &out_seq,
                                        );
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
                                    Animation::new(Duration::from_millis(115))
                                        .with_easing(ease_out_quint()),
                                    move |d, t| d.opacity(t).top(px(dy * (1. - t))),
                                ),
                        ),
                )
                .with_priority(2),
            );
        } else {
            let expired =
                self.closing.borrow().as_ref().is_some_and(|c| {
                    c.id == id && c.started.elapsed() >= Duration::from_millis(90)
                });
            if expired {
                *self.closing.borrow_mut() = None;
            }
            if let Some(closing) = self.closing.borrow().as_ref().filter(|c| c.id == id) {
                let accent = cx.theme().accent;
                let primary = cx.theme().primary;
                const MENU_PADDING: f32 = 5.;
                let option_radius = offset_radius(popup_radius, MENU_PADDING);
                let fade_items = closing.items.iter().map(|(value, is_sel, is_ptr)| {
                    h_flex()
                        .relative()
                        .h_7()
                        .px_2()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .flex_shrink_0()
                        .when(*is_ptr, |d| d.contour_fill(accent, option_radius))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .text_color(cx.theme().popover_foreground)
                                .truncate()
                                .child(value.clone()),
                        )
                        .child(
                            h_flex()
                                .size(px(14.))
                                .flex_shrink_0()
                                .justify_center()
                                .items_center()
                                .when(*is_sel, |d| {
                                    d.child(Icon::new(IconName::Check).xsmall().text_color(primary))
                                }),
                        )
                });
                control = control.child(
                    deferred(
                        anchored()
                            .position(closing.origin)
                            .snap_to_window_with_margin(px(4.))
                            .child(
                                v_flex()
                                    .relative()
                                    .w(closing.width)
                                    .max_h(closing.max_height)
                                    .p(px(MENU_PADDING))
                                    .overflow_hidden()
                                    .text_color(cx.theme().popover_foreground)
                                    .rounded(cx.theme().radius_lg)
                                    .contour(
                                        Some(cx.theme().popover),
                                        Some(cx.theme().border),
                                        popup_radius,
                                    )
                                    .shadow_md()
                                    .child(v_flex().children(fade_items))
                                    .with_animation(
                                        SharedString::from(format!("{id}-close-{}", closing.seq)),
                                        Animation::new(Duration::from_millis(85))
                                            .with_easing(ease_out_quint()),
                                        |d, t| d.opacity(1. - t),
                                    ),
                            ),
                    )
                    .with_priority(2),
                );
            }
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
