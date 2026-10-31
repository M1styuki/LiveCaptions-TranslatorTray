use super::Controller;
use crate::{ApiEditor, ApiWindow, models};
use captions_core::settings::SERVICES;
use serde_json::json;
use slint::{
    ComponentHandle, ModelRc, VecModel,
    winit_030::{WinitWindowAccessor, winit::window::WindowLevel},
};
use std::{cell::RefCell, rc::Rc};

const EDITORS: [&str; 9] = [
    "Ollama",
    "LMStudio",
    "OpenAI",
    "OpenRouter",
    "DeepL",
    "Youdao",
    "MTranServer",
    "Baidu",
    "LibreTranslate",
];
fn level(window: &slint::Window, topmost: bool) {
    window.with_winit_window(|window| {
        window.set_window_level(if topmost {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        })
    });
}
impl Controller {
    pub fn bind_caption_toggle(owner: &Rc<RefCell<Self>>) {
        let Some(ui) = owner.borrow().ui.upgrade() else {
            return;
        };
        let weak = Rc::downgrade(owner);
        ui.on_captions_changed(move |enabled| {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().captions_changed(enabled);
            }
        });
    }

    pub fn refresh_autostart(&self) {
        if let (Some(registration), Some(ui)) = (&self.autostart, self.ui.upgrade()) {
            match registration.enabled() {
                Ok(enabled) => {
                    ui.set_autostart_enabled(enabled);
                    ui.set_autostart_message("".into());
                }
                Err(error) => {
                    ui.set_autostart_message(format!("无法读取自启动状态：{error}").into())
                }
            }
        }
    }

    pub fn autostart_changed(&self, enabled: bool) {
        if self.exiting {
            return;
        }
        if let Some(ui) = self.ui.upgrade() {
            let Some(registration) = &self.autostart else {
                ui.set_autostart_enabled(false);
                return;
            };
            if let Err(error) = registration.set_enabled(enabled) {
                ui.set_autostart_enabled(!enabled);
                self.refresh_autostart();
                ui.set_autostart_message(format!("自启动设置未保存：{error}").into());
            } else {
                self.refresh_autostart();
            }
        }
    }

    pub fn setting_edited(&mut self, field: &str, text: &str) {
        if self.exiting {
            return;
        }
        let Some(mut settings) = self.settings.clone() else {
            return;
        };
        match field {
            "NumContexts" => {
                let Ok(value) = text.parse::<u8>() else {
                    return;
                };
                settings.set_num_contexts(value);
            }
            "DisplaySentences" => {
                let Ok(value) = text.parse::<u8>() else {
                    return;
                };
                settings.set_display_sentences(value);
            }
            _ => {
                let value = match field {
                    "ApiName" if SERVICES.contains(&text) => json!(text),
                    "TargetLanguage" if !text.trim().is_empty() => json!(text),
                    "ContextAware" | "LatencyShow" | "Topmost" | "CaptionLogEnabled" => {
                        let Ok(value) = text.parse::<bool>() else {
                            return;
                        };
                        json!(value)
                    }
                    "MaxSyncInterval" => {
                        let Ok(value) = text.parse::<u8>() else {
                            return;
                        };
                        json!(value.min(10))
                    }
                    _ => return,
                };
                let Ok((edited, _)) = settings.edited(self.screen, |root| {
                    if matches!(field, "LatencyShow" | "Topmost" | "CaptionLogEnabled") {
                        root["MainWindow"][field] = value;
                    } else {
                        root[field] = value;
                    }
                }) else {
                    return;
                };
                settings = edited;
            }
        }
        self.save_settings(settings);
        self.flow_updates();
    }
    pub fn captions_visibility_changed(&mut self, hidden: bool) {
        if self.exiting {
            return;
        }
        self.hidden = hidden;
        if let Some(engine) = &self.active {
            engine.set_hidden(hidden);
        }
        self.sync_settings_ui();
        self.windows_changed();
    }
    pub(super) fn sync_settings_ui(&self) {
        let Some(settings) = &self.settings else {
            return;
        };
        if let Some(ui) = self.ui.upgrade() {
            let value = settings.json();
            ui.set_selected_service(settings.api_name().into());
            ui.set_selected_language(settings.target_language().into());
            ui.set_settings_prompt(value["Prompt"].as_str().unwrap().into());
            ui.set_max_sync_interval(value["MaxSyncInterval"].as_i64().unwrap() as i32);
            ui.set_num_contexts(value["NumContexts"].as_i64().unwrap() as i32);
            ui.set_display_sentences(value["DisplaySentences"].as_i64().unwrap() as i32);
            ui.set_context_aware(value["ContextAware"].as_bool().unwrap());
            ui.set_original_font_size(
                value["MainWindow"]["OriginalFontSize"].as_i64().unwrap() as i32
            );
            ui.set_translated_font_size(
                value["MainWindow"]["TranslatedFontSize"].as_i64().unwrap() as i32,
            );
            ui.set_latency_show(value["MainWindow"]["LatencyShow"].as_bool().unwrap());
            ui.set_caption_log_enabled(value["MainWindow"]["CaptionLogEnabled"].as_bool().unwrap());
            ui.set_topmost(value["MainWindow"]["Topmost"].as_bool().unwrap());
            ui.set_live_captions_hidden(self.hidden);
            level(ui.window(), ui.get_topmost());
        }
        self.sync_main_geometry();
    }
    pub fn open_api(owner: &Rc<RefCell<Self>>) {
        if owner.borrow().exiting || owner.borrow().settings.is_none() {
            return;
        }
        if let Some(api) = &owner.borrow().api_window {
            let _ = api.show();
            return;
        }
        let api = match ApiWindow::new() {
            Ok(api) => api,
            Err(_) => {
                owner.borrow().message("API 设置窗口无法打开。");
                return;
            }
        };
        api.set_ui_font_family(windows_platform::desktop::ui_font_family().into());
        api.global::<crate::ModelLookup>()
            .on_index_of(crate::model_index);
        let theme_window = api.as_weak();
        api.on_frame_theme_changed(move || {
            if let Some(api) = theme_window.upgrade() {
                super::main_geometry::style_api(&api);
            }
        });
        let frame_window = api.as_weak();
        let mut frame_applied = false;
        api.window().on_winit_window_event(move |_, event| {
            if matches!(
                event,
                slint::winit_030::winit::event::WindowEvent::RedrawRequested
            ) && !frame_applied
            {
                frame_applied = true;
                if let Some(api) = frame_window.upgrade() {
                    super::main_geometry::style_api(&api);
                    super::main_geometry::center_api(&api);
                }
            }
            slint::winit_030::EventResult::Propagate
        });
        let weak = Rc::downgrade(owner);
        let api_weak = api.as_weak();
        api.on_window_action(move |action| {
            use slint::winit_030::winit::window::ResizeDirection;
            if action == 1 {
                if let Some(owner) = weak.upgrade() {
                    owner.borrow_mut().api_closed();
                }
            } else if let Some(api) = api_weak.upgrade() {
                api.window().with_winit_window(|window| match action {
                    0 => window.set_minimized(true),
                    2 => {
                        let _ = window.drag_window();
                    }
                    3 => {
                        let _ = window.drag_resize_window(ResizeDirection::SouthEast);
                    }
                    _ => {}
                });
            }
        });
        let weak = Rc::downgrade(owner);
        api.on_editor_field_edited(move |section, field, value| {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().edit_api_section(section, |owner| {
                    owner.api_field(field.as_str(), value.as_str());
                });
            }
        });
        let weak = Rc::downgrade(owner);
        api.on_editor_action(move |section, kind, value| {
            if let Some(owner) = weak.upgrade() {
                owner
                    .borrow_mut()
                    .edit_api_section(section, |owner| match kind {
                        0 => owner.api_config_action(value),
                        1 => owner.api_temperature_step(value),
                        2 => owner.request_models(),
                        3 => {
                            if let Some(model) = owner.models.get(value as usize).cloned() {
                                owner.api_field("ModelName", &model.id);
                            }
                        }
                        _ => {}
                    });
            }
        });
        let weak = Rc::downgrade(owner);
        api.on_section_changed(move |section| {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().api_select(section);
            }
        });
        let weak = Rc::downgrade(owner);
        api.on_config_action(move |action| {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().api_config_action(action);
            }
        });
        let weak = Rc::downgrade(owner);
        api.on_field_edited(move |field, value| {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().api_field(field.as_str(), value.as_str());
            }
        });
        let weak = Rc::downgrade(owner);
        api.on_temperature_stepped(move |delta| {
            if let Some(owner) = weak.upgrade() {
                let mut owner = owner.borrow_mut();
                owner.api_temperature_step(delta);
            }
        });
        let weak = Rc::downgrade(owner);
        api.on_prompt_edited(move |value| {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().api_prompt(value.as_str());
            }
        });
        let weak = Rc::downgrade(owner);
        api.on_models_requested(move || {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().request_models();
            }
        });
        let weak = Rc::downgrade(owner);
        api.on_model_selected(move |index| {
            if let Some(owner) = weak.upgrade() {
                let mut owner = owner.borrow_mut();
                if let Some(model) = owner.models.get(index.max(0) as usize).cloned() {
                    owner.api_field("ModelName", &model.id);
                }
            }
        });
        let weak = Rc::downgrade(owner);
        api.window().on_close_requested(move || {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().api_closed();
            }
            slint::CloseRequestResponse::HideWindow
        });
        let mut owner = owner.borrow_mut();
        owner.api_section = 0;
        owner.api_window = Some(api);
        owner.refresh_api();
        let api = owner.api_window.as_ref().unwrap();
        if api.show().is_err() {
            owner.api_window.take();
            owner.message("API 设置窗口无法显示。");
            return;
        }
        level(api.window(), true);
        super::main_geometry::style_api(api);
        owner.recount_windows();
        owner.retire_when_idle = false;
        owner.windows_changed();
    }
    fn editor_service(&self) -> Option<&'static str> {
        self.api_section
            .checked_sub(1)
            .and_then(|index| EDITORS.get(index).copied())
    }
    fn edit_api_section(&mut self, section: i32, edit: impl FnOnce(&mut Self)) {
        if !(1..=9).contains(&section) || self.exiting {
            return;
        }
        // Editing another visible card must neither switch the navigation nor
        // move the scroll position. The existing config/field handlers still
        // own validation, persistence and request generation cancellation.
        let selected = self.api_section;
        self.api_section = section as usize;
        edit(self);
        self.api_section = selected;
        self.refresh_api();
    }
    pub(super) fn api_select(&mut self, section: i32) {
        if !(0..=9).contains(&section) || self.exiting {
            return;
        }
        self.cancel_models();
        self.api_section = section as usize;
        if let Some(api) = &self.api_window {
            api.set_section(section);
            api.set_message("".into());
        }
        self.refresh_api();
    }
    fn refresh_api(&self) {
        let (Some(settings), Some(api)) = (&self.settings, &self.api_window) else {
            return;
        };
        api.set_prompt(settings.json()["Prompt"].as_str().unwrap().into());
        let editors = EDITORS
            .iter()
            .map(|service| {
                let index = settings.json()["ConfigIndices"][service].as_u64().unwrap() as usize;
                let configs = settings.configs(service).unwrap();
                let config = &configs[index];
                let text = |name: &str| config[name].as_str().unwrap_or("").into();
                ApiEditor {
                    service: (*service).into(),
                    config_index: index as i32,
                    config_count: configs.len() as i32,
                    model_name: text("ModelName"),
                    api_url: text("ApiUrl"),
                    api_key: text("ApiKey"),
                    app_id: text("AppId"),
                    app_key: text("AppKey"),
                    app_secret: text("AppSecret"),
                    source_language: text("SourceLanguage"),
                    temperature_text: config["Temperature"].to_string().into(),
                    keep_alive: config["Keep_alive"]
                        .as_i64()
                        .map_or_else(String::new, |value| value.to_string())
                        .into(),
                }
            })
            .collect::<Vec<_>>();
        if let Some(editor) = self
            .api_section
            .checked_sub(1)
            .and_then(|index| editors.get(index))
        {
            // Text editors share this explicitly refreshed property. A one-way
            // binding to editors[index] would be detached by the first edit.
            api.set_active_editor(editor.clone());
        }
        api.set_editors(ModelRc::new(VecModel::from(editors)));
        let Some(service) = self.editor_service() else {
            return;
        };
        let index = settings.json()["ConfigIndices"][service].as_u64().unwrap() as usize;
        let configs = settings.configs(service).unwrap();
        let config = &configs[index];
        let text = |name: &str| config[name].as_str().unwrap_or("").into();
        api.set_service(service.into());
        api.set_config_index(index as i32);
        api.set_config_count(configs.len() as i32);
        api.set_model_name(text("ModelName"));
        api.set_api_url(text("ApiUrl"));
        api.set_api_key(text("ApiKey"));
        api.set_app_id(text("AppId"));
        api.set_app_key(text("AppKey"));
        api.set_app_secret(text("AppSecret"));
        api.set_source_language(text("SourceLanguage"));
        api.set_temperature_text(config["Temperature"].to_string().into());
        api.set_keep_alive(
            config["Keep_alive"]
                .as_i64()
                .map_or_else(String::new, |value| value.to_string())
                .into(),
        );
    }
    pub(super) fn api_field(&mut self, field: &str, text: &str) {
        if self.exiting {
            return;
        }
        let (Some(service), Some(settings)) = (self.editor_service(), self.settings.clone()) else {
            return;
        };
        let index = settings.json()["ConfigIndices"][service].as_u64().unwrap() as usize;
        if settings.json()["Configs"][service][index]
            .get(field)
            .is_none()
        {
            return;
        }
        let value = match field {
            "Temperature" => {
                let Ok(value) = text.parse::<f64>() else {
                    if let Some(api) = &self.api_window {
                        api.set_message("温度必须是0至2之间的数字，仍保留上一次有效值。".into());
                    }
                    return;
                };
                if !value.is_finite() {
                    if let Some(api) = &self.api_window {
                        api.set_message("温度必须是0至2之间的数字，仍保留上一次有效值。".into());
                    }
                    return;
                }
                json!(value.clamp(0.0, 2.0))
            }
            "Keep_alive" => {
                let Ok(value) = text.parse::<i64>() else {
                    if let Some(api) = &self.api_window {
                        api.set_message("保留时间必须是非负整数，仍保留上一次有效值。".into());
                    }
                    return;
                };
                if value < 0 {
                    if let Some(api) = &self.api_window {
                        api.set_message("保留时间必须是非负整数，仍保留上一次有效值。".into());
                    }
                    return;
                }
                json!(value)
            }
            "ApiUrl" | "ApiKey" | "AppKey" | "AppId" | "AppSecret" | "SourceLanguage"
            | "ModelName" => json!(text),
            _ => return,
        };
        if field == "ApiUrl" {
            self.cancel_models();
        }
        if matches!(field, "Temperature" | "Keep_alive")
            && let Some(api) = &self.api_window
        {
            let message = api.get_message();
            if message.starts_with("温度必须") || message.starts_with("保留时间必须") {
                api.set_message("".into());
            }
        }
        if let Ok((settings, _)) = settings.edited(self.screen, |root| {
            root["Configs"][service][index][field] = value
        }) {
            self.save_settings(settings);
        }
        if field == "ModelName"
            && let Some(api) = &self.api_window
        {
            api.set_model_name(text.into());
        }
        if field == "Temperature"
            && let Some(api) = &self.api_window
        {
            let value = self.settings.as_ref().unwrap().json()["Configs"][service][index][field]
                .as_f64()
                .unwrap();
            if text.parse::<f64>().is_ok_and(|raw| raw != value) {
                api.set_temperature_text(value.to_string().into());
            }
        }
        self.refresh_api();
        self.flow_updates();
    }
    fn api_temperature_step(&mut self, delta: i32) {
        if self.exiting || !matches!(delta, -1 | 1) {
            return;
        }
        let (Some(service), Some(settings)) = (self.editor_service(), &self.settings) else {
            return;
        };
        let index = settings.json()["ConfigIndices"][service].as_u64().unwrap() as usize;
        let Some(current) = settings.json()["Configs"][service][index]["Temperature"].as_f64()
        else {
            return;
        };
        let value = (current + f64::from(delta) * 0.1).clamp(0.0, 2.0);
        let text = if (current * 10.0).fract() == 0.0 {
            format!("{value:.1}")
        } else {
            value.to_string()
        };
        self.api_field("Temperature", &text);
        if let Some(api) = &self.api_window {
            api.set_temperature_text(text.into());
        }
    }
    fn api_prompt(&mut self, text: &str) {
        if self.exiting {
            return;
        }
        let Some(settings) = &self.settings else {
            return;
        };
        match settings.with_prompt(text) {
            Ok(settings) => {
                self.save_settings(settings);
                if let Some(api) = &self.api_window {
                    api.set_message("".into());
                }
            }
            Err(error) => {
                if let Some(api) = &self.api_window {
                    api.set_message(error.to_string().into());
                }
            }
        }
    }
    pub(super) fn api_config_action(&mut self, action: i32) {
        if self.exiting {
            return;
        }
        let (Some(service), Some(mut settings)) = (self.editor_service(), self.settings.clone())
        else {
            return;
        };
        let index = settings.json()["ConfigIndices"][service].as_u64().unwrap() as usize;
        let count = settings.configs(service).unwrap().len();
        match action {
            0 | 1 => {
                let next = if action == 0 {
                    index.saturating_sub(1)
                } else {
                    (index + 1).min(count - 1)
                };
                if next == index {
                    return;
                }
                settings = settings
                    .edited(self.screen, |root| {
                        root["ConfigIndices"][service] = json!(next)
                    })
                    .unwrap()
                    .0;
            }
            2 => {
                settings.add_config(service);
            }
            3 => {
                if !settings.remove_config(service, index) {
                    if let Some(api) = &self.api_window {
                        api.set_message("每个接口至少保留一项配置。".into());
                    }
                    return;
                }
            }
            _ => return,
        }
        self.cancel_models();
        self.save_settings(settings);
        self.refresh_api();
        self.flow_updates();
    }
    pub(super) fn cancel_models(&mut self) {
        self.model_generation += 1;
        self.model_job.take();
        self.model_slot.lock().unwrap().take();
        self.models.clear();
        if let Some(api) = &self.api_window {
            api.set_models_loading(false);
            api.set_model_labels(ModelRc::default());
        }
    }
    fn request_models(&mut self) {
        if self.exiting || self.editor_service() != Some("LMStudio") {
            return;
        }
        let Some(settings) = &self.settings else {
            return;
        };
        let index = settings.json()["ConfigIndices"]["LMStudio"]
            .as_u64()
            .unwrap() as usize;
        let url = settings.json()["Configs"]["LMStudio"][index]["ApiUrl"]
            .as_str()
            .unwrap()
            .to_owned();
        self.cancel_models();
        match models::Job::start(
            self.model_generation,
            url,
            self.model_slot.clone(),
            self.ui.clone(),
        ) {
            Ok(job) => {
                self.model_job = Some(job);
                if let Some(api) = &self.api_window {
                    api.set_models_loading(true);
                    api.set_message("正在加载模型…".into());
                }
            }
            Err(_) => {
                if let Some(api) = &self.api_window {
                    api.set_message("模型列表请求无法启动，现有模型名称已保留。".into());
                }
            }
        }
    }
    pub(super) fn model_updates(&mut self) {
        let event = self.model_slot.lock().unwrap().take();
        let Some(event) = event else {
            return;
        };
        if event.generation != self.model_generation {
            return;
        }
        self.model_job.take();
        if let Some(api) = &self.api_window {
            api.set_models_loading(false);
            match event.result {
                Ok(models) => {
                    let labels = models
                        .iter()
                        .map(|model| model.display_name.clone().into())
                        .collect::<Vec<_>>();
                    self.models = models;
                    api.set_model_labels(ModelRc::new(VecModel::from(labels)));
                    api.set_message(
                        if self.models.is_empty() {
                            "没有可选的语言模型，现有名称已保留。"
                        } else {
                            "模型已加载，选择项目将写入模型 ID。"
                        }
                        .into(),
                    );
                }
                Err(error) => api.set_message(format!("{error}；现有模型名称已保留。").into()),
            }
        }
    }
    pub(super) fn api_closed(&mut self) {
        self.cancel_models();
        if let Some(api) = self.api_window.take() {
            let _ = api.hide();
        }
        self.flush_settings();
        self.recount_windows();
        self.retire_when_idle = true;
        self.windows_changed();
        self.maybe_exit();
    }
}
