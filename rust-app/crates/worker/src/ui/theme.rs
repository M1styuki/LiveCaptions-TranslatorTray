use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Window, px, rgb, rgba};

#[inline]
fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

#[inline]
fn a(hex: u32) -> Hsla {
    rgba(hex).into()
}

/// Applies the Windows 11 Fluent 2 light/dark theme tokens, keeping `colors`,
/// `tokens`, and the `gpui-base` projection synchronized via `Theme::update`.
pub fn sync_theme(
    window: Option<&mut Window>,
    dark_override: Option<bool>,
    has_mica: bool,
    cx: &mut App,
) {
    let dark = dark_override.unwrap_or_else(|| {
        window
            .as_ref()
            .map(|w| super::native::is_dark(w.appearance()))
            .unwrap_or_else(|| super::native::is_dark(cx.window_appearance()))
    });

    let mode = if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    if Theme::global(cx).mode != mode {
        Theme::change(mode, None, cx);
    }

    Theme::update(cx, |theme| {
        theme.font_size = px(16.);
        theme.radius = px(6.);
        theme.radius_lg = px(10.);
        theme.shadow = true;
        theme.focus_ring = true;

        if dark {
            theme.colors.background = if has_mica { a(0x202020c8) } else { c(0x202020) };
            theme.colors.foreground = c(0xf5f5f5);
            theme.colors.muted = c(0x2d2d2d);
            theme.colors.muted_foreground = c(0xa3a3a3);

            theme.colors.group_box = if has_mica { a(0xffffff0d) } else { c(0x2b2b2b) };
            theme.colors.group_box_foreground = c(0xf5f5f5);

            theme.colors.popover = c(0x2c2c2c);
            theme.colors.popover_foreground = c(0xf5f5f5);

            theme.colors.sidebar = if has_mica { a(0x20202040) } else { c(0x1c1c1c) };
            theme.colors.sidebar_foreground = c(0xf5f5f5);
            theme.colors.sidebar_accent = a(0xffffff12);
            theme.colors.sidebar_accent_foreground = c(0xffffff);
            theme.colors.sidebar_border = a(0xffffff14);

            theme.colors.title_bar = if has_mica { a(0x20202028) } else { c(0x1c1c1c) };
            theme.colors.title_bar_border = a(0xffffff12);

            theme.colors.border = a(0xffffff16);
            theme.colors.input = a(0xffffff26);

            theme.colors.primary = c(0x2892e8);
            theme.colors.primary_hover = c(0x3ba0f2);
            theme.colors.primary_active = c(0x1678cc);
            theme.colors.primary_foreground = c(0xffffff);
            theme.colors.button_primary = c(0x2892e8);
            theme.colors.button_primary_hover = c(0x3ba0f2);
            theme.colors.button_primary_active = c(0x1678cc);
            theme.colors.button_primary_foreground = c(0xffffff);

            theme.colors.secondary = a(0xffffff10);
            theme.colors.secondary_hover = a(0xffffff18);
            theme.colors.secondary_active = a(0xffffff0b);
            theme.colors.secondary_foreground = c(0xf5f5f5);
            theme.colors.button_secondary = a(0xffffff10);
            theme.colors.button_secondary_hover = a(0xffffff18);
            theme.colors.button_secondary_active = a(0xffffff0b);
            theme.colors.button_secondary_foreground = c(0xf5f5f5);

            theme.colors.button = a(0xffffff10);
            theme.colors.button_hover = a(0xffffff18);
            theme.colors.button_active = a(0xffffff0b);
            theme.colors.button_foreground = c(0xf5f5f5);

            theme.colors.accent = a(0xffffff0f);
            theme.colors.accent_foreground = c(0xf5f5f5);
            theme.colors.ring = c(0x60cdff);

            theme.colors.slider_bar = c(0x4cc2ff);
            theme.colors.slider_thumb = c(0xffffff);
            theme.colors.progress_bar = c(0x4cc2ff);

            theme.colors.danger = c(0xd13438);
            theme.colors.danger_hover = c(0xbe2b2f);
            theme.colors.danger_active = c(0xa52226);
            theme.colors.danger_foreground = c(0xffffff);
            theme.colors.button_danger = c(0xd13438);
            theme.colors.button_danger_hover = c(0xbe2b2f);
            theme.colors.button_danger_active = c(0xa52226);
            theme.colors.button_danger_foreground = c(0xffffff);

            theme.colors.table = a(0x00000000);
            theme.colors.table_head = a(0xffffff08);
            theme.colors.table_head_foreground = c(0xa3a3a3);
            theme.colors.table_hover = a(0xffffff0a);
            theme.colors.table_row_border = a(0xffffff12);

            theme.colors.overlay = a(0x00000078);
            theme.colors.switch = a(0xffffff26);
            theme.colors.switch_thumb = c(0xffffff);
            theme.colors.caret = c(0xf5f5f5);
            theme.colors.selection = a(0x4cc2ff55);
            theme.colors.scrollbar_thumb = a(0xffffff38);
            theme.colors.scrollbar_thumb_hover = a(0xffffff58);
        } else {
            theme.colors.background = if has_mica { a(0xf3f3f3c8) } else { c(0xf3f3f3) };
            theme.colors.foreground = c(0x1a1a1a);
            theme.colors.muted = c(0xeaeaea);
            theme.colors.muted_foreground = c(0x5c5c5c);

            theme.colors.group_box = if has_mica { a(0xffffffdc) } else { c(0xffffff) };
            theme.colors.group_box_foreground = c(0x1a1a1a);

            theme.colors.popover = c(0xf9f9f9);
            theme.colors.popover_foreground = c(0x1a1a1a);

            theme.colors.sidebar = if has_mica { a(0xf3f3f348) } else { c(0xeeeeee) };
            theme.colors.sidebar_foreground = c(0x1a1a1a);
            theme.colors.sidebar_accent = a(0x0000000e);
            theme.colors.sidebar_accent_foreground = c(0x1a1a1a);
            theme.colors.sidebar_border = a(0x00000014);

            theme.colors.title_bar = if has_mica { a(0xf3f3f328) } else { c(0xeeeeee) };
            theme.colors.title_bar_border = a(0x00000012);

            theme.colors.border = a(0x00000016);
            theme.colors.input = a(0x00000024);

            theme.colors.primary = c(0x0067c0);
            theme.colors.primary_hover = c(0x005fb8);
            theme.colors.primary_active = c(0x00539c);
            theme.colors.primary_foreground = c(0xffffff);
            theme.colors.button_primary = c(0x0067c0);
            theme.colors.button_primary_hover = c(0x005fb8);
            theme.colors.button_primary_active = c(0x00539c);
            theme.colors.button_primary_foreground = c(0xffffff);

            theme.colors.secondary = a(0xffffffc8);
            theme.colors.secondary_hover = a(0xf9f9f9ee);
            theme.colors.secondary_active = a(0xf0f0f0dd);
            theme.colors.secondary_foreground = c(0x1a1a1a);
            theme.colors.button_secondary = a(0xffffffc8);
            theme.colors.button_secondary_hover = a(0xf9f9f9ee);
            theme.colors.button_secondary_active = a(0xf0f0f0dd);
            theme.colors.button_secondary_foreground = c(0x1a1a1a);

            theme.colors.button = a(0xffffffcc);
            theme.colors.button_hover = a(0xf9f9f9f0);
            theme.colors.button_active = a(0xf0f0f0e0);
            theme.colors.button_foreground = c(0x1a1a1a);

            theme.colors.accent = a(0x0000000c);
            theme.colors.accent_foreground = c(0x1a1a1a);
            theme.colors.ring = c(0x0067c0);

            theme.colors.slider_bar = c(0x0067c0);
            theme.colors.slider_thumb = c(0xffffff);
            theme.colors.progress_bar = c(0x0067c0);

            theme.colors.danger = c(0xc42b1c);
            theme.colors.danger_hover = c(0xb02315);
            theme.colors.danger_active = c(0x9a1d11);
            theme.colors.danger_foreground = c(0xffffff);
            theme.colors.button_danger = c(0xc42b1c);
            theme.colors.button_danger_hover = c(0xb02315);
            theme.colors.button_danger_active = c(0x9a1d11);
            theme.colors.button_danger_foreground = c(0xffffff);

            theme.colors.table = a(0xffffff00);
            theme.colors.table_head = a(0x00000006);
            theme.colors.table_head_foreground = c(0x5c5c5c);
            theme.colors.table_hover = a(0x00000008);
            theme.colors.table_row_border = a(0x00000012);

            theme.colors.overlay = a(0x00000050);
            theme.colors.switch = a(0x00000028);
            theme.colors.switch_thumb = c(0xffffff);
            theme.colors.caret = c(0x1a1a1a);
            theme.colors.selection = a(0x0067c040);
            theme.colors.scrollbar_thumb = a(0x00000030);
            theme.colors.scrollbar_thumb_hover = a(0x0000004c);
        }
    });
}
