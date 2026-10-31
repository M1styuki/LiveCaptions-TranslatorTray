use crate::{
    MainWindow,
    data::Data,
    engine::{Engine, Mailbox, Status},
    flow::{self, Action, Flow},
    shutdown::ShutdownState,
    ui_bridge::Bridge,
};
use captions_core::settings::{Screen, Settings, WindowKind};
use protocol::{Empty, EngineState, Envelope, Message};
use slint::ComponentHandle;
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex},
};
use storage::actor::{Command as DataCommand, Event as DataEvent, Operation, Recovery};
use windows_platform::overlay::{self, Overlay};
mod caption_ui;
mod delete_ui;
mod file_ui;
mod history_ui;
mod main_geometry;
mod settings_ui;

pub struct Controller {
    pub bridge: Bridge,
    ui: slint::Weak<MainWindow>,
    engine: EngineState,
    pub(super) autostart: Option<windows_platform::autostart::Registration>,
    hidden: bool,
    shutdown: ShutdownState,
    replies: VecDeque<Envelope>,
    exiting: bool,
    active: Option<Engine>,
    mailbox: std::sync::Arc<Mailbox>,
    session: u64,
    overlay: Option<Overlay>,
    overlay_settings: overlay::Settings,
    overlay_content: overlay::Content,
    overlay_generation: u64,
    overlay_events: Arc<Mutex<VecDeque<(u64, overlay::Event)>>>,
    data: Data,
    screen: Screen,
    settings: Option<Settings>,
    settings_revision: u64,
    saved_settings_revision: u64,
    pending_start: bool,
    #[cfg(test)]
    show_failure: bool,
    data_request: u64,
    history_request: u64,
    history: history_ui::History,
    files: file_ui::Files,
    caption: caption_ui::Interaction,
    deletion: delete_ui::Deletion,
    geometry: main_geometry::Geometry,
    main_open: bool,
    pending_settings: Option<(u64, Settings)>,
    flow: Flow,
    shutdown_submitted: bool,
    retire_when_idle: bool,
    api_window: Option<crate::ApiWindow>,
    api_section: usize,
    model_job: Option<crate::models::Job>,
    model_slot: crate::models::Slot,
    model_generation: u64,
    models: Vec<providers::Model>,
}

impl Controller {
    #[cfg(test)]
    pub fn test_api(&self) -> crate::ApiWindow {
        self.api_window.as_ref().unwrap().clone_strong()
    }
    #[cfg(test)]
    pub fn test_close_api(&mut self) {
        self.api_closed();
    }
    #[cfg(test)]
    pub fn test_settings(&self) -> Settings {
        self.settings.clone().unwrap()
    }
    #[cfg(test)]
    pub fn test_windows(&self) -> u32 {
        self.shutdown.visible_windows
    }
    #[cfg(test)]
    pub fn test_show_settings(&mut self) {
        self.show(1, self.hidden).unwrap();
    }
    #[cfg(test)]
    pub fn test_data_stopped(&self) -> bool {
        self.data.is_stopped()
    }
    #[cfg(test)]
    pub fn test_wait_for_first_use(&mut self) {
        self.start(true).unwrap();
    }
    #[cfg(test)]
    pub fn test_start_network_fixture(&mut self, provider: providers::Translator, service: &str) {
        let settings = self
            .settings
            .as_ref()
            .unwrap()
            .edited(self.screen, |value| {
                value["ContextAware"] = true.into();
                value["ApiName"] = service.into();
                if providers::is_llm(service) {
                    let index = value["ConfigIndices"][service].as_u64().unwrap() as usize;
                    value["Configs"][service][index]["ModelName"] = "fixture-model".into();
                }
            })
            .unwrap()
            .0;
        self.save_settings(settings.clone());
        self.session += 1;
        self.flow.begin(settings, false);
        self.active = Some(Engine::local_network_fixture(
            self.session,
            provider,
            self.mailbox.clone(),
        ));
        self.engine = EngineState::Running;
        self.engine_changed();
        self.retire_when_idle = false;
    }
    #[cfg(test)]
    pub fn test_capture(&mut self, text: &str) {
        self.test_queue_capture(text);
        self.engine_updates();
        self.flow_updates();
    }
    #[cfg(test)]
    pub fn test_queue_capture(&mut self, text: &str) {
        let revision = &self.active.as_ref().unwrap().revision;
        let changes = revision.load(std::sync::atomic::Ordering::Acquire) + 1;
        self.mailbox.capture(
            self.session,
            revision,
            windows_platform::captions::CaptionSample {
                text: text.into(),
                changes,
                unchanged: 0,
            },
        );
    }
    #[cfg(test)]
    pub fn test_stop_engine(&mut self) {
        self.stop();
    }
    #[cfg(test)]
    pub fn test_caption_fault(&mut self, message: &str) {
        let session = self.session;
        self.mailbox.test_fault(session, message.into());
        self.engine_updates();
        self.flow_updates();
    }
    #[cfg(test)]
    pub fn test_queue_style(&mut self, style: overlay::Settings) {
        self.overlay_events
            .lock()
            .unwrap()
            .push_back((self.overlay_generation, overlay::Event::StyleChanged(style)));
    }
    #[cfg(test)]
    pub fn test_queue_placement(
        &mut self,
        value: captions_core::geometry::Placement,
        manual: bool,
        stale: bool,
    ) {
        let generation = self.overlay_generation.wrapping_sub(u64::from(stale));
        self.overlay_events
            .lock()
            .unwrap()
            .push_back((generation, overlay::Event::PlacementChanged(value, manual)));
    }
    #[cfg(test)]
    pub fn test_close_overlay(&mut self) {
        self.close_overlay();
    }
    #[cfg(test)]
    pub fn test_overlay_content(&self) -> overlay::Content {
        self.overlay_content.clone()
    }
    #[cfg(test)]
    pub fn test_fail_show(&mut self) {
        self.show_failure = true;
    }
    #[cfg(test)]
    pub fn test_try_pending_start(&mut self) -> io::Result<()> {
        self.start(true)
    }
    #[cfg(test)]
    pub fn test_pending_start_and_engine(&self) -> (bool, EngineState) {
        (self.pending_start, self.engine)
    }
    pub fn new(bridge: Bridge, ui: slint::Weak<MainWindow>, data: Data, screen: Screen) -> Self {
        if let Some(window) = ui.upgrade() {
            window
                .global::<crate::ModelLookup>()
                .on_index_of(crate::model_index);
            window.set_ui_font_family(windows_platform::desktop::ui_font_family().into());
        }
        let mailbox = Mailbox::new(ui.clone());
        let flow = Flow::new(data.instance.clone());
        Self {
            bridge,
            ui,
            engine: EngineState::Disabled,
            autostart: None,
            hidden: true,
            shutdown: ShutdownState::default(),
            replies: VecDeque::new(),
            exiting: false,
            active: None,
            mailbox,
            session: 0,
            overlay: None,
            overlay_settings: overlay::Settings::default(),
            overlay_content: overlay::Content::default(),
            overlay_generation: 0,
            overlay_events: Arc::default(),
            data,
            screen,
            settings: None,
            settings_revision: 0,
            saved_settings_revision: 0,
            pending_start: false,
            #[cfg(test)]
            show_failure: false,
            data_request: 0,
            history_request: 0,
            history: history_ui::History::default(),
            files: file_ui::Files::default(),
            caption: caption_ui::Interaction::default(),
            deletion: delete_ui::Deletion::default(),
            geometry: main_geometry::Geometry::default(),
            main_open: false,
            pending_settings: None,
            flow,
            shutdown_submitted: false,
            retire_when_idle: false,
            api_window: None,
            api_section: 0,
            model_job: None,
            model_slot: Arc::default(),
            model_generation: 0,
            models: Vec::new(),
        }
    }

    pub fn wakeup(&mut self) {
        self.data_updates();
        self.delete_updates();
        self.file_updates();
        self.model_updates();
        for command in self.bridge.take_commands() {
            if self.exiting {
                let message = if matches!(command.message, Message::Exit(_)) {
                    Message::Ack(Empty {})
                } else {
                    Message::Error {
                        code: "WorkerExiting".into(),
                        message: "正在保存并退出，请稍后再操作。".into(),
                    }
                };
                let _ = self.bridge.send(Envelope::new(command.request_id, message));
                continue;
            }
            if let Some(reply) = self
                .replies
                .iter()
                .find(|reply| reply.request_id == command.request_id)
            {
                if self.bridge.send(reply.clone()).is_err() {
                    self.exit();
                }
                continue;
            }
            let result = match command.message {
                Message::ShowSettings(preference) => self.show(1, preference.live_captions_hidden),
                Message::ShowMain(preference) => self.show(0, preference.live_captions_hidden),
                Message::StartEngine(preference) => self.start(preference.live_captions_hidden),
                Message::StopEngine(_) => {
                    self.stop();
                    Ok(())
                }
                Message::Exit(_) => {
                    let reply = Envelope::new(command.request_id, Message::Ack(Empty {}));
                    let _ = self.bridge.send(reply);
                    self.exit();
                    continue;
                }
                _ => Err(io::Error::other("宿主发送了未知命令。")),
            };
            let response = Envelope::new(
                command.request_id,
                match result {
                    Ok(()) => Message::Ack(Empty {}),
                    Err(error) => Message::Error {
                        code: "CommandFailed".into(),
                        message: error.to_string(),
                    },
                },
            );
            if self.replies.len() == 128 {
                self.replies.pop_front();
            }
            self.replies.push_back(response.clone());
            if self.bridge.send(response).is_err() {
                self.exit();
            }
            self.maybe_exit();
        }
        if !self.exiting {
            self.overlay_updates();
            self.engine_updates();
        }
        self.flow_updates();
        self.advance_exit();
    }

    fn start(&mut self, hidden: bool) -> io::Result<()> {
        if self.active.is_some() {
            return Ok(());
        }
        self.hidden = hidden;
        if self.settings.is_none() {
            self.pending_start = true;
            self.engine = EngineState::Starting;
            self.engine_changed();
            return self.pending_start_prompt();
        }
        let settings = self.settings.as_ref().unwrap();
        let settings = settings.clone();
        self.session += 1;
        self.engine = EngineState::Starting;
        self.engine_changed();
        let log_only = self.ui.upgrade().is_some_and(|ui| ui.get_log_only());
        match Engine::start(self.session, hidden, self.mailbox.clone()) {
            Ok(engine) => {
                self.flow.begin(settings, log_only);
                self.active = Some(engine);
                Ok(())
            }
            Err(error) => {
                self.engine = EngineState::Disabled;
                self.engine_changed();
                Err(error)
            }
        }
    }

    fn pending_start_prompt(&mut self) -> io::Result<()> {
        // Loading or creating defaults needs no decision. The host can send Start as
        // soon as its pipe connects, before the storage Ready event arrives.
        let needs_decision = self
            .ui
            .upgrade()
            .is_some_and(|ui| matches!(ui.get_data_state(), 3..=6));
        if self.pending_start
            && !self.main_open
            && needs_decision
            && !self.exiting
            && let Err(error) = self.show(1, self.hidden)
        {
            self.pending_start = false;
            self.engine = EngineState::Disabled;
            self.engine_changed();
            self.retire_when_idle = true;
            return Err(error);
        }
        Ok(())
    }

    fn stop(&mut self) {
        self.retire_when_idle = true;
        self.pending_start = false;
        // Invalidate the UI/controller session before waiting for any producer.
        self.session += 1;
        self.flow.stop();
        self.close_overlay();
        self.overlay_content = overlay::Content::default();
        if self.active.is_some() {
            self.engine = EngineState::Stopping;
            self.engine_changed();
        }
        if let Some(mut engine) = self.active.take() {
            engine.stop();
        }
        let _ = self.mailbox.take();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_source_caption("".into());
            ui.set_translated_caption("".into());
            ui.set_caption_notice("".into());
            ui.set_caption_cards(slint::ModelRc::default());
            ui.set_latency_ms("".into());
        }
        self.shutdown.overlay_open = false;
        self.engine = EngineState::Disabled;
        self.engine_changed();
        self.flush_settings();
    }

    pub fn pause_changed(&mut self, value: bool) {
        if self.exiting {
            return;
        }
        if let Some(settings) = &self.settings {
            let settings = settings.clone();
            self.capture_cutoff();
            self.flow.reconfigure(settings, value);
            if let Some(engine) = &self.active {
                engine.cancel_all();
            }
            self.flow_updates();
        }
    }

    fn engine_updates(&mut self) {
        let updates = self.mailbox.take();
        if let Some((session, status)) = updates.status
            && session == self.session
        {
            match status {
                Status::Ready => {
                    self.engine = EngineState::Running;
                    self.engine_changed();
                    self.open_overlay();
                }
                Status::Fault(message) => {
                    let notice = format!("系统字幕已停止：{message}");
                    self.engine = EngineState::Faulted;
                    self.engine_changed();
                    let _ = self.bridge.send(Envelope::new(
                        0,
                        Message::Error {
                            code: "CaptionsUnavailable".into(),
                            message,
                        },
                    ));
                    self.stop();
                    if let Some(ui) = self.ui.upgrade() {
                        ui.set_caption_notice(notice.into());
                    }
                    self.maybe_exit();
                    return;
                }
            }
        }
        if self.flow.can_capture()
            && let Some((session, revision, sample)) = self.mailbox.take_source()
            && session == self.session
        {
            self.flow.capture(sample, revision);
        }
        for completion in updates.completions {
            if let Some(engine) = &mut self.active {
                engine.finished(&completion.task.key);
            }
            self.flow.completed(completion);
        }
    }

    /// Rebase user actions on the caption observed at the action cutoff, not
    /// on a possibly older item waiting for the controller/storage fence.
    fn capture_cutoff(&mut self) {
        if let Some((session, revision, sample)) = self.mailbox.latest_source()
            && session == self.session
        {
            self.flow.capture(sample, revision);
        }
    }

    pub fn captions_changed(&mut self, enabled: bool) {
        if self.exiting {
            return;
        }
        if enabled {
            if let Err(error) = self.start(self.hidden) {
                self.message(&format!("字幕无法启动：{error}"));
            }
        } else {
            self.stop();
            self.maybe_exit();
        }
    }

    fn open_overlay(&mut self) {
        if self.overlay.is_some() {
            return;
        }
        let Some(settings) = &self.settings else {
            if let Some(ui) = self.ui.upgrade() {
                ui.set_overlay_visible(false);
            }
            return;
        };
        let geometry = overlay::SavedGeometry {
            bounds: settings.window_bounds(WindowKind::Overlay),
            anchor: settings.window_anchor(WindowKind::Overlay),
            manual_size: settings.window_manual_size(WindowKind::Overlay),
        };
        self.overlay_generation += 1;
        let generation = self.overlay_generation;
        let events = self.overlay_events.clone();
        let ui = self.ui.clone();
        let callback = Arc::new(move |event: overlay::Event| {
            let mut pending = events.lock().unwrap();
            if matches!(event, overlay::Event::StyleChanged(_)) {
                pending.retain(|(_, event)| !matches!(event, overlay::Event::StyleChanged(_)));
            }
            if matches!(
                event,
                overlay::Event::PlacementChanged(..) | overlay::Event::PlacementFailed(_)
            ) {
                pending.retain(|(_, event)| {
                    !matches!(
                        event,
                        overlay::Event::PlacementChanged(..) | overlay::Event::PlacementFailed(_)
                    )
                });
            }
            if pending.len() == 64 {
                pending.pop_front();
            }
            pending.push_back((generation, event));
            drop(pending);
            let weak = ui.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.invoke_host_wakeup();
                }
            });
        });
        match Overlay::open(
            self.overlay_settings.clone(),
            self.overlay_content.clone(),
            geometry,
            callback,
        ) {
            Ok(overlay) => {
                self.overlay = Some(overlay);
                self.shutdown.overlay_open = true;
                if let Some(ui) = self.ui.upgrade() {
                    ui.set_overlay_visible(true);
                }
            }
            Err(error) => {
                self.stop();
                let _ = self.bridge.send(Envelope::new(
                    0,
                    Message::Error {
                        code: "OverlayUnavailable".into(),
                        message: format!("悬浮字幕无法打开：{error}"),
                    },
                ));
            }
        }
    }
    fn close_overlay(&mut self) {
        let generation = self.overlay_generation;
        // Join the producer before consuming its final style. A queued style
        // accepted before Stop/Exit belongs to this generation until now.
        self.overlay.take();
        let (style, placement) = {
            let mut events = self.overlay_events.lock().unwrap();
            let style = events.iter().rev().find_map(|(event_generation, event)| {
                if *event_generation == generation
                    && let overlay::Event::StyleChanged(style) = event
                {
                    Some(style.clone())
                } else {
                    None
                }
            });
            let placement = events.iter().rev().find_map(|(event_generation, event)| {
                if *event_generation != generation {
                    return None;
                }
                match event {
                    overlay::Event::PlacementChanged(value, manual) => {
                        Some(Ok((value.clone(), *manual)))
                    }
                    overlay::Event::PlacementFailed(error) => Some(Err(error.clone())),
                    _ => None,
                }
            });
            events.retain(|(event_generation, _)| *event_generation != generation);
            (style, placement)
        };
        if let Some(style) = style {
            self.store_overlay_style(style);
        }
        match placement {
            Some(Ok((placement, manual))) => self.store_overlay_placement(placement, manual),
            Some(Err(error)) => self.message(&format!("悬浮窗位置暂时无法保存：{error}")),
            None => {}
        }
        self.overlay_generation += 1;
        self.shutdown.overlay_open = false;
        if let Some(ui) = self.ui.upgrade() {
            ui.set_overlay_visible(false);
        }
    }
    fn overlay_updates(&mut self) {
        let events: Vec<_> = self.overlay_events.lock().unwrap().drain(..).collect();
        for (generation, event) in events {
            if generation != self.overlay_generation {
                continue;
            }
            match event {
                overlay::Event::StyleChanged(style) => self.store_overlay_style(style),
                overlay::Event::PlacementChanged(placement, manual) => {
                    self.store_overlay_placement(placement, manual)
                }
                overlay::Event::PlacementFailed(error) => {
                    self.message(&format!("悬浮窗位置暂时无法保存：{error}"))
                }
                overlay::Event::Closed => {
                    self.stop();
                    self.maybe_exit();
                }
                overlay::Event::Failed(message) => {
                    self.stop();
                    let _ = self.bridge.send(Envelope::new(
                        0,
                        Message::Error {
                            code: "OverlayFailed".into(),
                            message: format!("悬浮字幕绘制失败：{message}"),
                        },
                    ));
                    self.maybe_exit();
                }
            }
        }
    }

    fn store_overlay_style(&mut self, style: overlay::Settings) {
        self.overlay_settings = style.clone();
        if let Some(settings) = &self.settings {
            let edited = settings.edited(self.screen, |value| {
                let target = &mut value["OverlayWindow"];
                target["FontSize"] = style.font_size.into();
                target["FontColor"] = style.font_color.into();
                target["FontBold"] = style.bold.into();
                target["Opacity"] = style.opacity.into();
                target["BackgroundColor"] = style.background.into();
                target["FontStroke"] = f64::from(style.stroke).into();
            });
            if let Ok((value, _)) = edited {
                self.save_settings(value);
            }
        }
    }
    fn store_overlay_placement(
        &mut self,
        placement: captions_core::geometry::Placement,
        manual: bool,
    ) {
        if let Some(settings) = &self.settings
            && let Ok((value, _)) =
                settings.positioned(self.screen, WindowKind::Overlay, &placement, manual)
        {
            self.save_settings(value);
        }
    }

    fn show(&mut self, page: i32, hidden: bool) -> io::Result<()> {
        #[cfg(test)]
        if std::mem::take(&mut self.show_failure) {
            return Err(io::Error::other("controlled virtual window show failure"));
        }
        self.retire_when_idle = false;
        self.hidden = hidden;
        let ui = self
            .ui
            .upgrade()
            .ok_or_else(|| io::Error::other("窗口已关闭。"))?;
        ui.set_page(page);
        ui.show()
            .map_err(|error| io::Error::other(error.to_string()))?;
        main_geometry::style_main(&ui);
        self.main_open = true;
        self.recount_windows();
        self.sync_settings_ui();
        self.page_changed(page);
        self.windows_changed();
        Ok(())
    }

    pub fn main_closed(&mut self) {
        self.flush_main_geometry();
        self.cancel_delete_confirmation();
        self.clear_copy_feedback();
        self.history_hidden();
        self.main_open = false;
        self.retire_when_idle = true;
        if self.active.is_none() && self.pending_start {
            self.pending_start = false;
            self.engine = EngineState::Disabled;
            self.engine_changed();
        }
        self.flush_settings();
        self.recount_windows();
        self.windows_changed();
        self.maybe_exit();
    }

    fn engine_changed(&self) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_captions_enabled(self.engine == EngineState::Running);
            ui.set_engine_busy(matches!(
                self.engine,
                EngineState::Starting | EngineState::Stopping
            ));
        }
        let _ = self.bridge.send(Envelope::new(
            0,
            Message::EngineStateChanged { state: self.engine },
        ));
    }

    fn windows_changed(&self) {
        let _ = self.bridge.send(Envelope::new(
            0,
            Message::WindowsChanged {
                visible_windows: self.shutdown.visible_windows,
                live_captions_hidden: self.hidden,
            },
        ));
    }
    fn recount_windows(&mut self) {
        self.shutdown.visible_windows = u32::from(self.main_open)
            + u32::from(
                self.api_window
                    .as_ref()
                    .is_some_and(|api| api.window().is_visible()),
            )
            + u32::from(self.files.dialog_open());
    }
    fn pending_business(&self) -> usize {
        self.flow.pending_saves() + usize::from(self.files.export_pending())
    }

    fn maybe_exit(&mut self) {
        self.shutdown.pending_saves = self.pending_business();
        if self.shutdown.may_exit(self.engine) {
            self.exit();
        }
    }

    fn exit(&mut self) {
        if self.exiting {
            return;
        }
        self.flush_main_geometry();
        self.exiting = true;
        self.cancel_delete_confirmation();
        self.clear_copy_feedback();
        self.history_hidden();
        self.cancel_models();
        self.api_window.take();
        self.shutdown_submitted = false;
        // Exiting announces the save phase before any possibly slow join/I/O.
        // The host must not force-kill an accepted edit while it is being saved.
        let _ = self
            .bridge
            .send(Envelope::new(0, Message::Exiting(Empty {})));
        self.cancel_file_dialog();
        if let Some(ui) = self.ui.upgrade() {
            let _ = ui.hide();
        }
        self.shutdown.visible_windows = 0;
        self.main_open = false;
        self.windows_changed();
        self.stop();
        self.flow_updates();
        self.advance_exit();
    }

    fn advance_exit(&mut self) {
        if !self.exiting
            || self.shutdown_submitted
            || self.pending_business() != 0
            || self.files.dialog_open()
        {
            return;
        }
        if !self.submit_pending_settings() {
            self.save_failed("最后一次修改仍在等待写入，请重试保存。");
            return;
        }
        self.data_request += 1;
        if let Err(error) = self.data.submit(DataCommand::Shutdown {
            request: self.data_request,
        }) {
            self.save_failed(&error.to_string());
        } else {
            self.shutdown_submitted = true;
        }
    }

    fn finish_exit(&mut self) {
        if let Some(ui) = self.ui.upgrade() {
            let _ = ui.hide();
        }
        let _ = slint::quit_event_loop();
    }

    fn save_failed(&mut self, message: &str) {
        self.exiting = false;
        self.shutdown_submitted = false;
        let _ = self.bridge.send(Envelope::new(
            0,
            Message::Error {
                code: "SaveFailed".into(),
                message: format!("保存尚未完成：{message}"),
            },
        ));
        self.message(&format!("保存尚未完成：{message}"));
        if let Some(ui) = self.ui.upgrade() {
            ui.set_retry_save(true);
        }
        let _ = self.show(1, self.hidden);
    }
    fn message(&self, message: &str) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_data_message(message.into());
        }
    }
    fn flush_settings(&mut self) {
        if !self.submit_pending_settings() {
            return;
        }
        self.data_request += 1;
        if let Err(error) = self.data.submit(DataCommand::Flush {
            request: self.data_request,
        }) {
            self.message(&error.to_string());
        }
    }
    pub fn prompt_changed(&mut self, prompt: &str) {
        if self.exiting {
            return;
        }
        let Some(settings) = &self.settings else {
            return;
        };
        match settings.with_prompt(prompt) {
            Ok(value) => self.save_settings(value),
            Err(error) => self.message(&error.to_string()),
        }
    }
    fn save_settings(&mut self, value: Settings) {
        if self.settings.as_ref() == Some(&value) {
            return;
        }
        self.settings_revision += 1;
        if self
            .settings
            .as_ref()
            .is_some_and(|previous| flow::translation_changed(previous, &value))
        {
            let log_only = self.ui.upgrade().is_some_and(|ui| ui.get_log_only());
            self.capture_cutoff();
            self.flow.reconfigure(value.clone(), log_only);
            if let Some(engine) = &self.active {
                engine.cancel_all();
            }
        } else {
            self.flow.presentation_settings(value.clone());
        }
        self.settings = Some(value.clone());
        self.pending_settings = Some((self.settings_revision, value));
        self.submit_pending_settings();
        self.sync_settings_ui();
    }
    fn submit_pending_settings(&mut self) -> bool {
        let Some((revision, value)) = self.pending_settings.take() else {
            return true;
        };
        match self
            .data
            .client
            .submit(DataCommand::Settings { revision, value })
        {
            Ok(()) => true,
            Err(error) => {
                self.message(&error.to_string());
                if let DataCommand::Settings { revision, value } = *error.command {
                    self.pending_settings = Some((revision, value));
                }
                false
            }
        }
    }
    pub fn data_action(&mut self, action: i32, path: &str) {
        if self.exiting {
            return;
        }
        self.data_request += 1;
        let request = self.data_request;
        let command = match action {
            0 => DataCommand::UseDefaults { request },
            2 => DataCommand::RecoverConfig {
                request,
                path: path.into(),
            },
            3 => DataCommand::Retry {
                request,
                directory: (!path.trim().is_empty()).then(|| path.into()),
            },
            4 => {
                self.flow.retry();
                // Retry even if a prior queue-full response prevented submission.
                if let Some(value) = self.settings.clone() {
                    self.settings_revision += 1;
                    self.pending_settings = Some((self.settings_revision, value));
                    if !self.submit_pending_settings() {
                        return;
                    }
                }
                DataCommand::Flush { request }
            }
            _ => return,
        };
        if let Err(error) = self.data.submit(command) {
            self.message(&error.to_string());
        }
    }
    fn data_updates(&mut self) {
        for event in self.data.take() {
            if self.file_event(&event) {
                continue;
            }
            if self.history_event(&event) {
                continue;
            }
            if self.flow.data_event(&event) {
                continue;
            }
            match event {
                DataEvent::FirstUse => {
                    // Only an empty, uninitialized directory emits FirstUse.
                    // Existing damaged data still requires recovery.
                    self.data_action(0, "");
                }
                DataEvent::Ready {
                    settings,
                    repairs,
                    migration,
                } => {
                    let style = &settings.json()["OverlayWindow"];
                    self.overlay_settings = overlay::Settings {
                        font_size: style["FontSize"].as_u64().unwrap() as u8,
                        font_color: style["FontColor"].as_u64().unwrap() as u8,
                        bold: style["FontBold"].as_u64().unwrap() as u8,
                        opacity: style["Opacity"].as_u64().unwrap() as u8,
                        background: style["BackgroundColor"].as_u64().unwrap() as u8,
                        stroke: style["FontStroke"].as_f64().unwrap() as f32,
                    };
                    if let Some(ui) = self.ui.upgrade() {
                        ui.set_data_state(2);
                        ui.set_settings_prompt(settings.json()["Prompt"].as_str().unwrap().into());
                        ui.set_topmost(settings.json()["MainWindow"]["Topmost"].as_bool().unwrap());
                        ui.set_selected_service(settings.api_name().into());
                        ui.set_selected_language(settings.target_language().into());
                    }
                    self.settings = Some(settings);
                    self.saved_settings_revision = self.settings_revision;
                    self.sync_settings_ui();
                    self.history_refresh();
                    self.message(&if let Some(report) = migration {
                        format!("已导入 {} 条历史；原文件保留。", report.migrated_rows)
                    } else if !repairs.fields.is_empty() {
                        format!("已修正 {} 项配置字段。", repairs.fields.len())
                    } else {
                        String::new()
                    });
                    if self.pending_start && !self.exiting {
                        self.pending_start = false;
                        if let Err(error) = self.start(self.hidden) {
                            self.engine = EngineState::Disabled;
                            self.engine_changed();
                            self.message(&error.to_string());
                        }
                    }
                }
                DataEvent::RecoveryRequired {
                    kind,
                    location,
                    message,
                } => {
                    if let Some(ui) = self.ui.upgrade() {
                        ui.set_data_state(match kind {
                            Recovery::Migration => 3,
                            Recovery::Settings => 4,
                            Recovery::Database => 5,
                        });
                        ui.set_recovery_location(location.to_string_lossy().as_ref().into());
                    }
                    self.message(&message);
                }
                DataEvent::SettingsSaved { revision, .. } => {
                    self.saved_settings_revision = self.saved_settings_revision.max(revision);
                    if revision == self.settings_revision
                        && !self.flow.is_blocked()
                        && self.flow.pending_saves() == 0
                    {
                        self.message("设置已保存。");
                    }
                }
                DataEvent::Stopped { .. } => {
                    if let Err(error) = self.data.stopped() {
                        self.message(&error.to_string());
                    }
                    self.finish_exit();
                }
                DataEvent::Error {
                    operation, message, ..
                } => {
                    if operation == Operation::Settings
                        && let Some(ui) = self.ui.upgrade()
                    {
                        ui.set_retry_save(true);
                    }
                    if operation == Operation::Shutdown {
                        self.save_failed(&message);
                    } else {
                        if operation == Operation::Initialize
                            && let Some(ui) = self.ui.upgrade()
                        {
                            ui.set_data_state(6);
                        }
                        self.message(&message);
                    }
                }
                _ => {}
            }
        }
        if let Err(error) = self.pending_start_prompt() {
            let _ = self.bridge.send(Envelope::new(
                0,
                Message::Error {
                    code: "CommandFailed".into(),
                    message: error.to_string(),
                },
            ));
            self.maybe_exit();
        }
        if !self.exiting {
            self.submit_pending_settings();
            self.submit_history();
        }
    }

    fn flow_updates(&mut self) {
        let available = self.active.as_ref().map_or(0, Engine::available);
        self.flow.drive(available);
        if self.flow.can_capture() {
            self.mailbox.wake_source();
        }
        self.flow.submit(&self.data.client);
        for action in self.flow.actions() {
            match action {
                Action::Cancel(key) => {
                    if let Some(engine) = &self.active {
                        engine.cancel(&key);
                    }
                }
                Action::Dispatch(request) => {
                    if let Some(engine) = &mut self.active {
                        let task = request.task.clone();
                        if let Err(error) = engine.dispatch(*request) {
                            self.flow.completed(crate::engine::Completion {
                                task,
                                result: Some(Err(error.to_string())),
                                latency_ms: 0,
                            });
                        }
                    }
                }
            }
        }
        if let Some(view) = self.flow.view()
            && self.active.as_ref().is_some_and(|engine| {
                engine.revision.load(std::sync::atomic::Ordering::Acquire) == view.capture_revision
            })
        {
            self.overlay_content
                .set_translation(view.overlay_translation, &view.translation);
            self.overlay_content.notice.clone_from(&view.notice);
            if let Some(ui) = self.ui.upgrade() {
                ui.set_caption_notice(view.notice.into());
                ui.set_latency_ms(
                    if view.translation.is_empty() {
                        String::new()
                    } else {
                        view.latency_ms.to_string()
                    }
                    .into(),
                );
                ui.set_source_caption(view.source.into());
                ui.set_caption_cards(slint::ModelRc::new(slint::VecModel::from(
                    view.cards
                        .into_iter()
                        .map(|card| crate::HistoryItem {
                            stamp_full: "".into(),
                            source: card.source.into(),
                            translation: card.translation.into(),
                            ..Default::default()
                        })
                        .collect::<Vec<_>>(),
                )));
                ui.set_translated_caption(view.translation.into());
            }
            self.overlay_content.source = view.overlay_source;
            if let Some(overlay) = &self.overlay {
                overlay.update(self.overlay_content.clone());
            }
        }
        self.shutdown.pending_saves = self.pending_business();
        let error = self.flow.error();
        if self.exiting && self.flow.is_blocked() {
            self.save_failed(error.as_deref().unwrap_or("仍有未保存内容，请重试保存。"));
        } else if let Some(error) = error {
            self.message(&error);
            if self.flow.is_blocked()
                && let Some(ui) = self.ui.upgrade()
            {
                ui.set_retry_save(true);
            }
        }
        if !self.flow.is_blocked()
            && self.flow.pending_saves() == 0
            && self.saved_settings_revision >= self.settings_revision
            && let Some(ui) = self.ui.upgrade()
            && ui.get_retry_save()
        {
            ui.set_retry_save(false);
            self.message("设置已保存。");
        }
        if self.retire_when_idle && !self.exiting && self.shutdown.may_exit(self.engine) {
            self.exit();
        }
    }
}
