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
    time::Duration,
};

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
            const MENU_PADDING: f32 = 5.;
            let option_radius = offset_radius(popup_radius, MENU_PADDING);
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
                    .when(active, |d| d.contour(Some(accent), None, option_radius));
                pointer_feedback(
                    (SharedString::from(format!("{id}-opt-fb")), index),
                    row,
                    accent.opacity(0.65),
                    accent,
                    option_radius,
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
