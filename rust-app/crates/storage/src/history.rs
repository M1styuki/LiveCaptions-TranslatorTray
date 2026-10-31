use captions_core::{scheduler::Task, session::WorkKey};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::{collections::HashMap, fmt, path::Path, time::Duration};

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS TranslationHistory (
 Id INTEGER PRIMARY KEY AUTOINCREMENT,Timestamp TEXT,SourceText TEXT,
 TranslatedText TEXT,TargetLanguage TEXT,ApiUsed TEXT
);
CREATE TABLE IF NOT EXISTS RustMigration (
 SourceKey TEXT PRIMARY KEY,ImportedAt INTEGER NOT NULL,SchemaVersion INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS RustSentenceMap (
 InstanceId TEXT NOT NULL,SessionId INTEGER NOT NULL,SentenceId INTEGER NOT NULL,
 Revision INTEGER NOT NULL,HistoryId INTEGER NOT NULL,
 PRIMARY KEY(InstanceId,SessionId,SentenceId),
 FOREIGN KEY(HistoryId) REFERENCES TranslationHistory(Id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS PendingTranslation (
 InstanceId TEXT NOT NULL,SessionId INTEGER NOT NULL,SentenceId INTEGER NOT NULL,
 Revision INTEGER NOT NULL,ConfigRevision INTEGER NOT NULL,SourceText TEXT NOT NULL,
 CreatedAt INTEGER NOT NULL,Sequence INTEGER NOT NULL,
 PRIMARY KEY(InstanceId,SessionId,SentenceId)
);
CREATE INDEX IF NOT EXISTS RustHistoryTimestampOrder
 ON TranslationHistory(CAST(Timestamp AS INTEGER) DESC,Id DESC);
"#;
#[derive(Debug)]
pub enum Error {
    Sql(rusqlite::Error),
    Io(std::io::Error),
    InvalidId,
    Backpressure,
}
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sql(error) => write!(f, "数据库操作失败（{:?}）。", error.sqlite_error_code()),
            Self::Io(error) => write!(f, "文件读写失败：{error}"),
            Self::InvalidId => f.write_str("数据库标识超出有效范围。"),
            Self::Backpressure => f.write_str("数据处理未完成，请稍候再试。"),
        }
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, PartialEq, Eq)]
pub struct Fence {
    pub instance_id: String,
    pub session_id: u64,
    pub config_revision: u64,
}
impl Fence {
    pub fn matches(&self, key: &WorkKey) -> bool {
        self.instance_id == key.instance_id
            && self.session_id == key.session_id
            && self.config_revision == key.config_revision
    }
}
#[derive(Clone, Serialize)]
pub struct Row {
    pub id: i64,
    pub timestamp: String,
    pub source: String,
    pub translation: String,
    pub target_language: String,
    pub api_used: String,
}
#[derive(Clone)]
pub struct Record {
    pub timestamp: i64,
    pub source: String,
    pub translation: String,
    pub target_language: String,
    pub api_used: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Save {
    Ignored,
    AlreadyPresent(i64),
    Written(i64),
}
pub struct Page {
    pub rows: Vec<Row>,
    pub total: u64,
    pub page: u64,
    pub pages: u64,
    pub page_size: u16,
}

pub struct Database {
    pub(crate) connection: Connection,
    instance: String,
    fence: Option<Fence>,
    expected: HashMap<u64, u64>,
}
fn valid(key: &WorkKey) -> Result<()> {
    if [
        key.session_id,
        key.sentence_id,
        key.revision,
        key.config_revision,
    ]
    .into_iter()
    .any(|id| id > i64::MAX as u64)
    {
        return Err(Error::InvalidId);
    }
    Ok(())
}
fn sql_id(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::InvalidId)
}
fn unsigned(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}
fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        id: row.get(0)?,
        timestamp: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        source: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        translation: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        target_language: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        api_used: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
    })
}
impl Database {
    pub fn open(path: &Path, instance: String) -> Result<Self> {
        Self::initialize(Connection::open(path)?, instance)
    }
    fn initialize(mut connection: Connection, instance: String) -> Result<Self> {
        connection.busy_timeout(Duration::from_millis(5000))?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=DELETE;")?;
        connection.execute_batch(SCHEMA)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM PendingTranslation WHERE InstanceId<>?1",
            [&instance],
        )?;
        transaction.execute(
            "DELETE FROM RustSentenceMap WHERE InstanceId<>?1",
            [&instance],
        )?;
        transaction.commit()?;
        Ok(Self {
            connection,
            instance,
            fence: None,
            expected: HashMap::new(),
        })
    }
    /// Commands share one bounded FIFO. The controller invalidates its session
    /// before posting this fence, so a result arriving later cannot be submitted;
    /// the DB thread independently checks the fence and expected revision again.
    pub fn set_fence(&mut self, fence: Option<Fence>) -> Result<()> {
        if fence.as_ref().is_some_and(|fence| {
            fence.instance_id != self.instance
                || fence.session_id > i64::MAX as u64
                || fence.config_revision > i64::MAX as u64
        }) {
            return Err(Error::InvalidId);
        }
        self.expected.clear();
        self.fence = fence;
        if let Some(fence) = &self.fence {
            self.connection.execute("DELETE FROM PendingTranslation WHERE InstanceId=?1 AND (SessionId<>?2 OR ConfigRevision<>?3)",params![self.instance,sql_id(fence.session_id)?,sql_id(fence.config_revision)?])?;
        } else {
            self.connection.execute(
                "DELETE FROM PendingTranslation WHERE InstanceId=?1",
                [&self.instance],
            )?;
        }
        Ok(())
    }
    /// Register only tasks entering HTTP/log-only processing, never the unbounded
    /// on-disk backlog. A revised waiting snapshot updates an existing entry.
    pub fn expect(&mut self, key: &WorkKey) -> Result<bool> {
        valid(key)?;
        if !self.fence.as_ref().is_some_and(|fence| fence.matches(key)) {
            return Ok(false);
        }
        if self.expected.len() == 64 && !self.expected.contains_key(&key.sentence_id) {
            return Err(Error::Backpressure);
        }
        let expected = self.expected.entry(key.sentence_id).or_insert(key.revision);
        *expected = (*expected).max(key.revision);
        Ok(true)
    }
    pub fn revise(&mut self, key: &WorkKey) {
        if self.fence.as_ref().is_some_and(|fence| fence.matches(key))
            && let Some(revision) = self.expected.get_mut(&key.sentence_id)
        {
            *revision = (*revision).max(key.revision);
        }
    }
    pub fn finished(&mut self, key: &WorkKey) {
        if !self.fence.as_ref().is_some_and(|fence| fence.matches(key)) {
            return;
        }
        if self.expected.get(&key.sentence_id) == Some(&key.revision) {
            self.expected.remove(&key.sentence_id);
        }
    }
    pub fn save(&mut self, key: &WorkKey, record: &Record) -> Result<Save> {
        valid(key)?;
        if !self.fence.as_ref().is_some_and(|fence| fence.matches(key))
            || self.expected.get(&key.sentence_id) != Some(&key.revision)
        {
            return Ok(Save::Ignored);
        }
        let transaction = self.connection.transaction()?;
        let mapped:Option<(i64,u64)>=transaction.query_row("SELECT HistoryId,Revision FROM RustSentenceMap WHERE InstanceId=?1 AND SessionId=?2 AND SentenceId=?3",params![key.instance_id,sql_id(key.session_id)?,sql_id(key.sentence_id)?],|row|Ok((row.get(0)?,unsigned(row,1)?))).optional()?;
        let history_id = if let Some((history_id, revision)) = mapped {
            if revision >= key.revision {
                return Ok(Save::AlreadyPresent(history_id));
            }
            transaction.execute("UPDATE TranslationHistory SET Timestamp=?1,SourceText=?2,TranslatedText=?3,TargetLanguage=?4,ApiUsed=?5 WHERE Id=?6",params![record.timestamp.to_string(),record.source,record.translation,record.target_language,record.api_used,history_id])?;
            history_id
        } else {
            transaction.execute("INSERT INTO TranslationHistory(Timestamp,SourceText,TranslatedText,TargetLanguage,ApiUsed) VALUES(?1,?2,?3,?4,?5)",params![record.timestamp.to_string(),record.source,record.translation,record.target_language,record.api_used])?;
            transaction.last_insert_rowid()
        };
        transaction.execute("INSERT INTO RustSentenceMap(InstanceId,SessionId,SentenceId,Revision,HistoryId) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(InstanceId,SessionId,SentenceId) DO UPDATE SET Revision=excluded.Revision,HistoryId=excluded.HistoryId",params![key.instance_id,sql_id(key.session_id)?,sql_id(key.sentence_id)?,sql_id(key.revision)?,history_id])?;
        transaction.commit()?;
        Ok(Save::Written(history_id))
    }
    pub fn pending(&mut self, task: &Task) -> Result<bool> {
        valid(&task.key)?;
        if task.sequence > i64::MAX as u64 {
            return Err(Error::InvalidId);
        }
        if !self
            .fence
            .as_ref()
            .is_some_and(|fence| fence.matches(&task.key))
        {
            return Ok(false);
        }
        self.connection.execute("INSERT INTO PendingTranslation(InstanceId,SessionId,SentenceId,Revision,ConfigRevision,SourceText,CreatedAt,Sequence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(InstanceId,SessionId,SentenceId) DO UPDATE SET Revision=excluded.Revision,ConfigRevision=excluded.ConfigRevision,SourceText=excluded.SourceText,CreatedAt=excluded.CreatedAt,Sequence=MIN(PendingTranslation.Sequence,excluded.Sequence) WHERE excluded.Revision>PendingTranslation.Revision",params![task.key.instance_id,sql_id(task.key.session_id)?,sql_id(task.key.sentence_id)?,sql_id(task.key.revision)?,sql_id(task.key.config_revision)?,task.source,task.created_at,sql_id(task.sequence)?])?;
        Ok(true)
    }
    pub fn take_pending(&mut self, limit: usize) -> Result<Vec<Task>> {
        let Some(fence) = self.fence.clone() else {
            return Ok(Vec::new());
        };
        let transaction = self.connection.transaction()?;
        let tasks = {
            let mut query=transaction.prepare("SELECT SentenceId,Revision,SourceText,CreatedAt,Sequence FROM PendingTranslation WHERE InstanceId=?1 AND SessionId=?2 AND ConfigRevision=?3 ORDER BY Sequence,SentenceId LIMIT ?4")?;
            query
                .query_map(
                    params![
                        fence.instance_id,
                        sql_id(fence.session_id)?,
                        sql_id(fence.config_revision)?,
                        limit.min(32) as i64
                    ],
                    |row| {
                        Ok(Task {
                            key: WorkKey {
                                instance_id: fence.instance_id.clone(),
                                session_id: fence.session_id,
                                config_revision: fence.config_revision,
                                sentence_id: unsigned(row, 0)?,
                                revision: unsigned(row, 1)?,
                            },
                            source: row.get(2)?,
                            complete: true,
                            created_at: row.get(3)?,
                            sequence: unsigned(row, 4)?,
                        })
                    },
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for task in &tasks {
            transaction.execute("DELETE FROM PendingTranslation WHERE InstanceId=?1 AND SessionId=?2 AND SentenceId=?3 AND Revision=?4",params![task.key.instance_id,sql_id(task.key.session_id)?,sql_id(task.key.sentence_id)?,sql_id(task.key.revision)?])?;
        }
        transaction.commit()?;
        Ok(tasks)
    }
    pub fn pending_count(&self) -> Result<u64> {
        let Some(fence) = &self.fence else {
            return Ok(0);
        };
        Ok(self.connection.query_row("SELECT COUNT(*) FROM PendingTranslation WHERE InstanceId=?1 AND SessionId=?2 AND ConfigRevision=?3",params![fence.instance_id,sql_id(fence.session_id)?,sql_id(fence.config_revision)?],|row|unsigned(row,0))?)
    }
    pub fn page(&self, query: &str, page: u64, page_size: u16) -> Result<Page> {
        let page_size = if [20, 30, 50, 100].contains(&page_size) {
            page_size
        } else {
            30
        };
        let pattern = format!("%{query}%");
        let total:u64=self.connection.query_row("SELECT COUNT(*) FROM TranslationHistory WHERE COALESCE(SourceText,'') LIKE ?1 OR COALESCE(TranslatedText,'') LIKE ?1",[&pattern],|row|unsigned(row,0))?;
        let pages = total.div_ceil(page_size as u64).max(1);
        let page = page.clamp(1, pages);
        let offset = (page - 1) * page_size as u64;
        let mut statement=self.connection.prepare("SELECT Id,Timestamp,SourceText,TranslatedText,TargetLanguage,ApiUsed FROM TranslationHistory WHERE COALESCE(SourceText,'') LIKE ?1 OR COALESCE(TranslatedText,'') LIKE ?1 ORDER BY CAST(Timestamp AS INTEGER) DESC,Id DESC LIMIT ?2 OFFSET ?3")?;
        let rows = statement
            .query_map(params![pattern, page_size, sql_id(offset)?], row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Page {
            rows,
            total,
            page,
            pages,
            page_size,
        })
    }
    pub fn all(&self, mut consume: impl FnMut(Row) -> Result<()>) -> Result<()> {
        let mut statement=self.connection.prepare("SELECT Id,Timestamp,SourceText,TranslatedText,TargetLanguage,ApiUsed FROM TranslationHistory ORDER BY CAST(Timestamp AS INTEGER) DESC,Id DESC")?;
        let mut rows = statement.query([])?;
        while let Some(value) = rows.next()? {
            consume(row(value)?)?;
        }
        Ok(())
    }
    pub fn clear(&mut self) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM PendingTranslation", [])?;
        transaction.execute("DELETE FROM TranslationHistory", [])?;
        transaction.execute("DELETE FROM RustSentenceMap", [])?;
        transaction.commit()?;
        self.fence = None;
        self.expected.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_clear_preserves_rows_and_in_memory_fence_for_later_save() {
        let root = std::env::temp_dir().join(format!(
            "LiveCaptions-clear-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let mut db = Database::open(&root.join("fixture.db"), "test-instance".into()).unwrap();
        db.set_fence(Some(Fence {
            instance_id: "test-instance".into(),
            session_id: 1,
            config_revision: 0,
        }))
        .unwrap();
        assert!(db.expect(&key(1, 1)).unwrap());
        assert!(matches!(
            db.save(&key(1, 1), &record("old")).unwrap(),
            Save::Written(_)
        ));
        assert!(db.expect(&key(2, 1)).unwrap());
        db.connection.execute_batch("CREATE TRIGGER fixture_clear_failure BEFORE DELETE ON TranslationHistory BEGIN SELECT RAISE(ABORT,'controlled fixture failure'); END;").unwrap();
        assert!(db.clear().is_err());
        assert_eq!(db.page("", 1, 30).unwrap().total, 1);
        assert!(
            matches!(
                db.save(&key(2, 1), &record("new")).unwrap(),
                Save::Written(_)
            ),
            "failed transaction must not clear its live fence/expected map"
        );
        assert_eq!(db.page("", 1, 30).unwrap().total, 2);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    fn key(sentence: u64, revision: u64) -> WorkKey {
        WorkKey {
            instance_id: "test-instance".into(),
            session_id: 1,
            sentence_id: sentence,
            revision,
            config_revision: 0,
        }
    }
    fn record(value: &str) -> Record {
        Record {
            timestamp: 123,
            source: value.into(),
            translation: format!("译文 {value}"),
            target_language: "zh-CN".into(),
            api_used: "Google".into(),
        }
    }
    fn database() -> Database {
        let mut db = Database::initialize(
            Connection::open_in_memory().unwrap(),
            "test-instance".into(),
        )
        .unwrap();
        db.set_fence(Some(Fence {
            instance_id: "test-instance".into(),
            session_id: 1,
            config_revision: 0,
        }))
        .unwrap();
        db
    }
    #[test]
    fn same_sentence_updates_its_id_and_delayed_old_results_never_touch_another_row() {
        let mut db = database();
        let first = key(1, 1);
        db.expect(&first).unwrap();
        let Save::Written(id) = db.save(&first, &record("A")).unwrap() else {
            panic!("first write")
        };
        let second = key(2, 1);
        db.expect(&second).unwrap();
        db.save(&second, &record("B")).unwrap();
        let revised = key(1, 2);
        db.revise(&revised);
        assert_eq!(db.save(&first, &record("STALE")).unwrap(), Save::Ignored);
        assert_eq!(
            db.save(&revised, &record("A revised")).unwrap(),
            Save::Written(id)
        );
        assert_eq!(
            db.save(&revised, &record("DUPLICATE")).unwrap(),
            Save::AlreadyPresent(id)
        );
        let page = db.page("", 1, 30).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.rows[0].source, "B");
        assert_eq!(page.rows[1].source, "A revised");
        db.set_fence(None).unwrap();
        assert_eq!(
            db.save(&second, &record("after stop")).unwrap(),
            Save::Ignored
        );
        db.set_fence(Some(Fence {
            instance_id: "test-instance".into(),
            session_id: 2,
            config_revision: 1,
        }))
        .unwrap();
        assert_eq!(
            db.save(&revised, &record("old session")).unwrap(),
            Save::Ignored
        );
    }
    #[test]
    fn thirty_one_same_second_rows_page_stably_and_search_is_parameterized() {
        let mut db = database();
        for sentence in 1..=31 {
            let key = key(sentence, 1);
            db.expect(&key).unwrap();
            db.save(&key, &record(&format!("source {sentence}")))
                .unwrap();
            db.finished(&key);
        }
        let first = db.page("", 1, 30).unwrap();
        let second = db.page("", 2, 30).unwrap();
        assert_eq!(
            (first.pages, first.rows.len(), second.rows.len()),
            (2, 30, 1)
        );
        assert!(first.rows.windows(2).all(|rows| rows[0].id > rows[1].id));
        assert_eq!(second.rows[0].source, "source 1");
        assert_eq!(db.page("' OR 1=1 --", 1, 30).unwrap().total, 0);
        assert_eq!(db.page("source _", 1, 30).unwrap().total, 31);
        assert_eq!(db.page("%", 999, 20).unwrap().page, 2);
        let late = key(31, 2);
        db.expect(&late).unwrap();
        db.clear().unwrap();
        assert_eq!(
            db.save(&late, &record("must not resurrect")).unwrap(),
            Save::Ignored
        );
        let empty = db.page("", 8, 30).unwrap();
        assert_eq!((empty.page, empty.pages, empty.total), (1, 1, 0));
    }
    #[test]
    fn overflow_rows_are_real_sqlite_rows_in_fifo_order_and_stop_clears_them() {
        let mut db = database();
        for sentence in 33..=40 {
            let key = key(sentence, 1);
            db.pending(&Task {
                key,
                source: format!("source {sentence}"),
                complete: true,
                sequence: sentence,
                created_at: 123,
            })
            .unwrap();
        }
        assert_eq!(db.pending_count().unwrap(), 8);
        let mut revised = Task {
            key: key(33, 2),
            source: "latest revision".into(),
            complete: true,
            sequence: 999,
            created_at: 124,
        };
        db.pending(&revised).unwrap();
        revised.key.revision = 1;
        revised.source = "stale".into();
        db.pending(&revised).unwrap();
        let restored = db.take_pending(3).unwrap();
        assert_eq!(
            restored
                .iter()
                .map(|task| task.key.sentence_id)
                .collect::<Vec<_>>(),
            vec![33, 34, 35]
        );
        assert_eq!(restored[0].source, "latest revision");
        assert_eq!(restored[0].sequence, 33);
        assert_eq!(db.pending_count().unwrap(), 5);
        db.set_fence(None).unwrap();
        assert_eq!(
            db.connection
                .query_row("SELECT COUNT(*) FROM PendingTranslation", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn forty_sentence_scheduler_uses_real_sqlite_overflow_and_keeps_every_completed_sentence() {
        use captions_core::{
            scheduler::{MAX_COMPLETE_WAITING, Scheduler},
            session::Session,
        };
        use std::collections::VecDeque;
        let mut database = database();
        let mut scheduler = Scheduler::default();
        let mut session = Session::new("test-instance".into());
        session.start();
        let mut running = VecDeque::new();
        for sentence in 1..=40 {
            let key = session.observe(sentence, 1).unwrap();
            let task = Task {
                key,
                source: format!("Complete sentence {sentence}."),
                complete: true,
                sequence: sentence,
                created_at: 123,
            };
            if let Some(spilled) = scheduler.offer(task).spill {
                assert!(database.pending(&spilled).unwrap());
                session.forget(&spilled.key);
            }
            while let Some(task) = scheduler.next_ready() {
                database.expect(&task.key).unwrap();
                running.push_back(task);
            }
            assert!(scheduler.in_flight() <= 2);
            assert!(scheduler.waiting() <= 32);
        }
        assert_eq!(database.pending_count().unwrap(), 6);
        assert_eq!(scheduler.waiting(), 32);
        let mut completed = 0_usize;
        while let Some(task) = if completed.is_multiple_of(2) {
            running.pop_back()
        } else {
            running.pop_front()
        } {
            assert!(session.accepts(&task.key));
            let value = Record {
                timestamp: task.created_at,
                source: task.source.clone(),
                translation: format!("译文 {}", task.key.sentence_id),
                target_language: "zh-CN".into(),
                api_used: "Fixture".into(),
            };
            assert!(matches!(
                database.save(&task.key, &value).unwrap(),
                Save::Written(_)
            ));
            database.finished(&task.key);
            scheduler.finished(&task.key);
            session.forget(&task.key);
            completed += 1;
            let capacity = MAX_COMPLETE_WAITING - scheduler.waiting();
            for restored in database.take_pending(capacity).unwrap() {
                assert!(session.retain(&restored.key));
                scheduler.restore(restored).unwrap();
            }
            if database.pending_count().unwrap() == 0 {
                scheduler.storage_drained();
            }
            while let Some(task) = scheduler.next_ready() {
                database.expect(&task.key).unwrap();
                running.push_back(task);
            }
            assert!(scheduler.in_flight() <= 2);
            assert!(scheduler.waiting() <= 32);
        }
        assert_eq!(completed, 40);
        assert_eq!(database.page("", 1, 100).unwrap().total, 40);
        assert_eq!(database.pending_count().unwrap(), 0);
    }
    #[test]
    fn old_finished_notification_cannot_remove_new_session_or_config_registration() {
        let mut db = database();
        let old = key(1, 1);
        db.expect(&old).unwrap();
        let mut current = old.clone();
        current.session_id = 2;
        db.set_fence(Some(Fence {
            instance_id: current.instance_id.clone(),
            session_id: 2,
            config_revision: 0,
        }))
        .unwrap();
        db.expect(&current).unwrap();
        db.finished(&old);
        assert!(matches!(
            db.save(&current, &record("new session")).unwrap(),
            Save::Written(_)
        ));
        let previous = current.clone();
        current.config_revision = 1;
        current.sentence_id = 2;
        db.set_fence(Some(Fence {
            instance_id: current.instance_id.clone(),
            session_id: 2,
            config_revision: 1,
        }))
        .unwrap();
        db.expect(&current).unwrap();
        let mut old_config = previous;
        old_config.sentence_id = 2;
        db.finished(&old_config);
        assert!(matches!(
            db.save(&current, &record("new config")).unwrap(),
            Save::Written(_)
        ));
    }
}
