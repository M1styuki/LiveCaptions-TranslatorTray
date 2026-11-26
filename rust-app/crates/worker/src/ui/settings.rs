use super::*;
use gpui_kit::component::{
    ActiveTheme as _,
    alert::Alert,
    h_flex,
    slider::{Slider, SliderEvent, SliderState},
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Styled as _, Window, div, rems,
};

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
    fn ensure_interval_slider(
        &mut self,
        ui: &MainWindow,
        value: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SliderState> {
        if let Some((state, applied, _)) = &mut self.interval_slider {
            if *applied != value {
                *applied = value;
                state.update(cx, |slider, cx| {
                    if slider.value().end().round() as i32 != value {
                        slider.set_value(value as f32, window, cx);
                    }
                });
            }
            return state.clone();
        }

        let state = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(10.)
                .step(1.)
                .default_value(value as f32)
        });
        let view_ui = ui.clone();
        let sub = cx.subscribe(&state, move |this, _, event: &SliderEvent, _| {
            let SliderEvent::Change(val) = event else {
                return;
            };
            let next = val.end().round().clamp(0., 10.) as i32;
            if let Some((_, applied, _)) = &mut this.interval_slider {
                *applied = next;
            }
            let view_ui = view_ui.clone();
            runtime::dispatch(move || {
                if view_ui.get_max_sync_interval() != next {
                    set(&view_ui, "MaxSyncInterval", next);
                }
            });
        });
        self.interval_slider = Some((state.clone(), value, sub));
        state
    }

    pub(super) fn settings(
        &mut self,
        s: &MainState,
        ui: &MainWindow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let service_ui = ui.clone();
        let service = self.dropdowns.show(
            "service",
            &s.selected_service,
            SERVICES.map(String::from).to_vec(),
            None,
            "当前翻译服务；DeepSeek 使用 OpenAI 兼容接口",
            window,
            cx,
            move |_, value| set(&service_ui, "ApiName", value),
        );

        let lang = ui.clone();
        let (language_input, _) = self.fields.input(
            "language",
            &s.selected_language,
            "",
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
            &s.selected_language,
            LANGUAGES.map(String::from).to_vec(),
            Some(language_input),
            "译文语言代码，如 zh-CN",
            window,
            cx,
            move |_, value| set(&lang, "TargetLanguage", value),
        );

        let slider_state = self.ensure_interval_slider(ui, s.max_sync_interval, window, cx);
        let slider = div()
            .id("interval-slider-wrap")
            .debug_selector(|| "interval-slider".to_owned())
            .w(rems(11.25))
            .flex_shrink_0()
            .child(
                Slider::new(&slider_state)
                    .horizontal()
                    .accessibility_label("调用间隔"),
            );

        let config = ui.clone();
        let sentences_text = s.display_sentences.to_string();
        let sentences_stepper = number_stepper(
            &mut self.fields,
            "sentences",
            "悬浮字幕前文句数",
            "保留显示的前文句数：0–6",
            &sentences_text,
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

        let aware = s.context_aware;
        let contexts_text = s.num_contexts.to_string();
        let contexts_stepper = aware.then(|| {
            let config = ui.clone();
            number_stepper(
                &mut self.fields,
                "contexts",
                "上下文句数",
                "翻译上下文句数：1–10",
                &contexts_text,
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
            )
        });

        let api_ui = ui.clone();
        let translation_card =
            v_flex()
                .gap_1p5()
                .child(section_heading("翻译", cx))
                .child(card_rows(
                    [
                        setting_row(
                            "翻译服务",
                            "Google 无需配置；DeepSeek 使用 OpenAI 兼容接口。",
                            div().w(rems(10.)).flex_shrink_0().child(service),
                            cx,
                        ),
                        setting_row(
                            "翻译服务与提示词配置",
                            "管理各翻译服务的接口地址、密钥与模型提示词",
                            contour_button(
                                "configure-api",
                                "配置",
                                "打开翻译服务与提示词设置",
                                cx,
                                move || api_ui.invoke_api_requested(),
                            ),
                            cx,
                        ),
                        setting_row(
                            "目标语言",
                            "译文的语言代码，例如 zh-CN。",
                            div().w(rems(10.)).flex_shrink_0().child(language),
                            cx,
                        ),
                        setting_row(
                            "调用间隔",
                            "按字幕变化次数控制调用频率，数值越小越频繁。",
                            h_flex()
                                .gap_3()
                                .flex_shrink_0()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{} 次", s.max_sync_interval)),
                                )
                                .child(slider),
                            cx,
                        ),
                    ],
                    cx,
                ));

        let config = ui.clone();
        let mut context_rows = vec![
            setting_row(
                "前文句数",
                "悬浮字幕中保留的前文句数，可设为 0–6。",
                sentences_stepper,
                cx,
            ),
            setting_row(
                "上下文翻译",
                "向翻译服务提供前文；模型服务会使用更多 token。",
                win11_switch(
                    "context-aware",
                    aware,
                    if aware {
                        "上下文翻译：已开启"
                    } else {
                        "上下文翻译：已关闭"
                    },
                    "上下文翻译",
                    cx,
                    move |next| {
                        let config = config.clone();
                        runtime::dispatch(move || set(&config, "ContextAware", next));
                    },
                ),
                cx,
            ),
        ];
        if let Some(stepper) = contexts_stepper {
            context_rows.push(setting_row(
                "上下文句数",
                "翻译时提供给服务的前文句数，可设为 1–10。",
                stepper,
                cx,
            ));
        }
        let context_card = v_flex()
            .gap_1p5()
            .child(section_heading("字幕与上下文", cx))
            .child(card_rows(context_rows, cx));

        let visibility = ui.clone();
        let hidden = s.live_captions_hidden;
        let latency = ui.clone();
        let latency_checked = s.latency_show;
        let autostart = ui.clone();
        let checked = s.autostart_enabled;

        let system_card = v_flex()
            .gap_1p5()
            .child(section_heading("窗口与系统", cx))
            .child(card_rows(
                [
                    setting_row(
                        "显示系统字幕",
                        "在系统字幕中修改源语言；隐藏窗口后仍可识别。",
                        win11_switch(
                            "system-captions",
                            !hidden,
                            if !hidden {
                                "显示系统字幕：已开启"
                            } else {
                                "显示系统字幕：已关闭"
                            },
                            "显示系统字幕",
                            cx,
                            move |_| {
                                let visibility = visibility.clone();
                                runtime::dispatch(move || {
                                    visibility.invoke_captions_visibility_changed(!hidden)
                                });
                            },
                        ),
                        cx,
                    ),
                    setting_row(
                        "显示翻译延迟",
                        "在字幕页显示最近一次翻译请求的耗时（毫秒）。",
                        win11_switch(
                            "latency",
                            latency_checked,
                            if latency_checked {
                                "显示翻译延迟：已开启"
                            } else {
                                "显示翻译延迟：已关闭"
                            },
                            "显示翻译延迟",
                            cx,
                            move |next| {
                                let latency = latency.clone();
                                runtime::dispatch(move || set(&latency, "LatencyShow", next));
                            },
                        ),
                        cx,
                    ),
                    setting_row(
                        "开机自启动",
                        "登录 Windows 后驻留托盘；字幕需手动开启。",
                        win11_switch(
                            "autostart",
                            checked,
                            if checked {
                                "开机自启动：已开启"
                            } else {
                                "开机自启动：已关闭"
                            },
                            "开机自启动",
                            cx,
                            move |next| {
                                let autostart = autostart.clone();
                                runtime::dispatch(move || autostart.invoke_autostart_changed(next));
                            },
                        ),
                        cx,
                    ),
                ],
                cx,
            ))
            .when(!s.autostart_message.is_empty(), |d| {
                d.child(Alert::warning("autostart-msg", s.autostart_message.clone()).small())
            });

        scroller(
            "settings-scroll",
            &self.scroll,
            &self.smooth_scroll,
            v_flex()
                .w_full()
                .max_w(rems(52.))
                .mx_auto()
                .px_6()
                .py_3p5()
                .gap_3p5()
                .child(translation_card)
                .child(context_card)
                .child(system_card),
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

fn section_heading(title: &'static str, cx: &App) -> Div {
    div()
        .px_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

fn setting_row(
    title: &'static str,
    description: &'static str,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    h_flex()
        .justify_between()
        .items_center()
        .gap_4()
        .px_4()
        .py_2()
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
