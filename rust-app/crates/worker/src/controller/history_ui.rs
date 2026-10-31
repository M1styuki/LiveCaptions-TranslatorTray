//! Only a visible history page subscribes to durable record changes. Keep at
//! most one database query in flight and one latest desired view, never a queue
//! of clicks. The text editor is a draft until Search/Enter is submitted.
use super::Controller;
use slint::ComponentHandle;
use storage::{
    actor::{Command, Event, Operation},
    history::Save,
};

pub(super) struct History {
    visible: bool,
    query: String,
    page: u64,
    size: u16,
    before_search_page: u64,
    generation: u64,
    pending: Option<(u64, u64)>,
    dirty: bool,
}
impl Default for History {
    fn default() -> Self {
        Self {
            visible: false,
            query: String::new(),
            page: 1,
            size: 30,
            before_search_page: 1,
            generation: 0,
            pending: None,
            dirty: false,
        }
    }
}
impl History {
    fn changed(&mut self) {
        self.generation += 1;
        self.dirty = true;
    }
}
impl Controller {
    pub fn bind_history(owner: &std::rc::Rc<std::cell::RefCell<Self>>) {
        let Some(ui) = owner.borrow().ui.upgrade() else {
            return;
        };
        let weak = std::rc::Rc::downgrade(owner);
        ui.on_history_requested(move |page| {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().history_requested(page);
            }
        });
        let weak = std::rc::Rc::downgrade(owner);
        ui.on_history_action(move |action, value| {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().history_action(action, value);
            }
        });
        let weak = std::rc::Rc::downgrade(owner);
        ui.on_page_changed(move |page| {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().page_changed(page);
            }
        });
    }
    #[cfg(test)]
    pub fn test_show_history(&mut self) {
        self.show(2, self.hidden).unwrap();
    }
    #[cfg(test)]
    pub fn test_history_idle(&self) -> bool {
        self.history.pending.is_none() && !self.history.dirty
    }
    #[cfg(test)]
    pub fn test_history_request(&self) -> u64 {
        self.history_request
    }
    pub fn page_changed(&mut self, page: i32) {
        if page != 2 {
            self.cancel_delete_confirmation();
        }
        if let Some(ui) = self.ui.upgrade() {
            ui.set_page(page);
            self.sync_main_geometry();
            if page == 2 && ui.window().is_visible() && !self.exiting {
                self.history.visible = true;
                self.history.changed();
                self.submit_history();
                return;
            }
        }
        self.history_hidden();
    }
    pub(super) fn history_hidden(&mut self) {
        self.history.visible = false;
        self.history.generation += 1;
        self.history.dirty = false;
    }
    pub fn history_requested(&mut self, page: i32) {
        if self.exiting {
            return;
        }
        self.history.page = page.max(1) as u64;
        self.history.changed();
        self.submit_history();
    }
    // Search, clear search, explicit refresh, and page capacity respectively.
    pub fn history_action(&mut self, action: i32, value: i32) {
        if self.exiting {
            return;
        }
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        match action {
            0 => {
                let query = ui.get_history_query().to_string();
                if query.is_empty() {
                    self.clear_history_search();
                } else {
                    if self.history.query.is_empty() {
                        self.history.before_search_page = self.history.page;
                    }
                    self.history.query = query;
                    self.history.page = 1;
                }
            }
            1 => self.clear_history_search(),
            2 => {}
            3 if [20, 30, 50, 100].contains(&value) => {
                self.history.size = value as u16;
                self.history.page = 1;
                ui.set_history_size(value);
            }
            _ => return,
        }
        self.history.changed();
        self.submit_history();
    }
    fn clear_history_search(&mut self) {
        if !self.history.query.is_empty() {
            self.history.page = self.history.before_search_page;
        }
        self.history.query.clear();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_history_query("".into());
        }
    }
    pub(super) fn history_refresh(&mut self) {
        if self.history.visible && !self.exiting {
            self.history.dirty = true;
        }
    }
    pub(super) fn history_cleared(&mut self) {
        // Reject a page response started before the destructive transaction.
        self.history.page = 1;
        self.history.before_search_page = 1;
        self.history.changed();
        self.history.dirty = self.history.visible;
        if let Some(ui) = self.ui.upgrade() {
            ui.set_history_page(1);
            ui.set_history_pages(1);
            ui.set_history_total("0".into());
            ui.set_history_rows(slint::ModelRc::default());
        }
        self.submit_history();
    }
    pub(super) fn submit_history(&mut self) {
        if self.exiting
            || self.settings.is_none()
            || !self.history.visible
            || !self.history.dirty
            || self.history.pending.is_some()
        {
            return;
        }
        self.data_request += 1;
        let request = self.data_request;
        let command = Command::Page {
            request,
            query: self.history.query.clone(),
            page: self.history.page,
            size: self.history.size,
        };
        if let Err(error) = self.data.submit(command) {
            // Leave the latest view dirty. An existing bounded actor command's
            // acknowledgement can retry queue submission; no timer or loop.
            self.message(&error.to_string());
        } else {
            self.history_request = request;
            self.history.pending = Some((request, self.history.generation));
            self.history.dirty = false;
        }
    }
    pub(super) fn history_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Saved {
                result: Save::Written(_),
                ..
            } => {
                self.history_refresh();
                // Flow must still receive its own durable acknowledgement.
                false
            }
            Event::Page { request, value } => {
                let Some((pending, generation)) = self.history.pending else {
                    return false;
                };
                if pending != *request {
                    return false;
                }
                self.history.pending = None;
                if generation == self.history.generation && self.history.visible {
                    self.history.page = value.page;
                    if let Some(ui) = self.ui.upgrade() {
                        ui.set_history_page(value.page.min(i32::MAX as u64) as i32);
                        ui.set_history_pages(value.pages.min(i32::MAX as u64) as i32);
                        ui.set_history_total(value.total.to_string().into());
                        let rows: Vec<_> = value
                            .rows
                            .iter()
                            .map(|row| crate::HistoryItem {
                                stamp: storage::timestamp::display(&row.timestamp, false).into(),
                                stamp_full: storage::timestamp::display(&row.timestamp, true)
                                    .into(),
                                source: row.source.as_str().into(),
                                translation: row.translation.as_str().into(),
                                service: row.api_used.as_str().into(),
                                language: row.target_language.as_str().into(),
                            })
                            .collect();
                        ui.set_history_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
                    }
                }
                true
            }
            Event::Error {
                request,
                operation: Operation::Page,
                message,
            } if self
                .history
                .pending
                .is_some_and(|(pending, _)| pending == *request) =>
            {
                let (_, generation) = self.history.pending.take().unwrap();
                if self.history.visible && generation == self.history.generation {
                    self.message(message);
                }
                // A later click/change may retry; an SQL failure itself never
                // schedules repeated identical queries.
                true
            }
            _ => false,
        }
    }
}
