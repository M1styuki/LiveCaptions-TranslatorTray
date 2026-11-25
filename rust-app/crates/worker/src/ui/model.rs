use super::native::WindowState;
use super::{Component, Rows, component};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HistoryItem {
    pub stamp: String,
    pub stamp_full: String,
    pub source: String,
    pub translation: String,
    pub service: String,
    pub language: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ApiEditor {
    pub service: String,
    pub config_index: i32,
    pub config_count: i32,
    pub model_name: String,
    pub api_url: String,
    pub api_key: String,
    pub app_id: String,
    pub app_key: String,
    pub app_secret: String,
    pub source_language: String,
    pub keep_alive: String,
    pub temperature_text: String,
}

component! {
    MainWindow, MainState, MainActions, MainView;
    properties {
        available_window_height: f32 = 10000.0,
        ui_font_family: String = "Segoe UI".into(),
        native_material: bool = false,
        frame_dark: bool = Default::default(),
        page: i32 = 1,
        log_only: bool = false,
        overlay_visible: bool = false,
        captions_enabled: bool = false,
        engine_busy: bool = false,
        autostart_enabled: bool = false,
        autostart_message: String = Default::default(),
        topmost: bool = false,
        source_caption: String = Default::default(),
        translated_caption: String = Default::default(),
        caption_notice: String = Default::default(),
        original_font_size: i32 = 15,
        translated_font_size: i32 = 18,
        copy_message: String = Default::default(),
        data_state: i32 = 0,
        data_message: String = "正在载入设置与历史…".into(),
        retry_save: bool = false,
        file_busy: bool = false,
        export_busy: bool = false,
        export_message: String = Default::default(),
        delete_confirm: bool = false,
        delete_busy: bool = false,
        delete_message: String = Default::default(),
        recovery_location: String = Default::default(),
        selected_service: String = "Google".into(),
        selected_language: String = "zh-CN".into(),
        settings_prompt: String = Default::default(),
        max_sync_interval: i32 = 3,
        num_contexts: i32 = 2,
        display_sentences: i32 = 1,
        context_aware: bool = false,
        latency_show: bool = false,
        caption_log_enabled: bool = false,
        caption_cards: Rows<HistoryItem> = Default::default(),
        latency_ms: String = Default::default(),
        live_captions_hidden: bool = true,
        history_query: String = Default::default(),
        history_size: i32 = 30,
        history_page: i32 = 1,
        history_pages: i32 = 1,
        history_total: String = "0".into(),
        history_rows: Rows<HistoryItem> = Default::default(),
    }
    actions {
        frame_theme_changed();
        data_action(arg0: i32, arg1: String);
        file_requested(arg0: i32);
        caption_copy_requested(arg0: String);
        caption_font_adjusted(arg0: i32, arg1: i32);
        delete_action(arg0: i32);
        prompt_changed(arg0: String);
        history_requested(arg0: i32);
        history_action(arg0: i32, arg1: i32);
        host_wakeup();
        page_changed(arg0: i32);
        pause_changed(arg0: bool);
        captions_changed(arg0: bool);
        autostart_changed(arg0: bool);
        topmost_changed(arg0: bool);
        setting_edited(arg0: String, arg1: String);
        api_requested();
        captions_visibility_changed(arg0: bool);
        window_action(arg0: i32);
    }
}

component! {
    ApiWindow, ApiState, ApiActions, ApiView;
    properties {
        native_material: bool = false,
        frame_dark: bool = Default::default(),
        ui_font_family: String = "Segoe UI".into(),
        section: i32 = 0,
        service: String = Default::default(),
        config_index: i32 = Default::default(),
        config_count: i32 = 1,
        prompt: String = Default::default(),
        model_name: String = Default::default(),
        api_url: String = Default::default(),
        api_key: String = Default::default(),
        app_id: String = Default::default(),
        app_key: String = Default::default(),
        app_secret: String = Default::default(),
        source_language: String = Default::default(),
        keep_alive: String = Default::default(),
        temperature_text: String = "1.0".into(),
        message: String = Default::default(),
        models_loading: bool = Default::default(),
        model_labels: Rows<String> = Default::default(),
        editors: Rows<ApiEditor> = Default::default(),
        active_editor: ApiEditor = Default::default(),
    }
    actions {
        frame_theme_changed();
        section_changed(arg0: i32);
        config_action(arg0: i32);
        field_edited(arg0: String, arg1: String);
        prompt_edited(arg0: String);
        temperature_stepped(arg0: i32);
        models_requested();
        model_selected(arg0: i32);
        window_action(arg0: i32);
        editor_field_edited(arg0: i32, arg1: String, arg2: String);
        editor_action(arg0: i32, arg1: i32, arg2: i32);
    }
}
