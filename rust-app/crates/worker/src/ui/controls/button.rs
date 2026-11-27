use super::{accent_highlight, control_highlight};
use crate::ui::shape::ContinuousCorners as _;
use gpui_kit::component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
};
use gpui_kit::{App, Div, ParentElement as _, Styled as _, div};

pub fn ring_button(button: Button, cx: &App) -> Div {
    let radius = f32::from(cx.theme().radius);
    div()
        .relative()
        .flex_shrink_0()
        .child(button.secondary())
        .ring_highlight(control_highlight(cx), radius)
}

pub fn accent_ring_button(button: Button, cx: &App) -> Div {
    let radius = f32::from(cx.theme().radius);
    div()
        .relative()
        .flex_shrink_0()
        .child(button)
        .ring_highlight(accent_highlight(cx), radius)
}
