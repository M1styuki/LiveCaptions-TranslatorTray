use super::*;
use gpui::{Bounds, Pixels, canvas, size};
use std::time::Duration;
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
pub(super) struct Interval {
    bounds: Rc<Cell<Bounds<Pixels>>>,
    dragging: Rc<Cell<bool>>,
}

impl Interval {
    fn render(&self, ui: &MainWindow, value: i32, p: Palette) -> impl IntoElement {
        let bounds = self.bounds.clone();
        let drag = self.dragging.clone();
        let click = self.dragging.clone();
        let release = self.dragging.clone();
        let release_out = self.dragging.clone();
        let capture = self.bounds.clone();
        let painting_drag = self.dragging.clone();
        let view = ui.clone();
        let set_position = Rc::new(move |x: Pixels| {
            let rect = bounds.get();
            let travel = (rect.size.width - px(20.)).max(px(1.));
            let value = (((x - rect.left() - px(10.)) / travel) * 10.)
                .round()
                .clamp(0., 10.) as i32;
            let ui = view.clone();
            runtime::dispatch(move || {
                if ui.get_max_sync_interval() != value {
                    set(&ui, "MaxSyncInterval", value);
                }
            });
        });
        let move_position = set_position.clone();
        let keyboard = ui.clone();
        let percent = value as f32 / 10.;
        let thumb_background = if p.dark {
            gpui::rgb(0x454545)
        } else {
            gpui::rgb(0xffffff)
        };
        tip(
            div()
                .id("interval-slider")
                .debug_selector(|| "interval-slider".to_owned())
                .relative()
                .flex_1()
                .min_w_0()
                .h(px(20.))
                .focusable()
                .tab_stop(true)
                .child(
                    canvas(
                        move |bounds, window, cx| {
                            capture.set(bounds);
                            let hover = bounds.contains(&window.mouse_position())
                                && window.is_window_active();
                            let knob = if painting_drag.get() {
                                5.5
                            } else if hover {
                                7.
                            } else {
                                6.
                            };
                            crate::ui::motion::animate(
                                "interval-thumb",
                                knob,
                                Duration::from_millis(140),
                                window,
                                cx,
                            )
                        },
                        move |bounds, knob, window, _| {
                            let width = (bounds.size.width - px(20.)).max(px(0.));
                            let center = bounds.center();
                            let start = bounds.left() + px(10.);
                            let active = width * percent;
                            let track =
                                Bounds::new(point(start, center.y - px(2.)), size(width, px(4.)));
                            super::super::shape::paint_fill(track, p.border, 2., window);
                            super::super::shape::paint_fill(
                                Bounds::new(track.origin, size(active, track.size.height)),
                                p.accent,
                                2.,
                                window,
                            );
                            let thumb = Bounds::new(
                                point(start + active - px(10.), center.y - px(10.)),
                                size(px(20.), px(20.)),
                            );
                            window.paint_quad(gpui::quad(
                                thumb,
                                px(10.),
                                thumb_background,
                                px(1.),
                                p.border,
                                gpui::BorderStyle::Solid,
                            ));
                            let dot = Bounds::new(
                                point(start + active - px(knob), center.y - px(knob)),
                                size(px(knob * 2.), px(knob * 2.)),
                            );
                            window.paint_quad(gpui::quad(
                                dot,
                                px(knob),
                                p.accent,
                                px(0.),
                                p.border,
                                gpui::BorderStyle::Solid,
                            ));
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                .on_hover(|_, window, _| window.refresh())
                .on_mouse_down(MouseButton::Left, move |event, window, _| {
                    click.set(true);
                    window.refresh();
                    set_position(event.position.x);
                })
                .on_mouse_move(move |event, _, _| {
                    if drag.get() && event.pressed_button == Some(MouseButton::Left) {
                        move_position(event.position.x);
                    }
                })
                .on_mouse_up(MouseButton::Left, move |_, window, _| {
                    release.set(false);
                    window.refresh();
                })
                .on_mouse_up_out(MouseButton::Left, move |_, window, _| {
                    release_out.set(false);
                    window.refresh();
                })
                .on_key_down(move |e: &KeyDownEvent, _, cx| {
                    let value = match e.keystroke.key.as_str() {
                        "left" | "down" => (value - 1).max(0),
                        "right" | "up" => (value + 1).min(10),
                        "home" => 0,
                        "end" => 10,
                        _ => return,
                    };
                    cx.stop_propagation();
                    let ui = keyboard.clone();
                    runtime::dispatch(move || set(&ui, "MaxSyncInterval", value));
                }),
            format!("调用间隔：{value}；按字幕变化次数计"),
            p,
        )
    }
}

const SERVICES: [&str; 11] = [
    "Google",
    "Google2",
    "Ollama",
    "OpenAI",
    "LMStudio",
    "DeepL",
    "OpenRouter",
    "Youdao",
    "MTranServer",
    "Baidu",
    "LibreTranslate",
];
const LANGUAGES: [&str; 13] = [
    "zh-CN", "zh-TW", "en-US", "en-GB", "ja-JP", "ko-KR", "fr-FR", "th-TH", "ru-RU", "es-ES",
    "pt-BR", "tr-TR", "ar-SA",
];

impl MainView {
    pub(super) fn settings(
        &mut self,
        s: &MainState,
        ui: &MainWindow,
        p: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let service_ui = ui.clone();
        let service = self.dropdowns.show(
            "service",
            p,
            &s.selected_service,
            SERVICES.map(String::from).to_vec(),
            None,
            "当前翻译服务；DeepSeek 使用 OpenAI 兼容接口",
            34.,
            window,
            move |_, value| set(&service_ui, "ApiName", value),
        );
        let lang = ui.clone();
        let (language, _) = self.fields.input(
            "language",
            &s.selected_language,
            "",
            false,
            false,
            true,
            window,
            cx,
            move |event| {
                if let Edit::Change(value) = event {
                    set(&lang, "TargetLanguage", value);
                }
            },
        );
        let lang = ui.clone();
        let language = self.dropdowns.show(
            "target-language",
            p,
            &s.selected_language,
            LANGUAGES.map(String::from).to_vec(),
            Some(language),
            "译文语言代码，如 zh-CN",
            34.,
            window,
            move |_, value| set(&lang, "TargetLanguage", value),
        );

        let slider = self.interval.render(ui, s.max_sync_interval, p);
        let api_ui = ui.clone();
        let translate = group("翻译", p)
            .child(
                row()
                    .gap(px(18.))
                    .child(column("翻译服务", service))
                    .child(column("目标语言", language)),
            )
            .child(
                row()
                    .gap(px(12.))
                    .child(
                        note("Google 无需配置；DeepSeek 使用 OpenAI 兼容接口。")
                            .flex_1()
                            .min_w_0(),
                    )
                    .child(action(
                        "configure-api",
                        "配置翻译服务",
                        "配置翻译服务",
                        p,
                        true,
                        true,
                        move || api_ui.invoke_api_requested(),
                    )),
            )
            .child(
                row()
                    .gap(px(14.))
                    .child(
                        row()
                            .gap(px(8.))
                            .flex_shrink_0()
                            .child(label("API 调用间隔"))
                            .child(
                                label(format!("{} 次", s.max_sync_interval))
                                    .min_w(px(32.))
                                    .text_color(p.foreground.opacity(0.75)),
                            ),
                    )
                    .child(slider),
            )
            .child(note("按字幕内容变化次数控制调用频率，数值越小越频繁。"));

        let config = ui.clone();
        let count = s.num_contexts;
        let context = number(
            &mut self.fields,
            "contexts",
            "上下文句数",
            "翻译上下文句数：1–10",
            &count.to_string(),
            p,
            Some((1, 10)),
            window,
            cx,
            {
                let config = config.clone();
                move |value| set(&config, "NumContexts", value)
            },
            move |delta| {
                set(
                    &config,
                    "NumContexts",
                    (config.get_num_contexts() + delta).clamp(1, 10),
                )
            },
        );
        let config = ui.clone();
        let count = s.display_sentences;
        let sentences = number(
            &mut self.fields,
            "sentences",
            "悬浮字幕前文句数",
            "保留显示的前文句数：0–6",
            &count.to_string(),
            p,
            Some((0, 6)),
            window,
            cx,
            {
                let config = config.clone();
                move |value| set(&config, "DisplaySentences", value)
            },
            move |delta| {
                set(
                    &config,
                    "DisplaySentences",
                    (config.get_display_sentences() + delta).clamp(0, 6),
                )
            },
        );
        let config = ui.clone();
        let aware = s.context_aware;
        let context = group("上下文", p)
            .child(
                row()
                    .gap(px(16.))
                    .child(description(
                        "结合前文翻译",
                        "向翻译服务提供前文；模型服务会使用更多 token。",
                    ))
                    .child(toggle(
                        "context-aware",
                        "上下文翻译",
                        aware,
                        p,
                        move || set(&config, "ContextAware", !aware),
                    )),
            )
            .child(
                row()
                    .gap(px(18.))
                    .child(number_column("上下文句数", context))
                    .child(number_column("悬浮字幕前文句数", sentences)),
            );

        let visibility = ui.clone();
        let is_hidden = s.live_captions_hidden;
        let title = if is_hidden {
            "显示系统字幕"
        } else {
            "隐藏系统字幕"
        };
        let latency = ui.clone();
        let latency_checked = s.latency_show;
        let display = group("显示", p)
            .child(
                row()
                    .gap(px(16.))
                    .child(description(
                        "Windows 实时字幕",
                        "在系统字幕中修改源语言；隐藏窗口后仍可识别。",
                    ))
                    .child(action(
                        "system-captions",
                        title,
                        title,
                        p,
                        false,
                        true,
                        move || visibility.invoke_captions_visibility_changed(!is_hidden),
                    )),
            )
            .child(divider(p))
            .child(
                row()
                    .gap(px(16.))
                    .child(label("显示翻译延迟").flex_1())
                    .child(toggle(
                        "latency",
                        "显示延迟",
                        latency_checked,
                        p,
                        move || set(&latency, "LatencyShow", !latency_checked),
                    )),
            );
        let autostart = ui.clone();
        let checked = s.autostart_enabled;
        let startup = group("启动", p)
            .child(
                row()
                    .gap(px(16.))
                    .child(description(
                        "开机自启动",
                        "登录 Windows 后驻留托盘；字幕需手动开启。",
                    ))
                    .child(toggle(
                        "autostart",
                        "开机自启动",
                        checked,
                        p,
                        move || autostart.invoke_autostart_changed(!checked),
                    )),
            )
            .children((!s.autostart_message.is_empty()).then(|| note(s.autostart_message.clone())));
        scroller(
            "settings-scroll",
            &self.scroll,
            col()
                .p(px(22.))
                .gap(px(16.))
                .child(heading("设置"))
                .child(note("翻译服务、字幕显示与上下文设置。"))
                .child(translate)
                .child(context)
                .child(display)
                .child(startup),
        )
    }
}

fn set(ui: &MainWindow, field: &'static str, value: impl ToString) {
    let value = value.to_string();
    match field {
        "ApiName" => ui.set_selected_service(value.clone()),
        "TargetLanguage" => ui.set_selected_language(value.clone()),
        "NumContexts" => ui.set_num_contexts(value.parse().unwrap()),
        "DisplaySentences" => ui.set_display_sentences(value.parse().unwrap()),
        "ContextAware" => ui.set_context_aware(value == "true"),
        "LatencyShow" => ui.set_latency_show(value == "true"),
        "MaxSyncInterval" => ui.set_max_sync_interval(value.parse().unwrap()),
        _ => unreachable!(),
    }
    ui.invoke_setting_edited(field.into(), value);
}

fn group(title: &'static str, p: Palette) -> Div {
    surface(p).p(px(18.)).gap(px(12.)).child(
        label(title)
            .text_size(px(16.))
            .font_weight(FontWeight::SEMIBOLD),
    )
}

fn column(title: &'static str, content: impl IntoElement) -> Div {
    col()
        .flex_1()
        .min_w_0()
        .gap(px(7.))
        .child(label(title))
        .child(content)
}

fn description(title: &'static str, details: &'static str) -> Div {
    col()
        .flex_1()
        .gap(px(4.))
        .child(label(title))
        .child(note(details))
}

fn number_column(title: &'static str, content: impl IntoElement) -> Div {
    col()
        .w(px(130.))
        .flex_shrink_0()
        .gap(px(7.))
        .child(label(title))
        .child(content)
}
