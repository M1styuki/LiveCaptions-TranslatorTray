use super::Controller;
use protocol::{Envelope, Message};
use std::{cell::RefCell, rc::Rc};
#[derive(Default)]
pub(super) struct Deletion {
    confirming: bool,
    busy: bool,
}
impl Controller {
    pub fn bind_delete(owner: &Rc<RefCell<Self>>) {
        let Some(ui) = owner.borrow().ui.upgrade() else {
            return;
        };
        let weak = Rc::downgrade(owner);
        ui.on_delete_action(move |action| {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().delete_action(action);
            }
        });
    }
    // Request confirmation, cancel, confirm. No other callback can start Delete.
    pub fn delete_action(&mut self, action: i32) {
        if self.exiting || self.deletion.busy {
            return;
        }
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        match action {
            0 if self.settings.is_some()
                && !self.files.dialog_open()
                && !self.files.export_pending() =>
            {
                self.deletion.confirming = true;
                ui.set_delete_confirm(true);
            }
            1 => self.cancel_delete_confirmation(),
            2 if self.deletion.confirming => {
                self.cancel_delete_confirmation();
                self.deletion.busy = true;
                ui.set_delete_busy(true);
                ui.set_delete_message("正在清空全部历史…".into());
                // Adopt the latest capture before selecting the suppressed
                // current sentence and invalidating in-flight translations.
                self.engine_updates();
                self.capture_cutoff();
                self.flow.clear_history();
                self.flow_updates();
            }
            _ => {}
        }
    }
    pub(super) fn cancel_delete_confirmation(&mut self) {
        self.deletion.confirming = false;
        if let Some(ui) = self.ui.upgrade() {
            ui.set_delete_confirm(false);
        }
    }
    pub(super) fn delete_updates(&mut self) {
        let Some(result) = self.flow.take_clear_result() else {
            return;
        };
        self.deletion.busy = false;
        if let Some(ui) = self.ui.upgrade() {
            ui.set_delete_busy(false);
        }
        match result {
            Ok(()) => {
                self.history_cleared();
                if let Some(ui) = self.ui.upgrade() {
                    ui.set_delete_message("已清空全部历史。".into());
                }
            }
            Err(message) => {
                if self.exiting {
                    self.exiting = false;
                    self.shutdown_submitted = false;
                    let _ = self.bridge.send(Envelope::new(
                        0,
                        Message::Error {
                            code: "SaveFailed".into(),
                            message: "历史删除未完成，已保留错误提示。".into(),
                        },
                    ));
                }
                let _ = self.show(2, self.hidden);
                if let Some(ui) = self.ui.upgrade() {
                    ui.set_delete_message(format!("删除未完成，请稍后重试。{message}").into());
                }
            }
        }
        self.maybe_exit();
    }
}
