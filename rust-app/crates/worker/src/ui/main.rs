#[path = "settings.rs"]
mod settings;

use super::{
    Weak,
    controls::*,
    model::{HistoryItem, MainState, MainWindow},
    runtime,
    shape::ContinuousCorners,
};
use gpui::{
    AnimationExt, Context, Div, FocusHandle, FontWeight, KeyDownEvent, MouseButton, Render,
    ScrollHandle, Window, div, point, prelude::*, px, relative, rgba,
};

#[derive(Default)]
pub(super) struct MainView {
    ui: Weak<MainWindow>,
    fields: Fields,
    dropdowns: Dropdowns,
    scroll: ScrollHandle,
    last_page: Option<i32>,
    delete_focus: Option<(FocusHandle, FocusHandle)>,
    interval: settings::Interval,
    wheel: settings::WheelMotion,
}

impl MainWindow {
    pub fn show(&self) -> gpui::Result<()> {
        let ui = self.clone();
        self.window()
            .show("实时字幕翻译", (900., 600.), move |window, cx| {
                let weak = ui.as_weak();
                ui.set_frame_dark(super::native::is_dark(window.appearance()));
                window.on_window_should_close(cx, move |_, _| {
                    let weak = weak.clone();
                    runtime::dispatch(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.window().request_close();
                        }
                    });
                    false
                });
                cx.observe_window_bounds(window, |this: &mut MainView, window, _| {
                    if let Some(ui) = this.ui.upgrade() {
                        ui.window().bounds_changed(window.bounds().size);
                    }
                })
                .detach();
                cx.observe_window_appearance(window, |this: &mut MainView, window, cx| {
                    sync_theme(Some(window), cx);
                    let dark = super::native::is_dark(window.appearance());
                    let weak = this.ui.clone();
                    runtime::dispatch(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_frame_dark(dark);
                            ui.invoke_frame_theme_changed();
                        }
                    });
                })
                .detach();
                MainView {
                    ui: ui.as_weak(),
                    ..Default::default()
                }
            })
    }

    pub fn hide(&self) -> gpui::Result<()> {
        self.window().hide(false);
        Ok(())
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(ui) = self.ui.upgrade() else {
            return div();
        };
        let s = ui.state();
        let p = Palette::new(s.frame_dark);
        if self.last_page.replace(s.page) != Some(s.page) {
            self.wheel.cancel();
            self.fields.clear();
            self.interval = settings::Interval::default();
            self.dropdowns.close();
            self.scroll.set_offset(point(px(0.), px(0.)));
        }
        let content = match s.page {
            0 => self.captions(&s, &ui, p).into_any_element(),
            2 => self.history(&s, &ui, p, window, cx).into_any_element(),
            _ if s.data_state == 2 => self.settings(&s, &ui, p, window, cx).into_any_element(),
            _ => recovery(&s, &ui, p).into_any_element(),
        };
        let caption_log = (s.page == 0).then(|| {
            let ui = ui.clone();
            let checked = s.caption_log_enabled;
            titlebar_button(
                p,
                "caption-cards",
                None,
                if checked {
                    "original-icons/history.svg"
                } else {
                    "original-icons/history-dismiss.svg"
                },
                "字幕历史卡片",
                checked,
                move || {
                    ui.set_caption_log_enabled(!checked);
                    ui.invoke_setting_edited("CaptionLogEnabled".into(), (!checked).to_string());
                },
            )
            .into_any_element()
        });
        let title_ui = ui.clone();
        let mut layout = col()
            .size_full()
            .relative()
            .font_family(s.ui_font_family.clone())
            .text_size(px(14.))
            .line_height(px(20.))
            .text_color(p.foreground)
            .child(titlebar(
                p,
                "实时字幕翻译",
                caption_log,
                "关闭主窗口",
                move |action| title_ui.invoke_window_action(action),
            ))
            .child(
                row()
                    .items_start()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(sidebar(&s, &ui, p, window.viewport_size().width < px(620.)))
                    .child(
                        col()
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .child(entrance(("main-page", s.page as usize), content)),
                    ),
            )
            .child(grip());
        if s.retry_save || (!s.data_message.is_empty() && s.data_message != "设置已保存。") {
            let retry_ui = ui.clone();
            let message = row()
                .w_full()
                .p(px(4.))
                .gap(px(8.))
                .child(label(s.data_message.clone()).flex_1().min_w_0())
                .children(s.retry_save.then(|| {
                    action(
                        "retry-save",
                        "重试保存",
                        "重试保存",
                        p,
                        false,
                        true,
                        move || retry_ui.invoke_data_action(4, String::new()),
                    )
                }));
            layout = col()
                .size_full()
                .child(message)
                .child(layout.flex_1().min_h_0());
        }
        if s.delete_confirm {
            let handles = self.delete_focus.get_or_insert_with(|| {
                let cancel = cx.focus_handle();
                cancel.focus(window);
                (cancel, cx.focus_handle())
            });
            layout = layout.child(delete_dialog(&ui, p, handles.clone()));
        } else {
            self.delete_focus = None;
        }
        layout.bg(p.window_background)
    }
}

fn sidebar(s: &MainState, ui: &MainWindow, p: Palette, compact: bool) -> Div {
    let navs = [
        ("字幕", "original-icons/home.svg"),
        ("设置", "original-icons/settings.svg"),
        ("历史记录", "original-icons/history.svg"),
    ];
    let pause = ui.clone();
    let captions = ui.clone();
    let topmost = ui.clone();
    let (paused, enabled, pinned) = (s.log_only, s.captions_enabled, s.topmost);
    col()
        .w(px(if compact { 60. } else { 144. }))
        .bg(p.window_background)
        .border_r_1()
        .border_color(p.pane_divider)
        .h_full()
        .flex_shrink_0()
        .px(px(10.))
        .pt(px(12.))
        .pb(px(8.))
        .gap(px(6.))
        .children(navs.into_iter().enumerate().map(|(index, (name, icon))| {
            let ui = ui.clone();
            let index = index as i32;
            let item = nav(
                name,
                if compact { "" } else { name },
                Some(icon),
                s.page == index,
                p,
                move || {
                    ui.set_page(index);
                    ui.invoke_page_changed(index);
                },
            );
            if compact {
                tip(item.pl_0().gap_0().justify_center(), name, p)
            } else {
                item
            }
        }))
        .child(div().flex_1())
        .child(divider(p).opacity(0.45))
        .child(
            row()
                .when(compact, |d| d.flex_col())
                .when(!compact, |d| d.h(px(32.)))
                .gap(px(6.))
                .child(icon_button(
                    "pause",
                    if paused {
                        "original-icons/pause-filled.svg"
                    } else {
                        "original-icons/pause.svg"
                    },
                    if paused {
                        "恢复翻译"
                    } else {
                        "暂停翻译"
                    },
                    p,
                    paused,
                    false,
                    true,
                    37.,
                    32.,
                    16.,
                    move || {
                        pause.set_log_only(!paused);
                        pause.invoke_pause_changed(!paused);
                    },
                ))
                .child(icon_button(
                    "captions",
                    if enabled {
                        "original-icons/captions.svg"
                    } else {
                        "original-icons/captions-off.svg"
                    },
                    if s.engine_busy {
                        "正在切换字幕…"
                    } else if enabled {
                        "关闭字幕"
                    } else {
                        "开启字幕"
                    },
                    p,
                    enabled,
                    false,
                    !s.engine_busy,
                    37.,
                    32.,
                    16.,
                    move || captions.invoke_captions_changed(!enabled),
                ))
                .child(icon_button(
                    "topmost",
                    if pinned {
                        "original-icons/pin.svg"
                    } else {
                        "original-icons/pin-off.svg"
                    },
                    "始终置顶",
                    p,
                    pinned,
                    false,
                    true,
                    37.,
                    32.,
                    16.,
                    move || {
                        topmost.set_topmost(!pinned);
                        topmost.invoke_topmost_changed(!pinned);
                    },
                )),
        )
}

impl MainView {
    fn captions(&self, s: &MainState, ui: &MainWindow, p: Palette) -> impl IntoElement {
        let wheel = self.wheel.clone();
        let scroll = self.scroll.clone();
        let mut content = col()
            .on_scroll_wheel(move |event, window, cx| wheel.handle(event, &scroll, window, cx))
            .px(px(20.))
            .py(px(18.))
            .gap(px(12.))
            .child(caption(
                "source",
                s.source_caption.clone(),
                s.original_font_size,
                34.,
                Some(0),
                ui,
                p,
            ))
            .children(
                (!s.caption_notice.is_empty())
                    .then(|| label(s.caption_notice.clone()).text_size(px(13.))),
            )
            .child(caption(
                "translation",
                s.translated_caption.clone(),
                s.translated_font_size,
                43.,
                Some(1),
                ui,
                p,
            ))
            .children(
                (s.latency_show && !s.latency_ms.is_empty())
                    .then(|| label(format!("延迟：{} ms", s.latency_ms)).text_size(px(12.))),
            )
            .children(
                (!s.copy_message.is_empty())
                    .then(|| label(s.copy_message.clone()).text_size(px(12.))),
            );
        if s.caption_log_enabled {
            content = content.child(col().gap(px(14.)).children(
                s.caption_cards.iter().enumerate().map(|(index, card)| {
                    col()
                        .gap(px(8.))
                        .child(caption(
                            ("source-card", index),
                            card.source.clone(),
                            12,
                            34.,
                            None,
                            ui,
                            p,
                        ))
                        .child(caption(
                            ("translated-card", index),
                            card.translation.clone(),
                            16,
                            34.,
                            None,
                            ui,
                            p,
                        ))
                }),
            ));
        }
        scroller("captions-scroll", &self.scroll, content)
    }

    fn history(
        &mut self,
        s: &MainState,
        ui: &MainWindow,
        p: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let previous = ui.clone();
        let next = ui.clone();
        let page = s.history_page;
        let size_ui = ui.clone();
        let (size, sizes) = (format!("{}/页", s.history_size), [20, 30, 50, 100]);
        let dropdown = self.dropdowns.show(
            "history-size",
            p,
            &size,
            sizes.map(|n| format!("{n}/页")).to_vec(),
            None,
            "每页条数",
            35.,
            window,
            move |index, _| size_ui.invoke_history_action(3, sizes[index]),
        );
        let search_ui = ui.clone();
        let (input, focus) = self.fields.input(
            "search",
            &s.history_query,
            "搜索",
            false,
            false,
            true,
            window,
            cx,
            move |event| match event {
                Edit::Change(text) => search_ui.set_history_query(text),
                Edit::Submit => search_ui.invoke_history_action(0, 0),
                Edit::Blur => {}
            },
        );
        let submit = ui.clone();
        let clear = ui.clone();
        let export = ui.clone();
        let delete = ui.clone();
        let refresh = ui.clone();
        let search = tip(
            field_shell("history-search", p, &focus, window, 35.)
                .pl(px(12.))
                .w((window.viewport_size().width - px(650.))
                    .max(px(150.))
                    .min(px(220.)))
                .child(div().min_w_0().flex_1().child(input))
                .children((!s.history_query.is_empty()).then(|| {
                    icon_button(
                        "history-clear",
                        "icons/close.svg",
                        "清除搜索，返回原页",
                        p,
                        false,
                        false,
                        true,
                        30.,
                        30.,
                        16.,
                        move || clear.invoke_history_action(1, 0),
                    )
                }))
                .child(icon_button(
                    "history-submit",
                    "original-icons/search.svg",
                    "搜索",
                    p,
                    false,
                    false,
                    true,
                    30.,
                    30.,
                    16.,
                    move || submit.invoke_history_action(0, 0),
                )),
            "提交搜索；支持 % 和 _ 通配符",
            p,
        );
        let ready = !s.file_busy && !s.export_busy && !s.delete_busy;
        let header = HistoryItem {
            stamp: "时间".into(),
            source: "字幕".into(),
            translation: "译文".into(),
            service: "API".into(),
            ..Default::default()
        };
        col()
            .size_full()
            .p(px(15.))
            .gap(px(5.))
            .child(
                row()
                    .gap(px(4.))
                    .flex_shrink_0()
                    .child(icon_button(
                        "history-prev",
                        "original-icons/previous.svg",
                        "上一页",
                        p,
                        false,
                        true,
                        page > 1,
                        38.,
                        30.,
                        16.,
                        move || previous.invoke_history_requested(page - 1),
                    ))
                    .child(
                        label(format!("{page}/{}", s.history_pages))
                            .min_w(px(34.))
                            .text_center()
                            .text_size(px(15.)),
                    )
                    .child(icon_button(
                        "history-next",
                        "original-icons/next.svg",
                        "下一页",
                        p,
                        false,
                        true,
                        page < s.history_pages,
                        38.,
                        30.,
                        16.,
                        move || next.invoke_history_requested(page + 1),
                    ))
                    .child(div().w(px(86.)).flex_shrink_0().child(dropdown))
                    .child(search)
                    .child(div().flex_1())
                    .child(icon_button(
                        "history-export",
                        "original-icons/download.svg",
                        if s.export_busy {
                            "正在导出全部历史"
                        } else {
                            "导出全部历史"
                        },
                        p,
                        false,
                        true,
                        ready,
                        38.,
                        30.,
                        16.,
                        move || export.invoke_file_requested(3),
                    ))
                    .child(icon_button(
                        "history-delete",
                        "original-icons/delete.svg",
                        "全部删除",
                        p,
                        false,
                        true,
                        ready,
                        38.,
                        30.,
                        16.,
                        move || delete.invoke_delete_action(0),
                    ))
                    .child(icon_button(
                        "history-refresh",
                        "original-icons/refresh.svg",
                        "刷新",
                        p,
                        false,
                        true,
                        true,
                        38.,
                        30.,
                        16.,
                        move || refresh.invoke_history_action(2, 0),
                    )),
            )
            .children((!s.export_message.is_empty()).then(|| label(s.export_message.clone())))
            .children((!s.delete_message.is_empty()).then(|| label(s.delete_message.clone())))
            .child(history_line(0, &header, true, p))
            .child(scroller(
                "history-scroll",
                &self.scroll,
                col().children(s.history_rows.iter().enumerate().map(|(i, line)| {
                    col()
                        .child(history_line(i + 1, line, false, p))
                        .child(divider(p))
                })),
            ))
    }
}

fn history_line(index: usize, entry: &HistoryItem, header: bool, p: Palette) -> Div {
    let fields = [
        (&entry.stamp, 0.4, Some(&entry.stamp_full)),
        (&entry.source, 1., None),
        (&entry.translation, 1., None),
        (&entry.service, 0.3, Some(&entry.language)),
    ];
    row()
        .items_start()
        .px(px(8.))
        .py(px(8.))
        .gap(px(8.))
        .when(header, |d| d.font_weight(FontWeight::SEMIBOLD))
        .children(
            fields
                .into_iter()
                .enumerate()
                .map(|(i, (text, ratio, hint))| {
                    let cell = label(text.clone())
                        .id(("history", index * 4 + i))
                        .flex_basis(px(0.))
                        .flex_grow()
                        .min_w_0()
                        .map(|mut d| {
                            d.style().flex_grow = Some(ratio);
                            d
                        });
                    if let Some(hint) = hint.filter(|hint| !hint.is_empty()) {
                        tip(cell, hint.clone(), p)
                    } else {
                        cell
                    }
                }),
        )
}

fn caption(
    id: impl Into<gpui::ElementId>,
    text: String,
    font_size: i32,
    min: f32,
    role: Option<i32>,
    ui: &MainWindow,
    p: Palette,
) -> impl IntoElement {
    let copy_ui = ui.clone();
    let resize = ui.clone();
    let copy = text.clone();
    let id = id.into();
    tip(
        div()
            .id(id.clone())
            .debug_selector(|| id.to_string())
            .relative()
            .w_full()
            .min_h(px(min))
            .px(px(14.))
            .py(px(10.))
            .text_size(px(font_size as f32))
            .line_height(relative(1.24))
            .contour(Some(p.control), None, 9.)
            .cursor_pointer()
            .child(text)
            .on_mouse_down(MouseButton::Left, move |_, _, _| {
                let ui = copy_ui.clone();
                let text = copy.clone();
                runtime::dispatch(move || ui.invoke_caption_copy_requested(text));
            })
            .on_scroll_wheel(move |event, _, cx| {
                let modifiers = event.modifiers;
                let amount = event.delta.pixel_delta(px(20.)).y;
                if let Some(role) = role
                    && modifiers.control
                    && !modifiers.shift
                    && !modifiers.alt
                    && !modifiers.platform
                    && amount != px(0.)
                {
                    let ui = resize.clone();
                    cx.stop_propagation();
                    runtime::dispatch(move || {
                        ui.invoke_caption_font_adjusted(role, if amount > px(0.) { 1 } else { -1 })
                    });
                }
            }),
        "单击复制字幕",
        p,
    )
}

fn recovery(s: &MainState, ui: &MainWindow, p: Palette) -> Div {
    let mut content = col().p(px(18.)).items_start().gap(px(12.));
    if s.data_state >= 3 {
        content = content.child(label(s.recovery_location.clone()));
    }
    let retry = ui.clone();
    let reset = ui.clone();
    let replace = ui.clone();
    let locate = ui.clone();
    let ready = !s.file_busy;
    if s.data_state == 3 || s.data_state == 4 {
        content = content
            .child(action(
                "reset",
                "保留原文件并恢复默认设置",
                "保留原文件并恢复默认设置",
                p,
                false,
                ready,
                move || reset.invoke_data_action(0, String::new()),
            ))
            .child(action(
                "replace",
                "选择另一份配置文件恢复…",
                "选择另一份配置文件恢复…",
                p,
                false,
                ready,
                move || replace.invoke_file_requested(1),
            ));
    }
    if s.data_state == 3 {
        content = content.child(action(
            "locate",
            "选择原数据目录完成恢复…",
            "选择原数据目录完成恢复…",
            p,
            false,
            ready,
            move || locate.invoke_file_requested(2),
        ));
    }
    if s.data_state >= 3 {
        let title = if s.data_state == 3 {
            "重试恢复"
        } else {
            "重新载入"
        };
        content = content.child(action(
            "retry-data",
            title,
            title,
            p,
            false,
            ready,
            move || retry.invoke_data_action(3, String::new()),
        ));
    }
    content
}

fn delete_dialog(
    ui: &MainWindow,
    p: Palette,
    (cancel_focus, confirm_focus): (FocusHandle, FocusHandle),
) -> impl IntoElement {
    let cancel = ui.clone();
    let confirm = ui.clone();
    let escape = ui.clone();
    let first = cancel_focus.clone();
    let second = confirm_focus.clone();
    div()
        .id("delete-dialog")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .bg(rgba(0x00000080))
        .flex()
        .items_center()
        .justify_center()
        .occlude()
        .on_key_down(move |e: &KeyDownEvent, window, cx| {
            if e.keystroke.key == "tab" {
                cx.stop_propagation();
                if first.is_focused(window) {
                    second.focus(window);
                } else {
                    first.focus(window);
                }
            }
            if e.keystroke.key == "escape" {
                cx.stop_propagation();
                let ui = escape.clone();
                runtime::dispatch(move || ui.invoke_delete_action(1));
            }
        })
        .child(
            col()
                .w(px(380.))
                .h(px(170.))
                .p(px(18.))
                .gap(px(8.))
                .contour(Some(p.background), Some(p.border), 6.)
                .child(
                    label("删除全部历史记录？")
                        .text_size(px(18.))
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(label("历史记录将永久删除，当前翻译上下文也会清空。"))
                .child(div().flex_1())
                .child(
                    row()
                        .justify_end()
                        .gap(px(8.))
                        .child(
                            action(
                                "cancel-delete",
                                "取消",
                                "取消",
                                p,
                                false,
                                true,
                                move || cancel.invoke_delete_action(1),
                            )
                            .track_focus(&cancel_focus),
                        )
                        .child(
                            action(
                                "confirm-delete",
                                "全部删除",
                                "全部删除",
                                p,
                                false,
                                true,
                                move || confirm.invoke_delete_action(2),
                            )
                            .track_focus(&confirm_focus),
                        ),
                ),
        )
        .with_animation(
            "delete-confirmation",
            gpui::Animation::new(std::time::Duration::from_millis(170))
                .with_easing(gpui::ease_out_quint()),
            |dialog, phase| dialog.opacity(phase),
        )
}
