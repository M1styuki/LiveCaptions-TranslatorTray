use super::{
    super::{
        motion::pointer_feedback,
        shape::{ContinuousCorners as _, HighlightRing},
    },
    accent_highlight, control_highlight,
};
use gpui_kit::component::{
    ActiveTheme as _,
    button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariants as _},
};
use gpui_kit::{
    App, ElementId, Hsla, InteractiveElement as _, ParentElement as _, Stateful, Styled as _, div,
};

fn transparent_button(button: Button, fg: Hsla, cx: &App) -> Button {
    button
        .custom(ButtonCustomVariant::new(cx).foreground(fg))
        .rounded(ButtonRounded::None)
}

fn filled_ring_button(
    id: &'static str,
    button: Button,
    enabled: bool,
    fill: Hsla,
    hover: Hsla,
    active: Hsla,
    fg: Hsla,
    mut ring: HighlightRing,
    cx: &App,
) -> Stateful<gpui_kit::Div> {
    let radius = f32::from(cx.theme().radius);
    if !enabled {
        ring.top = ring.top.opacity(ring.top.a * 0.45);
        ring.rest = ring.rest.opacity(ring.rest.a * 0.45);
    }
    let mut wrap = div()
        .id((ElementId::from(id), "wrap"))
        .relative()
        .flex_shrink_0()
        .contour_fill(if enabled { fill } else { fill.opacity(0.45) }, radius);
    if enabled {
        wrap = pointer_feedback((ElementId::from(id), "btn-fb"), wrap, hover, active, radius);
    }
    wrap.ring_highlight(ring, radius)
        .child(transparent_button(button, fg, cx))
}

pub fn ring_button(
    id: &'static str,
    button: Button,
    enabled: bool,
    cx: &App,
) -> Stateful<gpui_kit::Div> {
    filled_ring_button(
        id,
        button,
        enabled,
        cx.theme().button,
        cx.theme().secondary_hover,
        cx.theme().secondary_active,
        cx.theme().button_foreground,
        control_highlight(cx),
        cx,
    )
}

pub fn primary_ring_button(
    id: &'static str,
    button: Button,
    enabled: bool,
    cx: &App,
) -> Stateful<gpui_kit::Div> {
    filled_ring_button(
        id,
        button,
        enabled,
        cx.theme().primary,
        cx.theme().primary_hover,
        cx.theme().primary_active,
        cx.theme().primary_foreground,
        accent_highlight(cx),
        cx,
    )
}

pub fn danger_ring_button(
    id: &'static str,
    button: Button,
    enabled: bool,
    cx: &App,
) -> Stateful<gpui_kit::Div> {
    filled_ring_button(
        id,
        button,
        enabled,
        cx.theme().danger,
        cx.theme().danger_hover,
        cx.theme().danger_active,
        cx.theme().danger_foreground,
        accent_highlight(cx),
        cx,
    )
}

pub fn ghost_button(
    id: &'static str,
    button: Button,
    selected: bool,
    enabled: bool,
    danger: bool,
    radius: f32,
    cx: &App,
) -> Stateful<gpui_kit::Div> {
    let fg = if danger {
        cx.theme().danger
    } else {
        cx.theme().foreground
    };
    let hover = if danger {
        cx.theme().danger.opacity(0.14)
    } else {
        cx.theme().accent.opacity(0.65)
    };
    let active = if danger {
        cx.theme().danger.opacity(0.24)
    } else {
        cx.theme().accent
    };
    let mut wrap = div()
        .id((ElementId::from(id), "wrap"))
        .relative()
        .flex_shrink_0();
    if selected {
        wrap = wrap.contour_fill(cx.theme().accent, radius);
    }
    if enabled {
        wrap = pointer_feedback(
            (ElementId::from(id), "ghost-fb"),
            wrap,
            hover,
            active,
            radius,
        );
    }
    wrap.child(transparent_button(button, fg, cx))
}
