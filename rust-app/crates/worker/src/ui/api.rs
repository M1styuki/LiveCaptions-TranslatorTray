use super::{
    Weak,
    controls::*,
    model::{ApiState, ApiWindow},
    motion::entrance,
    runtime,
    theme::sync_theme,
};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::{
    Context, Div, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    ScrollHandle, SharedString, Styled as _, Window, div, point, prelude::FluentBuilder as _, px,
    rems,
};

#[derive(Default)]
pub(super) struct ApiView {
    ui: Weak<ApiWindow>,
    fields: Fields,
    dropdowns: Dropdowns,
    scroll: ScrollHandle,
    nav_scroll: ScrollHandle,
    last_editor: Option<(i32, i32)>,
    applied_theme: Option<(bool, bool)>,
}

impl ApiWindow {
    pub(crate) const INITIAL_SIZE: (f32, f32) = (950., 600.);

    pub fn show(&self) -> gpui_kit::Result<()> {
        let ui = self.clone();
        self.window()
            .show("API 设置", Self::INITIAL_SIZE, move |window, cx| {
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
                cx.observe_window_appearance(window, |this: &mut ApiView, window, cx| {
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
                ApiView {
                    ui: ui.as_weak(),
                    applied_theme: Some((dark, ui.window().has_mica())),
                    ..Default::default()
                }
            })
    }

    pub fn hide(&self) -> gpui_kit::Result<()> {
        self.window().hide(true);
        Ok(())
    }

    #[cfg(test)]
    pub fn invoke_section_count(&self) -> i32 {
        self.get_editors().row_count() as i32
    }
}

impl Render for ApiView {
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

        let editor = (s.section, s.active_editor.config_index);
        let previous = self.last_editor.replace(editor);
        if previous != Some(editor) {
            self.fields.clear();
            self.dropdowns.close();
            if previous.map(|previous| previous.0) != Some(editor.0) {
                self.scroll.set_offset(point(px(0.), px(0.)));
            }
        }

        let navs = [
            "提示词",
            "Ollama",
            "LMStudio",
            "OpenAI 兼容",
            "OpenRouter",
            "DeepL",
            "Youdao",
            "MTranServer",
            "Baidu",
            "LibreTranslate",
        ];
        let sidebar_width = if window.viewport_size().width < px(640.) {
            rems(7.75)
        } else {
            rems(9.5)
        };

        let nav_item = |index: i32, name: &'static str| {
            let ui = ui.clone();
            nav_item(
                name,
                name,
                None,
                false,
                index == s.section,
                true,
                cx,
                move || ui.invoke_section_changed(index),
            )
        };

        let sidebar = v_flex()
            .w(sidebar_width)
            .h_full()
            .flex_shrink_0()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .child(scroller(
                "api-navigation",
                &self.nav_scroll,
                v_flex()
                    .p_2p5()
                    .gap_1()
                    .child(
                        div()
                            .px_2p5()
                            .pt_1()
                            .pb_0p5()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().muted_foreground)
                            .child("通用"),
                    )
                    .child(nav_item(0, navs[0]))
                    .child(
                        div()
                            .px_2p5()
                            .pt_3()
                            .pb_0p5()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().muted_foreground)
                            .child("翻译服务"),
                    )
                    .children(
                        navs[1..]
                            .iter()
                            .enumerate()
                            .map(|(i, name)| nav_item((i + 1) as i32, name)),
                    ),
            ));

        let title = if s.section == 0 {
            "提示词"
        } else if s.service == "OpenAI" {
            "OpenAI 兼容"
        } else {
            &s.service
        };

        let mut content = v_flex()
            .w_full()
            .max_w(rems(42.))
            .p_6()
            .gap_4()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title.to_owned()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(if s.section == 0 {
                                "自定义翻译要求，适用于模型翻译服务。"
                            } else {
                                "修改后自动保存；配置此服务不会切换当前翻译服务。"
                            }),
                    ),
            )
            .when(!s.message.is_empty(), |d| {
                let err = s.message.contains("失败")
                    || s.message.contains("无效")
                    || s.message.contains("仍保留");
                d.child(if err {
                    Alert::warning("api-message", s.message.clone()).small()
                } else {
                    Alert::info("api-message", s.message.clone()).small()
                })
            });

        if s.section == 0 {
            content = content.child(self.prompt(&s, &ui, window, cx));
        } else if (s.section as usize) <= s.editors.row_count() {
            content = content.child(self.editor(&s, &ui, window, cx));
        }

        let title_ui = ui.clone();
        v_flex()
            .size_full()
            .relative()
            .bg(cx.theme().background)
            .text_sm()
            .text_color(cx.theme().foreground)
            .child(titlebar(
                "翻译服务设置",
                None,
                "关闭 API 设置",
                move |action| title_ui.invoke_window_action(action),
                cx,
            ))
            .child(hairline(cx))
            .child(
                h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(sidebar)
                    .child(v_flex().flex_1().h_full().min_w_0().child(entrance(
                        SharedString::from(format!(
                            "api-section-{}-{}",
                            s.section, s.active_editor.config_index
                        )),
                        scroller("api-scroll", &self.scroll, content),
                    ))),
            )
            .child(grip())
    }
}

impl ApiView {
    fn prompt(
        &mut self,
        s: &ApiState,
        ui: &ApiWindow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let ui = ui.clone();
        let (editor, _) = self.fields.textarea(
            "prompt",
            &s.prompt,
            "翻译提示词；保留目标语言占位符 {0}",
            window,
            cx,
            move |event| {
                if let Edit::Change(value) = event {
                    ui.invoke_prompt_edited(value);
                }
            },
        );
        section_card(cx)
            .p_5()
            .gap_3()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("{0} 代表目标语言，需保留在提示词中。源文本用 🔤 标记包围。"),
            )
            .child(editor)
    }

    fn editor(
        &mut self,
        s: &ApiState,
        ui: &ApiWindow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let section = s.section;
        let entry = &s.active_editor;
        let service = entry.service.as_str();
        let llm = matches!(service, "Ollama" | "LMStudio" | "OpenAI" | "OpenRouter");
        let action_ui = |kind, value| {
            let ui = ui.clone();
            move || ui.invoke_editor_action(section, kind, value)
        };

        let prev_cb = action_ui(0, 0);
        let next_cb = action_ui(0, 1);
        let new_cb = action_ui(0, 2);
        let del_cb = action_ui(0, 3);

        let mut form = section_card(cx)
            .p_5()
            .gap_4()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("配置"),
                    )
                    .child(
                        Button::new("previous-config")
                            .ghost()
                            .small()
                            .disabled(entry.config_index <= 0)
                            .icon(app_icon("original-icons/previous.svg").small())
                            .accessibility_label("上一个配置")
                            .tooltip("上一个配置")
                            .on_click(move |_, _, _| {
                                let prev_cb = prev_cb.clone();
                                runtime::dispatch(move || prev_cb());
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} / {}",
                                entry.config_index + 1,
                                entry.config_count
                            )),
                    )
                    .child(
                        Button::new("next-config")
                            .ghost()
                            .small()
                            .disabled(entry.config_index + 1 >= entry.config_count)
                            .icon(app_icon("original-icons/next.svg").small())
                            .accessibility_label("下一个配置")
                            .tooltip("下一个配置")
                            .on_click(move |_, _, _| {
                                let next_cb = next_cb.clone();
                                runtime::dispatch(move || next_cb());
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("new-config")
                            .outline()
                            .small()
                            .label("新建")
                            .tooltip("新增此服务的配置")
                            .on_click(move |_, _, _| {
                                let new_cb = new_cb.clone();
                                runtime::dispatch(move || new_cb());
                            }),
                    )
                    .child(
                        Button::new("delete-config")
                            .ghost()
                            .danger()
                            .small()
                            .disabled(entry.config_count <= 1)
                            .label("删除")
                            .tooltip("删除当前配置；每项服务至少保留一项")
                            .on_click(move |_, _, _| {
                                let del_cb = del_cb.clone();
                                runtime::dispatch(move || del_cb());
                            }),
                    ),
            )
            .child(hairline(cx));

        let edit = |field: &'static str| {
            let ui = ui.clone();
            move |value| ui.invoke_editor_field_edited(section, field.into(), value)
        };

        if service == "LMStudio" {
            let update = edit("ModelName");
            let (field, _) = self.fields.input(
                "lm-model",
                &entry.model_name,
                "",
                false,
                true,
                window,
                cx,
                move |event| {
                    if let Edit::Change(value) = event {
                        update(value);
                    }
                },
            );
            let models = if s.models_loading {
                vec![]
            } else {
                s.model_labels.iter().cloned().collect()
            };
            let models_ui = ui.clone();
            let menu = self.dropdowns.show(
                "lm-model-select",
                &entry.model_name,
                models,
                Some(field),
                "选择或输入 LMStudio 模型标识",
                window,
                cx,
                move |index, _| models_ui.invoke_editor_action(section, 3, index as i32),
            );
            let load_cb = action_ui(2, 0);
            form = form.child(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("模型名称"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().min_w_0().child(menu))
                            .child(
                                Button::new("load-models")
                                    .outline()
                                    .small()
                                    .loading(s.models_loading)
                                    .disabled(s.models_loading)
                                    .label(if s.models_loading {
                                        "加载中…"
                                    } else {
                                        "加载模型"
                                    })
                                    .tooltip("从 LMStudio 获取可用模型")
                                    .on_click(move |_, _, _| {
                                        let load_cb = load_cb.clone();
                                        runtime::dispatch(move || load_cb());
                                    }),
                            ),
                    ),
            );
        } else if llm {
            form = form.child(field(
                &mut self.fields,
                "ModelName",
                "模型名称",
                "服务提供的模型标识，如 deepseek-chat",
                &entry.model_name,
                false,
                None,
                window,
                cx,
                edit("ModelName"),
            ));
        }

        if service != "OpenRouter" {
            let local = matches!(service, "Ollama" | "LMStudio");
            form = form.child(field(
                &mut self.fields,
                "ApiUrl",
                if local {
                    "API 基础地址"
                } else {
                    "API 地址"
                },
                if local {
                    "服务基础地址；无需添加接口路径"
                } else {
                    "完整 API 请求地址"
                },
                &entry.api_url,
                false,
                None,
                window,
                cx,
                edit("ApiUrl"),
            ));
        }
        if service == "OpenAI" {
            form = form.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("使用完整聊天接口地址，例如 https://api.deepseek.com/chat/completions"),
            );
        }
        if service == "Youdao" {
            form = form.child(field(
                &mut self.fields,
                "AppKey",
                "应用 Key",
                "应用 Key",
                &entry.app_key,
                false,
                None,
                window,
                cx,
                edit("AppKey"),
            ));
        }
        if service == "Baidu" {
            form = form.child(field(
                &mut self.fields,
                "AppId",
                "应用 ID",
                "应用 ID",
                &entry.app_id,
                false,
                None,
                window,
                cx,
                edit("AppId"),
            ));
        }
        if matches!(service, "Baidu" | "Youdao") {
            form = form.child(field(
                &mut self.fields,
                "AppSecret",
                "应用密钥",
                "应用密钥",
                &entry.app_secret,
                true,
                None,
                window,
                cx,
                edit("AppSecret"),
            ));
        } else if !matches!(service, "Ollama" | "LMStudio") {
            form = form.child(field(
                &mut self.fields,
                "ApiKey",
                "API 密钥",
                "API 密钥",
                &entry.api_key,
                true,
                None,
                window,
                cx,
                edit("ApiKey"),
            ));
        }
        if llm {
            let update = edit("Temperature");
            let ui = ui.clone();
            form = form.child(
                h_flex()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("温度"),
                    )
                    .child(number_stepper(
                        &mut self.fields,
                        "Temperature",
                        "温度",
                        "控制译文随机性；数值越低越稳定",
                        &entry.temperature_text,
                        None,
                        window,
                        cx,
                        update,
                        move |delta| ui.invoke_editor_action(section, 1, delta),
                    )),
            );
        }
        if service == "MTranServer" {
            form = form.child(field(
                &mut self.fields,
                "SourceLanguage",
                "源语言",
                "待翻译文本的语言代码",
                &entry.source_language,
                false,
                Some(10.),
                window,
                cx,
                edit("SourceLanguage"),
            ));
        }
        form
    }
}

#[allow(clippy::too_many_arguments)]
fn field<T: 'static>(
    fields: &mut Fields,
    name: &'static str,
    title: &'static str,
    hint: &'static str,
    value: &str,
    secret: bool,
    width_rems: Option<f32>,
    window: &mut Window,
    cx: &mut Context<T>,
    callback: impl Fn(String) + 'static,
) -> Div {
    let (input, _) = fields.input(name, value, hint, secret, true, window, cx, move |event| {
        if let Edit::Change(value) = event {
            callback(value);
        }
    });
    let wrapped = div().debug_selector(|| title.to_owned()).child(input);

    if let Some(width) = width_rems {
        h_flex()
            .justify_between()
            .gap_3()
            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
            .child(wrapped.w(rems(width)))
    } else {
        v_flex()
            .w_full()
            .gap_1p5()
            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
            .child(wrapped)
    }
}
