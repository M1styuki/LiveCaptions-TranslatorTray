//! A single database/settings owner. Commands are bounded, settings coalesce
//! for 300ms, and explicit flush/shutdown saves the final accepted edit.
use crate::{
    atomic_file, export,
    history::{Database, Fence, Page, Record, Save},
    migrate::{self, Outcome, Report},
    settings_file::{LoadError, SettingsFile},
};
use captions_core::{
    scheduler::Task,
    session::WorkKey,
    settings::{RepairReport, Screen, Settings},
};
use std::{
    fmt, fs, io,
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const CAPACITY: usize = 64;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Initialize,
    Recovery,
    Settings,
    Flush,
    Fence,
    Expect,
    History,
    Pending,
    Page,
    Export,
    Delete,
    Shutdown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    Migration,
    Settings,
    Database,
}
#[derive(Clone)]
pub enum Command {
    UseDefaults {
        request: u64,
    },
    Import {
        request: u64,
        directory: PathBuf,
    },
    RecoverConfig {
        request: u64,
        path: PathBuf,
    },
    Retry {
        request: u64,
        directory: Option<PathBuf>,
    },
    Settings {
        revision: u64,
        value: Settings,
    },
    Flush {
        request: u64,
    },
    Fence {
        request: u64,
        value: Option<Fence>,
    },
    Expect {
        request: u64,
        key: WorkKey,
    },
    Revise {
        request: u64,
        key: WorkKey,
    },
    Finished {
        request: u64,
        key: WorkKey,
    },
    Save {
        request: u64,
        key: WorkKey,
        value: Record,
    },
    Pending {
        request: u64,
        task: Task,
    },
    TakePending {
        request: u64,
        limit: usize,
    },
    Page {
        request: u64,
        query: String,
        page: u64,
        size: u16,
    },
    Export {
        request: u64,
        path: PathBuf,
    },
    Delete {
        request: u64,
    },
    Shutdown {
        request: u64,
    },
}
pub enum Event {
    FirstUse,
    Ready {
        settings: Settings,
        repairs: RepairReport,
        migration: Option<Box<Report>>,
    },
    RecoveryRequired {
        kind: Recovery,
        location: PathBuf,
        message: String,
    },
    SettingsSaved {
        revision: u64,
        written: bool,
    },
    Flushed {
        request: u64,
    },
    FenceSet {
        request: u64,
    },
    Expected {
        request: u64,
        key: WorkKey,
        accepted: bool,
    },
    Saved {
        request: u64,
        key: WorkKey,
        result: Save,
    },
    PendingStored {
        request: u64,
        key: WorkKey,
        accepted: bool,
    },
    PendingTaken {
        request: u64,
        tasks: Vec<Task>,
        remaining: u64,
    },
    Page {
        request: u64,
        value: Page,
    },
    Exported {
        request: u64,
        path: PathBuf,
        rows: u64,
    },
    Deleted {
        request: u64,
    },
    Stopped {
        request: u64,
    },
    Error {
        request: u64,
        operation: Operation,
        message: String,
    },
}
pub type Callback = Arc<dyn Fn(Event) + Send + Sync>;
#[derive(Clone)]
pub struct Client {
    sender: SyncSender<Command>,
}
pub struct SubmitError {
    pub command: Box<Command>,
    pub queue_full: bool,
}
impl fmt::Debug for SubmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubmitError")
            .field("queue_full", &self.queue_full)
            .finish_non_exhaustive()
    }
}
impl fmt::Display for SubmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.queue_full {
            "数据队列已满，请等待保存完成。"
        } else {
            "数据工作线程已关闭。"
        })
    }
}
impl std::error::Error for SubmitError {}
impl Client {
    pub fn submit(&self, command: Command) -> Result<(), SubmitError> {
        self.sender.try_send(command).map_err(|error| match error {
            TrySendError::Full(command) => SubmitError {
                command: Box::new(command),
                queue_full: true,
            },
            TrySendError::Disconnected(command) => SubmitError {
                command: Box::new(command),
                queue_full: false,
            },
        })
    }
}
pub struct Storage {
    client: Option<Client>,
    thread: Option<JoinHandle<()>>,
}
impl Storage {
    pub fn start(
        data: PathBuf,
        legacy: PathBuf,
        screen: Screen,
        instance: String,
        callback: Callback,
    ) -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(CAPACITY);
        let thread = thread::Builder::new()
            .name("settings-history".into())
            .spawn(move || {
                let mut actor = Actor {
                    data,
                    legacy,
                    screen,
                    instance,
                    callback,
                    active: None,
                    waiting: Waiting::None,
                };
                if let Err(error) = actor.initialize(false) {
                    actor.error(0, Operation::Initialize, error);
                }
                loop {
                    let received = if let Some(deadline) =
                        actor.active.as_ref().and_then(|state| state.due)
                    {
                        receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    } else {
                        receiver.recv().map_err(|_| RecvTimeoutError::Disconnected)
                    };
                    match received {
                        Ok(command) => {
                            if actor.handle(command) {
                                break;
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            let _ = actor.flush_settings();
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            let _ = actor.flush_settings();
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            client: Some(Client { sender }),
            thread: Some(thread),
        })
    }
    pub fn client(&self) -> Client {
        self.client.as_ref().expect("live storage owner").clone()
    }
    /// Join only after receiving Stopped, when the thread has closed its DB.
    pub fn join(mut self) -> io::Result<()> {
        self.client.take();
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| io::Error::other("数据工作线程异常结束。"))?;
        }
        Ok(())
    }
}
struct Active {
    file: SettingsFile,
    settings: Settings,
    database: Database,
    dirty: Option<u64>,
    due: Option<Instant>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Waiting {
    None,
    FirstUse,
    Migration,
    Settings,
    Database,
}
struct Actor {
    data: PathBuf,
    legacy: PathBuf,
    screen: Screen,
    instance: String,
    callback: Callback,
    active: Option<Active>,
    waiting: Waiting,
}
impl Actor {
    fn emit(&self, event: Event) {
        (self.callback)(event);
    }
    fn error(&self, request: u64, operation: Operation, error: impl fmt::Display) {
        self.emit(Event::Error {
            request,
            operation,
            message: error.to_string(),
        });
    }
    fn recovery(&mut self, kind: Recovery, location: PathBuf, message: String) {
        self.waiting = match kind {
            Recovery::Migration => Waiting::Migration,
            Recovery::Settings => Waiting::Settings,
            Recovery::Database => Waiting::Database,
        };
        self.emit(Event::RecoveryRequired {
            kind,
            location,
            message,
        });
    }
    fn load(&mut self, migration: Option<Report>) -> io::Result<()> {
        let loaded = match SettingsFile::open(&self.data, self.screen) {
            Ok(loaded) => loaded,
            Err(LoadError::Recovery { backup, message }) => {
                self.recovery(Recovery::Settings, backup, message);
                return Ok(());
            }
            Err(error) => return Err(io::Error::other(error)),
        };
        self.activate(loaded.file, loaded.settings, loaded.report, migration)
    }
    fn activate(
        &mut self,
        mut file: SettingsFile,
        settings: Settings,
        repairs: RepairReport,
        migration: Option<Report>,
    ) -> io::Result<()> {
        let database = match Database::open(
            &self.data.join("translation_history.db"),
            self.instance.clone(),
        ) {
            Ok(database) => database,
            Err(error) => {
                self.recovery(
                    Recovery::Database,
                    self.data.join("translation_history.db"),
                    error.to_string(),
                );
                return Ok(());
            }
        };
        file.save(&settings)?;
        atomic_file::write(&self.data.join(".initialized"), |file| {
            std::io::Write::write_all(file, b"1\n")
        })?;
        self.active = Some(Active {
            file,
            settings: settings.clone(),
            database,
            dirty: None,
            due: None,
        });
        self.waiting = Waiting::None;
        self.emit(Event::Ready {
            settings,
            repairs,
            migration: migration.map(Box::new),
        });
        Ok(())
    }
    fn outcome(&mut self, outcome: Outcome, allow_defaults: bool) -> io::Result<()> {
        match outcome {
            Outcome::Imported(report) => self.load(Some(report)),
            Outcome::AlreadyInitialized => self.load(None),
            Outcome::NoLegacyFiles => {
                if allow_defaults {
                    self.load(None)
                } else {
                    self.waiting = Waiting::FirstUse;
                    self.emit(Event::FirstUse);
                    Ok(())
                }
            }
            Outcome::NeedsRecovery { stage, message } => {
                self.recovery(Recovery::Migration, stage, message);
                Ok(())
            }
        }
    }
    fn initialize(&mut self, allow_defaults: bool) -> io::Result<()> {
        let outcome = migrate::initialize(&self.data, &self.legacy, self.screen)?;
        self.outcome(outcome, allow_defaults)
    }
    fn defaults(&mut self) -> io::Result<()> {
        match self.waiting {
            Waiting::FirstUse => self.initialize(true),
            Waiting::Migration => {
                let outcome = migrate::recover_with_defaults(&self.data, self.screen)?;
                self.outcome(outcome, true)
            }
            Waiting::Settings => {
                let (file, settings) =
                    SettingsFile::defaults_after_recovery(&self.data, self.screen);
                self.activate(file, settings, RepairReport::default(), None)
            }
            _ => Err(io::Error::other(
                "当前没有可以使用默认设置恢复的配置；数据库不会被清空。",
            )),
        }
    }
    fn replacement(&mut self, path: PathBuf) -> io::Result<()> {
        match self.waiting {
            Waiting::Migration => {
                let outcome = migrate::recover_with_config(&self.data, &path, self.screen)?;
                self.outcome(outcome, true)
            }
            Waiting::Settings => {
                let bytes = fs::read(path)?;
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| io::Error::other("所选配置不是有效的UTF-8。"))?;
                let (settings, repairs) =
                    Settings::parse(text, self.screen).map_err(io::Error::other)?;
                let (file, _) = SettingsFile::defaults_after_recovery(&self.data, self.screen);
                self.activate(file, settings, repairs, None)
            }
            _ => Err(io::Error::other("当前不需要替换恢复配置。")),
        }
    }
    fn flush_settings(&mut self) -> bool {
        let Some(state) = &mut self.active else {
            return true;
        };
        state.due = None;
        let Some(revision) = state.dirty else {
            return true;
        };
        match state.file.save(&state.settings) {
            Ok(written) => {
                state.dirty = None;
                self.emit(Event::SettingsSaved { revision, written });
                true
            }
            Err(error) => {
                self.error(revision, Operation::Settings, error);
                false
            }
        }
    }
    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::UseDefaults { request } => {
                if let Err(error) = self.defaults() {
                    self.error(request, Operation::Recovery, error);
                }
                return false;
            }
            Command::Import { request, directory } => {
                if self.waiting != Waiting::FirstUse {
                    self.error(
                        request,
                        Operation::Recovery,
                        "数据目录已初始化，不会自动合并旧数据。",
                    );
                    return false;
                }
                match migrate::initialize(&self.data, &directory, self.screen) {
                    Ok(Outcome::NoLegacyFiles) => self.error(
                        request,
                        Operation::Recovery,
                        "所选目录没有旧版setting.json或translation_history.db。",
                    ),
                    Ok(outcome) => {
                        self.legacy = directory;
                        if let Err(error) = self.outcome(outcome, true) {
                            self.error(request, Operation::Recovery, error);
                        }
                    }
                    Err(error) => self.error(request, Operation::Recovery, error),
                }
                return false;
            }
            Command::RecoverConfig { request, path } => {
                if let Err(error) = self.replacement(path) {
                    self.error(request, Operation::Recovery, error);
                }
                return false;
            }
            Command::Retry { request, directory } => {
                let result = if self.waiting == Waiting::Migration
                    && migrate::stage_path(&self.data).is_ok_and(|path| path.exists())
                {
                    let source = directory.map(Ok).unwrap_or_else(|| {
                        migrate::retry_source(&self.data)?.ok_or_else(|| io::Error::other(
                            "无法确定上次迁移的旧数据来源，请重新选择旧版文件夹；保留副本尚未移动。"))
                    });
                    source.and_then(|source| {
                        let outcome = migrate::retry(&self.data, &source, self.screen)?;
                        self.legacy = source;
                        self.outcome(outcome, false)
                    })
                } else {
                    self.initialize(false)
                };
                if let Err(error) = result {
                    self.error(request, Operation::Recovery, error);
                }
                return false;
            }
            Command::Settings { revision, value } => {
                if let Some(state) = &mut self.active {
                    state.settings = value;
                    state.dirty = Some(revision);
                    state.due = Some(Instant::now() + Duration::from_millis(300));
                } else {
                    self.error(revision, Operation::Settings, "数据尚未载入。");
                }
                return false;
            }
            Command::Flush { request } => {
                if self.flush_settings() {
                    self.emit(Event::Flushed { request });
                }
                return false;
            }
            Command::Shutdown { request } => {
                if !self.flush_settings() {
                    self.error(
                        request,
                        Operation::Shutdown,
                        "设置尚未保存，请重试保存后退出。",
                    );
                    return false;
                }
                self.active.take();
                self.emit(Event::Stopped { request });
                return true;
            }
            _ => {}
        }
        let Some(state) = &mut self.active else {
            self.error(0, Operation::Initialize, "数据尚未载入。");
            return false;
        };
        let result: Result<Event, (u64, Operation, String)> = (|| {
            Ok(match command {
                Command::Fence { request, value } => {
                    state
                        .database
                        .set_fence(value)
                        .map_err(|error| (request, Operation::Fence, error.to_string()))?;
                    Event::FenceSet { request }
                }
                Command::Expect { request, key } => {
                    let accepted = state
                        .database
                        .expect(&key)
                        .map_err(|error| (request, Operation::Expect, error.to_string()))?;
                    Event::Expected {
                        request,
                        key,
                        accepted,
                    }
                }
                Command::Revise { request, key } => {
                    state.database.revise(&key);
                    return Ok(Event::Flushed { request });
                }
                Command::Finished { request, key } => {
                    state.database.finished(&key);
                    return Ok(Event::Flushed { request });
                }
                Command::Save {
                    request,
                    key,
                    value,
                } => {
                    let result = state
                        .database
                        .save(&key, &value)
                        .map_err(|error| (request, Operation::History, error.to_string()))?;
                    Event::Saved {
                        request,
                        key,
                        result,
                    }
                }
                Command::Pending { request, task } => {
                    let accepted = state
                        .database
                        .pending(&task)
                        .map_err(|error| (request, Operation::Pending, error.to_string()))?;
                    Event::PendingStored {
                        request,
                        key: task.key,
                        accepted,
                    }
                }
                Command::TakePending { request, limit } => {
                    let tasks = state
                        .database
                        .take_pending(limit)
                        .map_err(|error| (request, Operation::Pending, error.to_string()))?;
                    let remaining = state
                        .database
                        .pending_count()
                        .map_err(|error| (request, Operation::Pending, error.to_string()))?;
                    Event::PendingTaken {
                        request,
                        tasks,
                        remaining,
                    }
                }
                Command::Page {
                    request,
                    query,
                    page,
                    size,
                } => {
                    let value = state
                        .database
                        .page(&query, page, size)
                        .map_err(|error| (request, Operation::Page, error.to_string()))?;
                    Event::Page { request, value }
                }
                Command::Export { request, path } => {
                    let rows = export::all_history(&state.database, &path)
                        .map_err(|error| (request, Operation::Export, error.to_string()))?;
                    Event::Exported {
                        request,
                        path,
                        rows,
                    }
                }
                Command::Delete { request } => {
                    state
                        .database
                        .clear()
                        .map_err(|error| (request, Operation::Delete, error.to_string()))?;
                    Event::Deleted { request }
                }
                _ => unreachable!("lifecycle/settings commands handled above"),
            })
        })();
        match result {
            Ok(event) => self.emit(event),
            Err((request, operation, message)) => self.error(request, operation, message),
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../scratch/actor-tests")
            .join(format!(
                "{}-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
        fs::create_dir_all(&root).unwrap();
        root.canonicalize().unwrap()
    }
    fn cleanup(root: &std::path::Path) {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../scratch/actor-tests")
            .canonicalize()
            .unwrap();
        let actual = root.canonicalize().unwrap();
        assert_eq!(actual.parent(), Some(parent.as_path()));
        fs::remove_dir_all(actual).unwrap();
    }
    #[test]
    fn first_use_does_not_initialize_data_until_choice_and_flush_keeps_the_last_edit() {
        let root = root();
        let data = root.join("data");
        let legacy = root.join("legacy");
        fs::create_dir(&data).unwrap();
        fs::create_dir(&legacy).unwrap();
        let (events, receiver) = mpsc::sync_channel(64);
        let callback = Arc::new(move |event| {
            let _ = events.send(event);
        });
        let storage = Storage::start(
            data.clone(),
            legacy,
            Screen::new(1920.0, 1080.0).unwrap(),
            "actor-fixture".into(),
            callback,
        )
        .unwrap();
        let client = storage.client();
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::FirstUse
        ));
        assert!(!data.join("setting.json").exists());
        assert!(!data.join("translation_history.db").exists());
        client.submit(Command::UseDefaults { request: 1 }).unwrap();
        let Event::Ready { settings, .. } = receiver.recv_timeout(Duration::from_secs(3)).unwrap()
        else {
            panic!("ready")
        };
        for revision in 1..=5 {
            let value = settings
                .with_prompt(&format!("Edit {revision} to {{0}}"))
                .unwrap();
            client
                .submit(Command::Settings { revision, value })
                .unwrap();
        }
        client.submit(Command::Flush { request: 99 }).unwrap();
        let Event::SettingsSaved { revision, .. } =
            receiver.recv_timeout(Duration::from_secs(3)).unwrap()
        else {
            panic!("saved")
        };
        assert_eq!(revision, 5);
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::Flushed { request: 99 }
        ));
        client.submit(Command::Shutdown { request: 100 }).unwrap();
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::Stopped { request: 100 }
        ));
        drop(client);
        storage.join().unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(data.join("setting.json")).unwrap()).unwrap();
        assert_eq!(value["Prompt"], "Edit 5 to {0}");
        cleanup(&root);
    }
    #[test]
    fn full_command_channel_returns_the_original_command_instead_of_dropping_it() {
        let (sender, _receiver) = mpsc::sync_channel(CAPACITY);
        let client = Client { sender };
        for request in 0..CAPACITY as u64 {
            client.submit(Command::Flush { request }).unwrap();
        }
        let error = client.submit(Command::Flush { request: 999 }).unwrap_err();
        assert!(error.queue_full);
        assert!(matches!(*error.command, Command::Flush { request: 999 }));
    }
    #[test]
    fn restarted_migration_retries_the_selected_origin_and_unknown_origin_needs_a_choice() {
        let root = root();
        let data = root.join("data");
        let selected = root.join("selected-old-folder");
        let decoy = root.join("executable-folder");
        for path in [&data, &selected, &decoy] {
            fs::create_dir(path).unwrap();
        }
        fs::write(selected.join("setting.json"), b"invalid config fixture").unwrap();
        let db = rusqlite::Connection::open(selected.join("translation_history.db")).unwrap();
        db.execute_batch(crate::history::SCHEMA).unwrap();
        db.execute("INSERT INTO TranslationHistory(Timestamp,SourceText,TranslatedText,TargetLanguage,ApiUsed) VALUES('1700000000','Selected origin sentence','测试译文','zh-CN','Google')", []).unwrap();
        drop(db);
        let screen = Screen::new(1920.0, 1080.0).unwrap();
        let (sender, events) = mpsc::sync_channel(64);
        let storage = Storage::start(
            data.clone(),
            decoy.clone(),
            screen,
            "first-worker".into(),
            Arc::new(move |event| {
                sender.send(event).unwrap();
            }),
        )
        .unwrap();
        let client = storage.client();
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::FirstUse
        ));
        client
            .submit(Command::Import {
                request: 1,
                directory: selected.clone(),
            })
            .unwrap();
        let Event::RecoveryRequired {
            kind: Recovery::Migration,
            location: stage,
            ..
        } = events.recv_timeout(Duration::from_secs(3)).unwrap()
        else {
            panic!("migration recovery");
        };
        client.submit(Command::Shutdown { request: 2 }).unwrap();
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::Stopped { .. }
        ));
        drop(client);
        storage.join().unwrap();

        fs::write(
            selected.join("setting.json"),
            br#"{"TargetLanguage":"expected-selected-origin"}"#,
        )
        .unwrap();
        fs::write(
            decoy.join("setting.json"),
            br#"{"TargetLanguage":"wrong-origin"}"#,
        )
        .unwrap();
        let (sender, events) = mpsc::sync_channel(64);
        let storage = Storage::start(
            data.clone(),
            decoy,
            screen,
            "restarted-worker".into(),
            Arc::new(move |event| {
                sender.send(event).unwrap();
            }),
        )
        .unwrap();
        let client = storage.client();
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::RecoveryRequired {
                kind: Recovery::Migration,
                ..
            }
        ));
        let manifest_path = stage.join("migration.json");
        let original_manifest = fs::read(&manifest_path).unwrap();
        let mut old_manifest: serde_json::Value =
            serde_json::from_slice(&original_manifest).unwrap();
        old_manifest.as_object_mut().unwrap().remove("source");
        fs::write(&manifest_path, serde_json::to_vec(&old_manifest).unwrap()).unwrap();
        client
            .submit(Command::Retry {
                request: 3,
                directory: None,
            })
            .unwrap();
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::Error {
                request: 3,
                operation: Operation::Recovery,
                ..
            }
        ));
        assert!(
            stage.is_dir(),
            "unknown source must not archive or substitute a folder"
        );
        fs::write(&manifest_path, &original_manifest).unwrap();
        client
            .submit(Command::Retry {
                request: 4,
                directory: None,
            })
            .unwrap();
        let Event::Ready {
            settings,
            migration: Some(report),
            ..
        } = events.recv_timeout(Duration::from_secs(3)).unwrap()
        else {
            panic!("resumed selected source");
        };
        assert_eq!(settings.target_language(), "expected-selected-origin");
        assert_eq!(report.migrated_rows, 1);
        client
            .submit(Command::Page {
                request: 5,
                query: String::new(),
                page: 1,
                size: 30,
            })
            .unwrap();
        let Event::Page { value, .. } = events.recv_timeout(Duration::from_secs(3)).unwrap() else {
            panic!("history");
        };
        assert_eq!(value.total, 1);
        assert_eq!(value.rows[0].source, "Selected origin sentence");
        client.submit(Command::Shutdown { request: 6 }).unwrap();
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::Stopped { .. }
        ));
        drop(client);
        storage.join().unwrap();
        cleanup(&root);
    }
}
