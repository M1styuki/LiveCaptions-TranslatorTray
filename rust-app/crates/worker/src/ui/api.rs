use super::{
    Weak,
    controls::*,
    model::{ApiState, ApiWindow},
    runtime,
};
use gpui::{Context, Div, FontWeight, Render, ScrollHandle, Window, div, point, prelude::*, px};

#[derive(Default)]
pub(super) struct ApiView {
    ui: Weak<ApiWindow>,
    fields: Fields,
    dropdowns: Dropdowns,
    scroll: ScrollHandle,
    nav_scroll: ScrollHandle,
    last_editor: Option<(i32, i32)>,
}

impl ApiWindow {
    pub(crate) const INITIAL_SIZE: (f32, f32) = (950., 600.);

    pub fn show(&self) -> gpui::Result<()> {
        let ui = self.clone();
        self.window()
            .show("API 设置", Self::INITIAL_SIZE, move |window, cx| {
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
                cx.observe_window_appearance(window, |this: &mut ApiView, window, cx| {
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
                ApiView {
                    ui: ui.as_weak(),
                    ..Default::default()
                }
            })
    }

    pub fn hide(&self) -> gpui::Result<()> {
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
        let p = Palette::for_window(s.frame_dark, ui.window().has_mica());
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
            128.
        } else {
            190.
        };
        let sidebar = col()
            .w(px(sidebar_width))
            .h_full()
            .flex_shrink_0()
            .child(scroller(
                "api-navigation",
                &self.nav_scroll,
                col()
                    .p(px(12.))
                    .gap(px(5.))
                    .children(navs.into_iter().enumerate().map(|(index, name)| {
                        let ui = ui.clone();
                        let index = index as i32;
                        nav(name, name, None, index == s.section, p, move || {
                            ui.invoke_section_changed(index)
                        })
                    })),
            ));
        let title = if s.section == 0 {
            "提示词"
        } else if s.service == "OpenAI" {
            "OpenAI 兼容"
        } else {
            &s.service
        };
        let mut content = col()
            .p(px(24.))
            .gap(px(16.))
            .child(heading(title.to_owned()))
            .child(note(if s.section == 0 {
                "自定义翻译要求，适用于模型翻译服务。"
            } else {
                "修改后自动保存；配置此服务不会切换当前翻译服务。"
            }))
            .children((!s.message.is_empty()).then(|| label(s.message.clone())));
        if s.section == 0 {
            content = content.child(self.prompt(&s, &ui, p, window, cx));
        } else if (s.section as usize) <= s.editors.row_count() {
            content = content.child(self.editor(&s, &ui, p, window, cx));
        }
        let title_ui = ui.clone();
        col()
            .size_full()
            .relative()
            .font_family(s.ui_font_family)
            .text_size(px(14.))
            .line_height(px(20.))
            .text_color(p.foreground)
            .child(titlebar(
                p,
                "翻译服务设置",
                None,
                "关闭 API 设置",
                move |action| title_ui.invoke_window_action(action),
            ))
            .child(
                row()
                    .items_start()
                    .bg(p.window_background)
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(sidebar)
                    .child(div().w(px(1.)).h_full().flex_shrink_0().bg(p.pane_divider))
                    .child(col().flex_1().h_full().min_w_0().child(entrance(
                        ("api-page", s.section as usize),
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
        p: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let ui = ui.clone();
        let (editor, focus) = self.fields.input(
            "prompt",
            &s.prompt,
            "",
            true,
            false,
            true,
            window,
            cx,
            move |event| {
                if let Edit::Change(value) = event {
                    ui.invoke_prompt_edited(value);
                }
            },
        );
        let chars_per_line =
            ((f32::from(window.viewport_size().width) - 302.) / 9.).max(20.) as usize;
        let lines = s
            .prompt
            .lines()
            .map(|line| line.chars().count().div_ceil(chars_per_line).max(1))
            .sum::<usize>();
        surface(p)
            .p(px(20.))
            .gap(px(12.))
            .child(label(
                "{0} 代表目标语言，需保留在提示词中。源文本用 🔤 标记包围。",
            ))
            .child(tip(
                field_shell(
                    "prompt-field",
                    p,
                    &focus,
                    window,
                    (20. * lines as f32 + 24.).max(210.),
                )
                .p(px(10.))
                .child(editor),
                "翻译提示词；保留目标语言占位符 {0}",
                p,
            ))
    }

    fn editor(
        &mut self,
        s: &ApiState,
        ui: &ApiWindow,
        p: Palette,
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
        let mut form = surface(p)
            .p(px(20.))
            .gap(px(18.))
            .child(
                row()
                    .gap(px(8.))
                    .child(label("配置").font_weight(FontWeight::SEMIBOLD))
                    .child(icon_button(
                        "previous-config",
                        "original-icons/previous.svg",
                        "上一个配置",
                        p,
                        false,
                        false,
                        entry.config_index > 0,
                        32.,
                        30.,
                        16.,
                        action_ui(0, 0),
                    ))
                    .child(label(format!(
                        "{} / {}",
                        entry.config_index + 1,
                        entry.config_count
                    )))
                    .child(icon_button(
                        "next-config",
                        "original-icons/next.svg",
                        "下一个配置",
                        p,
                        false,
                        false,
                        entry.config_index + 1 < entry.config_count,
                        32.,
                        30.,
                        16.,
                        action_ui(0, 1),
                    ))
                    .child(div().flex_1())
                    .child(action(
                        "new-config",
                        "新建",
                        "新增此服务的配置",
                        p,
                        false,
                        true,
                        action_ui(0, 2),
                    ))
                    .child(action(
                        "delete-config",
                        "删除",
                        "删除当前配置；每项服务至少保留一项",
                        p,
                        false,
                        true,
                        action_ui(0, 3),
                    )),
            )
            .child(divider(p));

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
                p,
                &entry.model_name,
                models,
                Some(field),
                "选择或输入 LMStudio 模型标识",
                30.,
                window,
                move |index, _| models_ui.invoke_editor_action(section, 3, index as i32),
            );
            form = form.child(
                col().gap(px(7.)).child(label("模型名称")).child(
                    row()
                        .gap(px(12.))
                        .child(div().flex_1().min_w_0().child(menu))
                        .child(action(
                            "load-models",
                            if s.models_loading {
                                "加载中…"
                            } else {
                                "加载模型"
                            },
                            "从 LMStudio 获取可用模型",
                            p,
                            false,
                            !s.models_loading,
                            action_ui(2, 0),
                        )),
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
                p,
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
                p,
                window,
                cx,
                edit("ApiUrl"),
            ));
        }
        if service == "OpenAI" {
            form = form.child(note(
                "使用完整聊天接口地址，例如 https://api.deepseek.com/chat/completions",
            ));
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
                p,
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
                p,
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
                p,
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
                p,
                window,
                cx,
                edit("ApiKey"),
            ));
        }
        if llm {
            let update = edit("Temperature");
            let ui = ui.clone();
            form = form.child(
                row()
                    .justify_between()
                    .gap(px(12.))
                    .child(label("温度"))
                    .child(number(
                        &mut self.fields,
                        "Temperature",
                        "温度",
                        "控制译文随机性；数值越低越稳定",
                        &entry.temperature_text,
                        p,
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
                Some(160.),
                p,
                window,
                cx,
                edit("SourceLanguage"),
            ));
        }
        form
    }
}

fn field<T: 'static>(
    fields: &mut Fields,
    name: &'static str,
    title: &'static str,
    hint: &'static str,
    value: &str,
    secret: bool,
    width: Option<f32>,
    p: Palette,
    window: &mut Window,
    cx: &mut Context<T>,
    callback: impl Fn(String) + 'static,
) -> Div {
    let (input, focus) = fields.input(
        name,
        value,
        "",
        false,
        secret,
        true,
        window,
        cx,
        move |event| {
            if let Edit::Change(value) = event {
                callback(value);
            }
        },
    );
    let input = tip(
        field_shell(title, p, &focus, window, 30.)
            .px(px(10.))
            .child(input),
        hint,
        p,
    );
    if let Some(width) = width {
        row()
            .justify_between()
            .gap(px(12.))
            .child(label(title))
            .child(input.w(px(width)))
    } else {
        col()
            .w_full()
            .h(px(54.))
            .gap(px(5.))
            .child(label(title))
            .child(input)
    }
}
