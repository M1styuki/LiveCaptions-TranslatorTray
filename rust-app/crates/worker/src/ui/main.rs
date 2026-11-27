#[path = "settings.rs"]
mod settings;

use super::{
    Weak,
    controls::*,
    model::{HistoryItem, MainState, MainWindow},
    motion::{
        SmoothScroll, entrance, expand_progress, pointer_feedback, reveal_row, smooth_spring,
    },
    runtime,
    shape::{ContinuousCorners as _, offset_radius, paint_fill},
    theme::sync_theme,
};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    alert::Alert,
    button::Button,
    empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    h_flex, v_flex,
};
use gpui_kit::{
    Animation, AnimationExt as _, App, Bounds, Context, Corners, Div, ElementId, FocusHandle,
    FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton,
    ParentElement as _, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, canvas, div, ease_out_quint, point, prelude::FluentBuilder as _, px,
    relative, rems, size,
};
use std::time::Duration;

#[derive(Default)]
pub(super) struct MainView {
    ui: Weak<MainWindow>,
    fields: Fields,
    dropdowns: Dropdowns,
    scroll: ScrollHandle,
    smooth_scroll: SmoothScroll,
    last_page: Option<i32>,
    applied_theme: Option<(bool, bool)>,
    delete_focus: Option<(FocusHandle, FocusHandle)>,
}

impl MainWindow {
    pub fn show(&self) -> gpui_kit::Result<()> {
        let ui = self.clone();
        self.window()
            .show("实时字幕翻译", (900., 600.), move |window, cx| {
                let weak = ui.as_weak();
                let dark = super::native::is_dark(window.appearance());
                ui.set_frame_dark(dark);
                sync_theme(Some(window), Some(dark), ui.window().has_mica(), cx);

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
                    let dark = super::native::is_dark(window.appearance());
                    let has_mica = this.ui.upgrade().is_some_and(|ui| ui.window().has_mica());
                    sync_theme(Some(window), Some(dark), has_mica, cx);
                    this.applied_theme = Some((dark, has_mica));
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
                    applied_theme: Some((dark, ui.window().has_mica())),
                    ..Default::default()
                }
            })
    }

    pub fn hide(&self) -> gpui_kit::Result<()> {
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
        let theme_key = (s.frame_dark, ui.window().has_mica());
        if self.applied_theme != Some(theme_key) {
            self.applied_theme = Some(theme_key);
            sync_theme(Some(window), Some(s.frame_dark), ui.window().has_mica(), cx);
        }

        if self.last_page.replace(s.page) != Some(s.page) {
            self.fields.clear();
            self.dropdowns.close();
            self.scroll.set_offset(point(px(0.), px(0.)));
            self.smooth_scroll.reset();
        }

        let content = match s.page {
            0 => self.captions(&s, &ui, cx).into_any_element(),
            2 => self.history(&s, &ui, window, cx).into_any_element(),
            _ if s.data_state == 2 => self.settings(&s, &ui, window, cx).into_any_element(),
            _ => recovery(&s, &ui, cx).into_any_element(),
        };

        let caption_log = (s.page == 0).then(|| {
            let ui = ui.clone();
            let checked = s.caption_log_enabled;
            ghost_button(
                "caption-cards",
                Button::new("caption-cards")
                    .small()
                    .icon(app_icon(if checked {
                        "original-icons/history.svg"
                    } else {
                        "original-icons/history-dismiss.svg"
                    }))
                    .accessibility_label("字幕历史卡片")
                    .tooltip("字幕历史卡片")
                    .on_click(move |_, _, _| {
                        let ui = ui.clone();
                        runtime::dispatch(move || {
                            ui.set_caption_log_enabled(!checked);
                            ui.invoke_setting_edited(
                                "CaptionLogEnabled".into(),
                                (!checked).to_string(),
                            );
                        });
                    }),
                checked,
                true,
                false,
                f32::from(cx.theme().radius),
                cx,
            )
            .into_any_element()
        });

        let title_ui = ui.clone();
        let compact = window.viewport_size().width < px(620.);

        let show_data_banner =
            s.retry_save || (!s.data_message.is_empty() && s.data_message != "设置已保存。");

        let right_pane = v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .when(show_data_banner, |pane| {
                let retry_ui = ui.clone();
                let mut banner = if s.retry_save {
                    Alert::error("data-status", s.data_message.clone()).small()
                } else {
                    Alert::warning("data-status", s.data_message.clone()).small()
                };
                if s.retry_save {
                    banner = banner.banner();
                }
                pane.child(
                    h_flex()
                        .px_4()
                        .pt_3()
                        .gap_2()
                        .child(div().flex_1().min_w_0().child(banner))
                        .when(s.retry_save, |d| {
                            d.child(ring_button(
                                "retry-save",
                                Button::new("retry-save")
                                    .small()
                                    .label("重试保存")
                                    .on_click(move |_, _, _| {
                                        let retry_ui = retry_ui.clone();
                                        runtime::dispatch(move || {
                                            retry_ui.invoke_data_action(4, String::new());
                                        });
                                    }),
                                true,
                                cx,
                            ))
                        }),
                )
            })
            .child(v_flex().flex_1().min_h_0().w_full().child(entrance(
                SharedString::from(format!("main-page-{}-{}", s.page, s.data_state == 2)),
                content,
            )));

        let mut layout = v_flex()
            .size_full()
            .relative()
            .text_sm()
            .text_color(cx.theme().foreground)
            .child(titlebar(
                "实时字幕翻译",
                caption_log,
                "关闭主窗口",
                move |action| title_ui.invoke_window_action(action),
                cx,
            ))
            .child(
                h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(sidebar(&s, &ui, compact, cx))
                    .child(right_pane),
            )
            .child(grip());

        if s.delete_confirm {
            let handles = self.delete_focus.get_or_insert_with(|| {
                let cancel = cx.focus_handle();
                cancel.focus(window, cx);
                (cancel, cx.focus_handle())
            });
            layout = layout.child(delete_dialog(&ui, handles.clone(), cx));
        } else {
            self.delete_focus = None;
        }
        layout
    }
}

fn sidebar(s: &MainState, ui: &MainWindow, compact: bool, cx: &App) -> Div {
    let navs = [
        ("字幕", "original-icons/home.svg"),
        ("设置", "original-icons/settings.svg"),
        ("历史记录", "original-icons/history.svg"),
    ];
    let pause = ui.clone();
    let captions = ui.clone();
    let topmost = ui.clone();
    let (paused, enabled, pinned) = (s.log_only, s.captions_enabled, s.topmost);
    let active_index = s.page.clamp(0, 2) as f32;
    let pill_color = cx.theme().sidebar_accent;
    let pill_radius = f32::from(cx.theme().radius);

    v_flex()
        .w(if compact { rems(3.0) } else { rems(7.75) })
        .h_full()
        .flex_shrink_0()
        .bg(cx.theme().sidebar)
        .p_1p5()
        .gap_1()
        .child(
            v_flex()
                .relative()
                .gap_1()
                .child(
                    canvas(
                        move |_, window, cx| {
                            let step = rems(2.5).to_pixels(window.rem_size());
                            smooth_spring("main-nav-pill", step * active_index, window, cx)
                        },
                        move |bounds, y, window, _| {
                            let pill_h = rems(2.25).to_pixels(window.rem_size());
                            let pill = Bounds::new(
                                point(bounds.origin.x, bounds.origin.y + y),
                                size(bounds.size.width, pill_h),
                            );
                            paint_fill(pill, pill_color, pill_radius, window);
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                .children(navs.into_iter().enumerate().map(|(index, (name, icon))| {
                    let ui = ui.clone();
                    let index = index as i32;
                    nav_item(
                        name,
                        name,
                        Some(icon),
                        compact,
                        s.page == index,
                        false,
                        cx,
                        move || {
                            ui.set_page(index);
                            ui.invoke_page_changed(index);
                        },
                    )
                })),
        )
        .child(div().flex_1())
        .child(hairline(cx))
        .child(
            div()
                .flex()
                .when(compact, |d| d.flex_col().items_center())
                .when(!compact, |d| d.flex_row().justify_between())
                .pt_1()
                .gap_1()
                .child(ghost_button(
                    "pause",
                    Button::new("pause")
                        .small()
                        .icon(
                            app_icon(if paused {
                                "original-icons/pause-filled.svg"
                            } else {
                                "original-icons/pause.svg"
                            })
                            .small(),
                        )
                        .accessibility_label(if paused {
                            "恢复翻译"
                        } else {
                            "暂停翻译"
                        })
                        .tooltip(if paused {
                            "恢复翻译"
                        } else {
                            "暂停翻译"
                        })
                        .on_click(move |_, _, _| {
                            let pause = pause.clone();
                            runtime::dispatch(move || {
                                pause.set_log_only(!paused);
                                pause.invoke_pause_changed(!paused);
                            });
                        }),
                    paused,
                    true,
                    false,
                    f32::from(cx.theme().radius),
                    cx,
                ))
                .child(ghost_button(
                    "captions",
                    Button::new("captions")
                        .small()
                        .disabled(s.engine_busy)
                        .icon(
                            app_icon(if enabled {
                                "original-icons/captions.svg"
                            } else {
                                "original-icons/captions-off.svg"
                            })
                            .small(),
                        )
                        .accessibility_label(if s.engine_busy {
                            "正在切换字幕…"
                        } else if enabled {
                            "关闭字幕"
                        } else {
                            "开启字幕"
                        })
                        .tooltip(if s.engine_busy {
                            "正在切换字幕…"
                        } else if enabled {
                            "关闭字幕"
                        } else {
                            "开启字幕"
                        })
                        .on_click(move |_, _, _| {
                            let captions = captions.clone();
                            runtime::dispatch(move || captions.invoke_captions_changed(!enabled));
                        }),
                    enabled,
                    !s.engine_busy,
                    false,
                    f32::from(cx.theme().radius),
                    cx,
                ))
                .child(ghost_button(
                    "topmost",
                    Button::new("topmost")
                        .small()
                        .icon(
                            app_icon(if pinned {
                                "original-icons/pin.svg"
                            } else {
                                "original-icons/pin-off.svg"
                            })
                            .small(),
                        )
                        .accessibility_label("始终置顶")
                        .tooltip("始终置顶")
                        .on_click(move |_, _, _| {
                            let topmost = topmost.clone();
                            runtime::dispatch(move || {
                                topmost.set_topmost(!pinned);
                                topmost.invoke_topmost_changed(!pinned);
                            });
                        }),
                    pinned,
                    true,
                    false,
                    f32::from(cx.theme().radius),
                    cx,
                )),
        )
}

impl MainView {
    fn captions(&self, s: &MainState, ui: &MainWindow, cx: &App) -> impl IntoElement {
        let mut content = v_flex()
            .px_5()
            .py_3p5()
            .gap_3()
            .child(caption_block(
                "source",
                "原文",
                s.source_caption.clone(),
                "开启字幕后，将在此显示实时识别原文",
                s.original_font_size,
                2.5,
                Some(0),
                false,
                ui,
                cx,
            ))
            .when(!s.caption_notice.is_empty(), |d| {
                d.child(Alert::warning("caption-notice", s.caption_notice.clone()).small())
            })
            .child(caption_block(
                "translation",
                "译文",
                s.translated_caption.clone(),
                "译文将在此实时显示",
                s.translated_font_size,
                2.75,
                Some(1),
                true,
                ui,
                cx,
            ));

        let show_latency = s.latency_show && !s.latency_ms.is_empty();
        let show_copy = !s.copy_message.is_empty();
        if show_latency || show_copy {
            let copy_is_error = s.copy_message.contains("失败");
            content = content.child(
                h_flex()
                    .gap_3()
                    .px_1()
                    .text_xs()
                    .when(show_latency, |d| {
                        d.child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("延迟 {} ms", s.latency_ms)),
                        )
                    })
                    .when(show_copy, |d| {
                        d.child(
                            div()
                                .text_color(if copy_is_error {
                                    cx.theme().danger
                                } else {
                                    cx.theme().primary
                                })
                                .child(s.copy_message.clone()),
                        )
                    }),
            );
        }

        if s.caption_log_enabled {
            content = content.child(
                v_flex()
                    .pt_1()
                    .gap_2()
                    .child(
                        div()
                            .px_1()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().muted_foreground)
                            .child("最近记录"),
                    )
                    .children(s.caption_cards.iter().enumerate().map(|(index, card)| {
                        v_flex()
                            .gap_1p5()
                            .child(caption_block(
                                ("source-card", index),
                                "",
                                card.source.clone(),
                                "",
                                12,
                                2.0,
                                None,
                                false,
                                ui,
                                cx,
                            ))
                            .child(caption_block(
                                ("translated-card", index),
                                "",
                                card.translation.clone(),
                                "",
                                15,
                                2.0,
                                None,
                                true,
                                ui,
                                cx,
                            ))
                    })),
            );
        }

        scroller(
            "captions-scroll",
            &self.scroll,
            &self.smooth_scroll,
            content,
        )
    }

    fn history(
        &mut self,
        s: &MainState,
        ui: &MainWindow,
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
            &size,
            sizes.map(|n| format!("{n}/页")).to_vec(),
            None,
            "每页条数",
            window,
            cx,
            move |index, _| size_ui.invoke_history_action(3, sizes[index]),
        );

        let search_ui = ui.clone();
        let (input, search_focus) = self.fields.input(
            "search",
            &s.history_query,
            "搜索字幕或译文（支持 % _）",
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

        let search_input = input.prefix(Icon::new(IconName::Search).small()).when(
            !s.history_query.is_empty(),
            |i| {
                i.suffix(ghost_button(
                    "history-clear",
                    Button::new("history-clear")
                        .xsmall()
                        .icon(app_icon("icons/close.svg").xsmall())
                        .accessibility_label("清除搜索")
                        .tooltip("清除搜索，返回原页")
                        .on_click(move |_, _, _| {
                            let clear = clear.clone();
                            runtime::dispatch(move || clear.invoke_history_action(1, 0));
                        }),
                    false,
                    true,
                    false,
                    offset_radius(f32::from(cx.theme().radius), 3.),
                    cx,
                ))
            },
        );

        let ready = !s.file_busy && !s.export_busy && !s.delete_busy;
        let header = HistoryItem {
            stamp: "时间".into(),
            source: "字幕".into(),
            translation: "译文".into(),
            service: "API".into(),
            ..Default::default()
        };

        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w(rems(8.5))
                            .max_w(rems(15.))
                            .child(ring_input(
                                "search",
                                search_input,
                                &search_focus,
                                window,
                                cx,
                            )),
                    )
                    .child(ring_button(
                        "history-submit",
                        Button::new("history-submit")
                            .small()
                            .icon(app_icon("original-icons/search.svg").small())
                            .accessibility_label("搜索")
                            .tooltip("提交搜索；支持 % 和 _ 通配符")
                            .on_click(move |_, _, _| {
                                let submit = submit.clone();
                                runtime::dispatch(move || submit.invoke_history_action(0, 0));
                            }),
                        true,
                        cx,
                    ))
                    .child(div().flex_1())
                    .child(
                        h_flex()
                            .gap_1()
                            .child(ghost_button(
                                "history-prev",
                                Button::new("history-prev")
                                    .small()
                                    .disabled(page <= 1)
                                    .icon(app_icon("original-icons/previous.svg").small())
                                    .accessibility_label("上一页")
                                    .tooltip("上一页")
                                    .on_click(move |_, _, _| {
                                        let previous = previous.clone();
                                        runtime::dispatch(move || {
                                            previous.invoke_history_requested(page - 1)
                                        });
                                    }),
                                false,
                                page > 1,
                                false,
                                f32::from(cx.theme().radius),
                                cx,
                            ))
                            .child(
                                div()
                                    .px_1p5()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{page} / {}", s.history_pages)),
                            )
                            .child(ghost_button(
                                "history-next",
                                Button::new("history-next")
                                    .small()
                                    .disabled(page >= s.history_pages)
                                    .icon(app_icon("original-icons/next.svg").small())
                                    .accessibility_label("下一页")
                                    .tooltip("下一页")
                                    .on_click(move |_, _, _| {
                                        let next = next.clone();
                                        runtime::dispatch(move || {
                                            next.invoke_history_requested(page + 1)
                                        });
                                    }),
                                false,
                                page < s.history_pages,
                                false,
                                f32::from(cx.theme().radius),
                                cx,
                            )),
                    )
                    .child(div().w(rems(6.75)).flex_shrink_0().child(dropdown))
                    .child(div().w(px(1.)).h_4().bg(cx.theme().border))
                    .child(ghost_button(
                        "history-refresh",
                        Button::new("history-refresh")
                            .small()
                            .icon(app_icon("original-icons/refresh.svg").small())
                            .accessibility_label("刷新")
                            .tooltip("刷新")
                            .on_click(move |_, _, _| {
                                let refresh = refresh.clone();
                                runtime::dispatch(move || refresh.invoke_history_action(2, 0));
                            }),
                        false,
                        true,
                        false,
                        f32::from(cx.theme().radius),
                        cx,
                    ))
                    .child(ring_button(
                        "history-export",
                        Button::new("history-export")
                            .small()
                            .loading(s.export_busy)
                            .disabled(!ready)
                            .icon(app_icon("original-icons/download.svg").small())
                            .accessibility_label("导出全部历史")
                            .tooltip(if s.export_busy {
                                "正在导出全部历史"
                            } else {
                                "导出全部历史"
                            })
                            .on_click(move |_, _, _| {
                                let export = export.clone();
                                runtime::dispatch(move || export.invoke_file_requested(3));
                            }),
                        ready && !s.export_busy,
                        cx,
                    ))
                    .child(ghost_button(
                        "history-delete",
                        Button::new("history-delete")
                            .small()
                            .disabled(!ready)
                            .icon(app_icon("original-icons/delete.svg").small())
                            .accessibility_label("全部删除")
                            .tooltip("全部删除")
                            .on_click(move |_, _, _| {
                                let delete = delete.clone();
                                runtime::dispatch(move || delete.invoke_delete_action(0));
                            }),
                        false,
                        ready,
                        true,
                        f32::from(cx.theme().radius),
                        cx,
                    )),
            )
            .when(!s.export_message.is_empty(), |d| {
                let err = s.export_message.contains("未完成")
                    || s.export_message.contains("失败")
                    || s.export_message.contains("不会被覆盖");
                d.child(if err {
                    Alert::warning("history-export-msg", s.export_message.clone()).small()
                } else {
                    Alert::info("history-export-msg", s.export_message.clone()).small()
                })
            })
            .when(!s.delete_message.is_empty(), |d| {
                let err = s.delete_message.contains("未完成") || s.delete_message.contains("失败");
                d.child(if err {
                    Alert::error("history-delete-msg", s.delete_message.clone()).small()
                } else {
                    Alert::info("history-delete-msg", s.delete_message.clone()).small()
                })
            })
            .child(
                section_card(cx)
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(history_line(0, &header, true, cx))
                    .child(hairline(cx))
                    .child(if s.history_rows.row_count() == 0 {
                        v_flex()
                            .flex_1()
                            .min_h_0()
                            .items_center()
                            .justify_center()
                            .child(
                                Empty::new().header(
                                    EmptyHeader::new()
                                        .media(
                                            EmptyMedia::new()
                                                .with_variant(EmptyMediaVariant::Icon)
                                                .child(Icon::new(IconName::Inbox).small()),
                                        )
                                        .title(EmptyTitle::new().child("暂无历史记录"))
                                        .description(EmptyDescription::new().child(
                                            if s.history_query.is_empty() {
                                                "开启实时字幕翻译后，记录会自动保存在这里。"
                                            } else {
                                                "没有找到匹配的记录，可尝试调整关键词。"
                                            },
                                        )),
                                ),
                            )
                            .into_any_element()
                    } else {
                        scroller(
                            "history-scroll",
                            &self.scroll,
                            &self.smooth_scroll,
                            v_flex().children(s.history_rows.iter().enumerate().map(
                                |(i, line)| {
                                    v_flex()
                                        .child(history_line(i + 1, line, false, cx))
                                        .child(hairline(cx))
                                },
                            )),
                        )
                        .into_any_element()
                    }),
            )
    }
}

fn history_line(index: usize, entry: &HistoryItem, header: bool, cx: &App) -> Div {
    let fields = [
        (&entry.stamp, 0.4, Some(&entry.stamp_full)),
        (&entry.source, 1., None),
        (&entry.translation, 1., None),
        (&entry.service, 0.3, Some(&entry.language)),
    ];
    h_flex()
        .items_start()
        .px_3()
        .py_2()
        .gap_3()
        .when(header, |d| {
            let head_corners = Corners {
                top_left: cx.theme().radius_lg,
                top_right: cx.theme().radius_lg,
                bottom_right: px(0.),
                bottom_left: px(0.),
            };
            d.relative()
                .contour_fill_corners(cx.theme().table_head, head_corners)
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.theme().table_head_foreground)
        })
        .when(!header, |d| {
            d.text_sm()
                .text_color(cx.theme().foreground)
                .hover(|row| row.bg(cx.theme().table_hover))
        })
        .children(
            fields
                .into_iter()
                .enumerate()
                .map(|(i, (text, ratio, hint))| {
                    let cell = div()
                        .id(("history", index * 4 + i))
                        .flex_basis(px(0.))
                        .flex_grow(ratio)
                        .min_w_0()
                        .child(text.clone());
                    if let Some(hint) = hint.filter(|hint| !hint.is_empty()) {
                        let hint: SharedString = hint.clone().into();
                        cell.tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(hint.clone())
                                .build(window, cx)
                        })
                        .into_any_element()
                    } else {
                        cell.into_any_element()
                    }
                }),
        )
}

#[allow(clippy::too_many_arguments)]
fn caption_block(
    id: impl Into<ElementId>,
    eyebrow: &'static str,
    text: String,
    placeholder: &'static str,
    font_size: i32,
    min_rems: f32,
    role: Option<i32>,
    accent_eyebrow: bool,
    ui: &MainWindow,
    cx: &App,
) -> impl IntoElement {
    let copy_ui = ui.clone();
    let resize = ui.clone();
    let copy = text.clone();
    let id = id.into();
    let is_empty = text.is_empty();
    let radius = f32::from(cx.theme().radius_lg);

    let card = h_flex()
        .id(id.clone())
        .debug_selector(|| id.to_string())
        .relative()
        .w_full()
        .min_h(rems(min_rems))
        .when(is_empty, |d| d.items_center())
        .when(!is_empty, |d| d.items_baseline())
        .px_3p5()
        .py_2p5()
        .gap_3()
        .contour(Some(cx.theme().group_box), Some(cx.theme().border), radius);

    pointer_feedback(
        (id, "caption-fb"),
        card,
        cx.theme().secondary_hover,
        cx.theme().secondary_active,
        radius,
    )
    .tooltip(|window, cx| {
        gpui_kit::component::tooltip::Tooltip::new("单击复制字幕").build(window, cx)
    })
    .when(!eyebrow.is_empty(), |d| {
        d.child(
            div()
                .w(rems(2.25))
                .flex_shrink_0()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(if accent_eyebrow {
                    cx.theme().primary
                } else {
                    cx.theme().muted_foreground
                })
                .child(eyebrow),
        )
    })
    .child(
        div()
            .flex_1()
            .min_w_0()
            .when(is_empty, |d| d.text_xs().line_height(relative(1.35)))
            .when(!is_empty, |d| {
                d.text_size(px(font_size as f32))
                    .line_height(relative(1.25))
            })
            .text_color(if is_empty {
                cx.theme().muted_foreground
            } else {
                cx.theme().foreground
            })
            .child(if is_empty {
                placeholder.to_owned()
            } else {
                text
            }),
    )
    .on_mouse_down(MouseButton::Left, move |_, _, _| {
        let ui = copy_ui.clone();
        let text = copy.clone();
        runtime::dispatch(move || ui.invoke_caption_copy_requested(text));
    })
    .on_scroll(move |event, _, cx| {
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
    })
}

fn recovery(s: &MainState, ui: &MainWindow, cx: &App) -> Div {
    let retry = ui.clone();
    let reset = ui.clone();
    let replace = ui.clone();
    let locate = ui.clone();
    let ready = !s.file_busy;

    let mut card = section_card(cx).max_w(rems(34.)).p_5().gap_3().child(
        div()
            .text_base()
            .font_weight(FontWeight::SEMIBOLD)
            .child("需要恢复数据"),
    );

    if s.data_state >= 3 && !s.recovery_location.is_empty() {
        card = card.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(s.recovery_location.clone()),
        );
    }

    let mut actions = h_flex().flex_wrap().gap_2().pt_1();
    if s.data_state == 3 || s.data_state == 4 {
        actions = actions
            .child(ring_button(
                "reset",
                Button::new("reset")
                    .small()
                    .disabled(!ready)
                    .label("保留原文件并恢复默认设置")
                    .on_click(move |_, _, _| {
                        let reset = reset.clone();
                        runtime::dispatch(move || reset.invoke_data_action(0, String::new()));
                    }),
                ready,
                cx,
            ))
            .child(ring_button(
                "replace",
                Button::new("replace")
                    .small()
                    .disabled(!ready)
                    .label("选择另一份配置文件恢复…")
                    .on_click(move |_, _, _| {
                        let replace = replace.clone();
                        runtime::dispatch(move || replace.invoke_file_requested(1));
                    }),
                ready,
                cx,
            ));
    }
    if s.data_state == 3 {
        actions = actions.child(ring_button(
            "locate",
            Button::new("locate")
                .small()
                .disabled(!ready)
                .label("选择原数据目录完成恢复…")
                .on_click(move |_, _, _| {
                    let locate = locate.clone();
                    runtime::dispatch(move || locate.invoke_file_requested(2));
                }),
            ready,
            cx,
        ));
    }
    if s.data_state >= 3 {
        let title = if s.data_state == 3 {
            "重试恢复"
        } else {
            "重新载入"
        };
        actions = actions.child(primary_ring_button(
            "retry-data",
            Button::new("retry-data")
                .small()
                .disabled(!ready)
                .label(title)
                .on_click(move |_, _, _| {
                    let retry = retry.clone();
                    runtime::dispatch(move || retry.invoke_data_action(3, String::new()));
                }),
            ready,
            cx,
        ));
    }

    v_flex()
        .size_full()
        .p_6()
        .items_center()
        .justify_center()
        .child(card.child(actions))
}

fn delete_dialog(
    ui: &MainWindow,
    (cancel_focus, confirm_focus): (FocusHandle, FocusHandle),
    cx: &App,
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
        .bg(cx.theme().overlay)
        .flex()
        .items_center()
        .justify_center()
        .occlude()
        .on_key_down(move |e: &KeyDownEvent, window, cx| {
            if e.keystroke.key == "tab" {
                cx.stop_propagation();
                if first.is_focused(window) {
                    second.focus(window, cx);
                } else {
                    first.focus(window, cx);
                }
            }
            if e.keystroke.key == "escape" {
                cx.stop_propagation();
                let ui = escape.clone();
                runtime::dispatch(move || ui.invoke_delete_action(1));
            }
            if e.keystroke.key == "enter" || e.keystroke.key == "space" {
                cx.stop_propagation();
                let action = if second.is_focused(window) { 2 } else { 1 };
                let ui = escape.clone();
                runtime::dispatch(move || ui.invoke_delete_action(action));
            }
        })
        .child(
            v_flex()
                .relative()
                .w(rems(24.))
                .p_5()
                .gap_3()
                .text_color(cx.theme().popover_foreground)
                .rounded(cx.theme().radius_lg)
                .contour(
                    Some(cx.theme().popover),
                    None,
                    f32::from(cx.theme().radius_lg),
                )
                .shadow_xl()
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("删除全部历史记录？"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("历史记录将永久删除，当前翻译上下文也会清空。"),
                )
                .child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .pt_2()
                        .child(ring_button(
                            "cancel-delete",
                            Button::new("cancel-delete")
                                .label("取消")
                                .track_focus(&cancel_focus)
                                .on_click(move |_, _, _| {
                                    let cancel = cancel.clone();
                                    runtime::dispatch(move || cancel.invoke_delete_action(1));
                                }),
                            true,
                            cx,
                        ))
                        .child(danger_ring_button(
                            "confirm-delete",
                            Button::new("confirm-delete")
                                .label("全部删除")
                                .track_focus(&confirm_focus)
                                .on_click(move |_, _, _| {
                                    let confirm = confirm.clone();
                                    runtime::dispatch(move || confirm.invoke_delete_action(2));
                                }),
                            true,
                            cx,
                        )),
                ),
        )
        .with_animation(
            "delete-confirmation",
            Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()),
            |dialog, phase| dialog.opacity(phase),
        )
}
