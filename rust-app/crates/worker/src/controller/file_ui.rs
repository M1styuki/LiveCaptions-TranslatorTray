use super::Controller;
use protocol::{Envelope, Message};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
};
use storage::actor::{Command, Event, Operation};
use windows_platform::file_dialog::{Job, Kind};

#[derive(Clone, Copy)]
enum Purpose {
    Recover,
    Retry,
    Export,
}
struct Selection {
    generation: u64,
    result: Result<Option<PathBuf>, String>,
}
#[derive(Default)]
pub(super) struct Files {
    job: Option<Job>,
    slot: Arc<Mutex<Option<Selection>>>,
    generation: u64,
    purpose: Option<Purpose>,
    export: Option<u64>,
    #[cfg(test)]
    hidden_fixture: bool,
}
impl Files {
    pub(super) fn dialog_open(&self) -> bool {
        self.job.is_some()
    }
    pub(super) fn export_pending(&self) -> bool {
        self.export.is_some()
    }
}
impl Controller {
    pub fn bind_files(owner: &Rc<RefCell<Self>>) {
        let Some(ui) = owner.borrow().ui.upgrade() else {
            return;
        };
        let weak = Rc::downgrade(owner);
        ui.on_file_requested(move |action| {
            if let Some(owner) = weak.upgrade() {
                owner.borrow_mut().file_requested(action);
            }
        });
    }
    pub fn file_requested(&mut self, action: i32) {
        if self.exiting || self.files.job.is_some() {
            return;
        }
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let (purpose, kind) = match (action, ui.get_data_state()) {
            (1, 3 | 4) => (Purpose::Recover, Kind::Config),
            (2, 3) => (Purpose::Retry, Kind::Folder),
            (3, 2) if self.files.export.is_none() => (Purpose::Export, Kind::Csv),
            _ => return,
        };
        if matches!(purpose, Purpose::Export) {
            ui.set_export_message("".into());
        }
        self.files.generation += 1;
        let generation = self.files.generation;
        let slot = self.files.slot.clone();
        let weak = self.ui.clone();
        let callback = Arc::new(move |result| {
            *slot.lock().unwrap() = Some(Selection { generation, result });
            let ui = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui.upgrade() {
                    ui.invoke_host_wakeup();
                }
            });
        });
        #[cfg(test)]
        let result = if self.files.hidden_fixture {
            Job::start_hidden_fixture(kind, callback)
        } else {
            start_owned_dialog(&ui, kind, callback)
        };
        #[cfg(not(test))]
        let result = start_owned_dialog(&ui, kind, callback);
        match result {
            Ok(job) => {
                self.files.job = Some(job);
                self.files.purpose = Some(purpose);
                ui.set_file_busy(true);
                self.recount_windows();
                self.windows_changed();
            }
            Err(error) if matches!(purpose, Purpose::Export) => {
                self.export_failed(&error.to_string())
            }
            Err(error) => self.message(&error.to_string()),
        }
    }
    pub(super) fn cancel_file_dialog(&mut self) {
        self.files.purpose = None;
        if let Some(job) = &self.files.job {
            // Keep pumping the UI until the STA has destroyed its owned HWNDs.
            // Its completion callback remains valid and will reap/join the job.
            job.cancel();
        }
    }
    pub(super) fn file_updates(&mut self) {
        let result = self.files.slot.lock().unwrap().take();
        let Some(selection) = result else {
            return;
        };
        if selection.generation != self.files.generation {
            return;
        }
        let purpose = self.files.purpose.take();
        self.files.job.take();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_file_busy(false);
        }
        self.recount_windows();
        self.windows_changed();
        if !self.exiting {
            match (purpose, selection.result) {
                (Some(purpose), Ok(Some(path))) => match purpose {
                    Purpose::Export => self.export_selected(path),
                    _ => {
                        self.data_request += 1;
                        let request = self.data_request;
                        // Preserve native Windows paths, including unpaired
                        // UTF-16, instead of passing through lossy UI strings.
                        let command = match purpose {
                            Purpose::Recover => Command::RecoverConfig { request, path },
                            Purpose::Retry => Command::Retry {
                                request,
                                directory: Some(path),
                            },
                            Purpose::Export => unreachable!(),
                        };
                        if let Err(error) = self.data.submit(command) {
                            self.message(&error.to_string());
                        }
                    }
                },
                (Some(Purpose::Export), Err(error)) => self.export_failed(&error),
                (_, Err(error)) => self.message(&error),
                _ => {} // Cancel never submits a storage command or creates a file.
            }
        }
        self.maybe_exit();
    }
    fn export_selected(&mut self, path: PathBuf) {
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
        {
            self.export_failed("请选择 .csv 文件，配置和数据库文件不会被覆盖。");
            return;
        }
        self.data_request += 1;
        let request = self.data_request;
        if let Err(error) = self.data.submit(Command::Export { request, path }) {
            self.export_failed(&error.to_string());
        } else {
            self.files.export = Some(request);
            if let Some(ui) = self.ui.upgrade() {
                ui.set_export_busy(true);
                ui.set_export_message("正在导出全部历史…".into());
            }
        }
    }
    fn export_failed(&mut self, message: &str) {
        let message = format!("历史导出未完成：{message}");
        if self.exiting {
            self.exiting = false;
            self.shutdown_submitted = false;
            let _ = self.bridge.send(Envelope::new(
                0,
                Message::Error {
                    code: "SaveFailed".into(),
                    message: message.clone(),
                },
            ));
        }
        let _ = self.show(2, self.hidden);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_export_message(message.into());
        }
    }
    pub(super) fn file_event(&mut self, event: &Event) -> bool {
        let Some(request) = self.files.export else {
            return false;
        };
        match event {
            Event::Exported {
                request: done,
                path,
                rows,
            } if *done == request => {
                self.files.export = None;
                if let Some(ui) = self.ui.upgrade() {
                    ui.set_export_busy(false);
                }
                if let Some(ui) = self.ui.upgrade() {
                    ui.set_export_message(
                        format!("已导出全部 {rows} 条历史：{}", path.display()).into(),
                    );
                }
                true
            }
            Event::Error {
                request: done,
                operation: Operation::Export,
                message,
            } if *done == request => {
                self.files.export = None;
                if let Some(ui) = self.ui.upgrade() {
                    ui.set_export_busy(false);
                }
                self.export_failed(message);
                true
            }
            _ => false,
        }
    }
    #[cfg(test)]
    pub fn test_hidden_file_dialogs(&mut self) {
        self.files.hidden_fixture = true;
    }
    #[cfg(test)]
    pub fn test_select_file(&mut self, result: Result<Option<PathBuf>, String>) {
        assert!(self.files.purpose.is_some());
        self.files.job.take();
        *self.files.slot.lock().unwrap() = Some(Selection {
            generation: self.files.generation,
            result,
        });
    }
    #[cfg(test)]
    pub fn test_host_exit(&mut self) {
        self.exit();
    }
}

fn start_owned_dialog(
    ui: &crate::MainWindow,
    kind: Kind,
    callback: windows_platform::file_dialog::Callback,
) -> std::io::Result<Job> {
    let parent = super::main_geometry::hwnd(ui)
        .ok_or_else(|| std::io::Error::other("设置窗口尚未就绪，请重新打开后再选择文件。"))?;
    Job::start_owned(kind, ui.get_topmost(), parent, callback)
}
