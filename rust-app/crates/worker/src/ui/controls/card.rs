use super::super::{motion::pointer_feedback_corners, runtime, shape::ContinuousCorners as _};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, Corners, Div, ElementId, FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Pixels, Role, SharedString, StatefulInteractiveElement as _, Styled as _,
    div, px,
};
use std::rc::Rc;

pub fn hairline(cx: &App) -> Div {
    div()
        .w_full()
        .h(px(1.))
        .flex_shrink_0()
        .bg(cx.theme().border.opacity(0.65))
}

pub fn section_heading(title: &'static str, cx: &App) -> Div {
    div()
        .px_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

pub fn section_card(cx: &App) -> Div {
    let radius = f32::from(cx.theme().radius_lg);
    v_flex()
        .relative()
        .w_full()
        // `.rounded` shapes only `.shadow_xs()`; surface fill uses continuous corners.
        .rounded(cx.theme().radius_lg)
        .shadow_xs()
        .contour_fill(cx.theme().group_box, radius)
}

pub enum CardItem {
    Static(Div),
    Interactive(Box<dyn FnOnce(Corners<Pixels>) -> Div>),
}

impl From<Div> for CardItem {
    fn from(row: Div) -> Self {
        Self::Static(row)
    }
}

pub fn card_rows<I: Into<CardItem>>(rows: impl IntoIterator<Item = I>, cx: &App) -> Div {
    let rows: Vec<CardItem> = rows.into_iter().map(Into::into).collect();
    let count = rows.len();
    let radius = cx.theme().radius_lg;
    let surface = cx.theme().group_box;
    v_flex()
        .w_full()
        .gap(px(2.))
        .children(rows.into_iter().enumerate().map(|(i, item)| {
            let (top, bottom) = (i == 0, i + 1 == count);
            let corners = Corners {
                top_left: if top { radius } else { px(0.) },
                top_right: if top { radius } else { px(0.) },
                bottom_right: if bottom { radius } else { px(0.) },
                bottom_left: if bottom { radius } else { px(0.) },
            };
            let row = match item {
                CardItem::Static(row) => row,
                CardItem::Interactive(build) => build(corners),
            };
            // Keep `.rounded*()` solely to shape `.shadow_xs()`; tile fill uses continuous corners.
            let tile = div()
                .relative()
                .w_full()
                .overflow_hidden()
                .shadow_xs()
                .contour_fill_corners(surface, corners)
                .child(row.w_full());
            match (top, bottom) {
                (true, true) => tile.rounded(radius),
                (true, false) => tile.rounded_t(radius),
                (false, true) => tile.rounded_b(radius),
                (false, false) => tile,
            }
        }))
}

pub fn setting_row(
    title: &'static str,
    description: &'static str,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    h_flex()
        .justify_between()
        .items_center()
        .min_h(px(56.))
        .gap_4()
        .px_4()
        .py_2p5()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(cx.theme().foreground)
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                ),
        )
        .child(control)
}

pub fn navigation_row(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    hint: &'static str,
    cx: &App,
    on_click: impl Fn() + 'static,
) -> CardItem {
    let callback = Rc::new(on_click);
    let click_cb = callback.clone();
    let key_cb = callback;
    let hint_text: SharedString = hint.to_owned().into();
    let hover_fill = cx.theme().accent.opacity(0.65);
    let press_fill = cx.theme().accent;
    let fg = cx.theme().foreground;
    let muted = cx.theme().muted_foreground;

    CardItem::Interactive(Box::new(move |corners| {
        let row = h_flex()
            .id(id)
            .debug_selector(|| id.to_owned())
            .relative()
            .w_full()
            .min_h(px(56.))
            .px_4()
            .py_2p5()
            .items_center()
            .justify_between()
            .gap_4()
            .focusable()
            .tab_stop(true)
            .role(Role::Button);

        let row = pointer_feedback_corners(
            (ElementId::from(id), "nav-row-fb"),
            row,
            hover_fill,
            press_fill,
            corners,
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg)
                        .child(title),
                )
                .child(div().text_xs().text_color(muted).child(description)),
        )
        .child(Icon::new(IconName::ChevronRight).small().text_color(muted))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(hint_text.clone()).build(window, cx)
        })
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            let cb = click_cb.clone();
            runtime::dispatch(move || cb());
        })
        .on_key_down(move |event: &KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                let cb = key_cb.clone();
                runtime::dispatch(move || cb());
            }
        });

        div().w_full().child(row)
    }))
}
