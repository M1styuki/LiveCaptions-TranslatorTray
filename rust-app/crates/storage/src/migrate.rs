//! Import only into uninitialized data. Original files are opened read-only;
//! raw backups and a WAL-aware SQLite snapshot remain with the user's data.
use crate::{
    atomic_file,
    history::{self, SCHEMA},
    timestamp,
};
use captions_core::settings::{Screen, Settings};
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
    params,
    types::ValueRef,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileHash {
    pub name: String,
    pub sha256: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Report {
    pub source_key: String,
    pub source_files: Vec<FileHash>,
    pub snapshot_sha256: Option<String>,
    pub imported_database_sha256: Option<String>,
    pub imported_settings_sha256: Option<String>,
    pub original_rows: u64,
    pub migrated_rows: u64,
    pub converted_timestamps: u64,
    pub assumed_local_timestamps: u64,
    pub ambiguous_local_timestamps: u64,
    pub unparseable_timestamps: u64,
    pub preserved_cells_sha256: String,
    pub used_default_settings: bool,
    pub repaired_fields: Vec<String>,
    pub previous_api_name: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    schema_version: u32,
    target: PathBuf,
    #[serde(default)]
    source: Option<PathBuf>,
    phase: String,
    report: Report,
}
pub enum Outcome {
    AlreadyInitialized,
    NoLegacyFiles,
    Imported(Report),
    NeedsRecovery { stage: PathBuf, message: String },
}
fn failure(message: &str) -> io::Error {
    io::Error::other(message.to_owned())
}
fn db_error(error: rusqlite::Error) -> io::Error {
    io::Error::other(history::Error::from(error).to_string())
}
fn hexadecimal(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub fn sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hexadecimal(digest.finalize()))
}
fn source_hashes(source: &Path) -> io::Result<Vec<FileHash>> {
    let mut result = Vec::new();
    for name in [
        "setting.json",
        "translation_history.db",
        "translation_history.db-wal",
    ] {
        let path = source.join(name);
        if path.is_file() {
            result.push(FileHash {
                name: name.into(),
                sha256: sha256(&path)?,
            });
        }
    }
    Ok(result)
}
fn copy_new(source: &Path, destination: &Path) -> io::Result<()> {
    let mut source = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    io::copy(&mut source, &mut output)?;
    output.flush()?;
    output.sync_all()
}
pub fn stage_path(data: &Path) -> io::Result<PathBuf> {
    let parent = data
        .parent()
        .ok_or_else(|| failure("数据目录缺少父目录。"))?;
    let name = data
        .file_name()
        .ok_or_else(|| failure("数据目录名称无效。"))?;
    let mut stage = std::ffi::OsString::from(".");
    stage.push(name);
    stage.push(".migration-staging");
    Ok(parent.join(stage))
}
fn initialized(data: &Path) -> bool {
    data.join("setting.json").exists()
        || data.join("translation_history.db").exists()
        || data.join(".initialized").exists()
}
fn empty(data: &Path) -> io::Result<bool> {
    if !data.exists() {
        return Ok(true);
    }
    Ok(fs::read_dir(data)?.next().is_none())
}
fn manifest(stage: &Path) -> io::Result<Manifest> {
    let bytes = fs::read(stage.join("migration.json"))?;
    serde_json::from_slice(&bytes).map_err(|_| failure("迁移记录损坏；保留副本未改动。"))
}
fn save_manifest(stage: &Path, manifest: &Manifest) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(io::Error::other)?;
    atomic_file::write(&stage.join("migration.json"), |file| file.write_all(&bytes))
}

fn snapshot(source: &Path, target: &Path) -> io::Result<()> {
    let source = Connection::open_with_flags(
        source,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(db_error)?;
    source
        .busy_timeout(Duration::from_millis(5000))
        .map_err(db_error)?;
    let mut target = Connection::open(target).map_err(db_error)?;
    {
        let backup = Backup::new(&source, &mut target).map_err(db_error)?;
        let mut busy = 0u32;
        loop {
            match backup.step(64).map_err(db_error)? {
                StepResult::Done => break,
                StepResult::More => {}
                StepResult::Busy | StepResult::Locked => {
                    busy += 1;
                    if busy >= 500 {
                        return Err(failure("旧数据库仍被占用，请先退出旧程序后重试。"));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => return Err(failure("数据库快照未能完成。")),
            }
        }
    }
    // Only the new snapshot is changed here. It must be self-contained before
    // it is copied; never checkpoint or change journal mode on the source.
    target
        .execute_batch("PRAGMA journal_mode=DELETE;")
        .map_err(db_error)?;
    target.close().map_err(|(_, error)| db_error(error))?;
    Ok(())
}
fn cells(connection: &Connection) -> io::Result<(u64, String)> {
    let mut digest = Sha256::new();
    let mut count = 0u64;
    let mut query=connection.prepare("SELECT Id,SourceText,TranslatedText,TargetLanguage,ApiUsed FROM TranslationHistory ORDER BY Id").map_err(db_error)?;
    let mut rows = query.query([]).map_err(db_error)?;
    while let Some(row) = rows.next().map_err(db_error)? {
        count += 1;
        for column in 0..5 {
            let value = row.get_ref(column).map_err(db_error)?;
            match value {
                ValueRef::Null => digest.update([0]),
                ValueRef::Integer(value) => {
                    digest.update([1]);
                    digest.update(value.to_le_bytes());
                }
                ValueRef::Real(value) => {
                    digest.update([2]);
                    digest.update(value.to_le_bytes());
                }
                ValueRef::Text(value) => {
                    digest.update([3]);
                    digest.update((value.len() as u64).to_le_bytes());
                    digest.update(value);
                }
                ValueRef::Blob(value) => {
                    digest.update([4]);
                    digest.update((value.len() as u64).to_le_bytes());
                    digest.update(value);
                }
            }
        }
    }
    Ok((count, hexadecimal(digest.finalize())))
}
fn convert_database(
    path: &Path,
    source_key: &str,
    warnings: &Path,
    report: &mut Report,
) -> io::Result<()> {
    let mut connection = Connection::open(path).map_err(db_error)?;
    connection
        .busy_timeout(Duration::from_millis(5000))
        .map_err(db_error)?;
    // Read before creating tables: an unrelated/missing schema is an error,
    // rather than silently replacing the source with an empty history table.
    let (original_rows, before) = cells(&connection)?;
    report.original_rows = original_rows;
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=DELETE;")
        .map_err(db_error)?;
    connection.execute_batch(SCHEMA).map_err(db_error)?;
    let transaction = connection.transaction().map_err(db_error)?;
    let mut warnings = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(warnings)?;
    {
        let mut query = transaction
            .prepare("SELECT Id,Timestamp FROM TranslationHistory ORDER BY Id")
            .map_err(db_error)?;
        let mut rows = query.query([]).map_err(db_error)?;
        while let Some(row) = rows.next().map_err(db_error)? {
            let id: i64 = row.get(0).map_err(db_error)?;
            let raw = row.get_ref(1).map_err(db_error)?;
            let converted = raw.as_str().ok().and_then(timestamp::legacy);
            match converted {
                Some(value) => {
                    if !value.already_numeric {
                        transaction
                            .execute(
                                "UPDATE TranslationHistory SET Timestamp=?1 WHERE Id=?2",
                                params![value.unix_seconds.to_string(), id],
                            )
                            .map_err(db_error)?;
                        report.converted_timestamps += 1;
                    }
                    if value.assumed_local {
                        report.assumed_local_timestamps += 1;
                    }
                    if value.ambiguous_local {
                        report.ambiguous_local_timestamps += 1;
                        serde_json::to_writer(
                            &mut warnings,
                            &serde_json::json!({"row_id":id,"reason":"ambiguous local time; earliest UTC chosen"}),
                        )?;
                        warnings.write_all(b"\n")?;
                    }
                }
                None => {
                    report.unparseable_timestamps += 1;
                    serde_json::to_writer(
                        &mut warnings,
                        &serde_json::json!({"row_id":id,"reason":"unparseable timestamp retained unchanged"}),
                    )?;
                    warnings.write_all(b"\n")?;
                }
            }
        }
    }
    let (after_rows, after) = cells(&transaction)?;
    if after_rows != original_rows || after != before {
        return Err(failure("迁移校验不一致，未启用副本；旧数据保持原样。"));
    }
    transaction.execute("INSERT INTO RustMigration(SourceKey,ImportedAt,SchemaVersion) VALUES(?1,?2,1) ON CONFLICT(SourceKey) DO NOTHING",params![source_key,chrono::Utc::now().timestamp()]).map_err(db_error)?;
    transaction.commit().map_err(db_error)?;
    warnings.flush()?;
    warnings.sync_all()?;
    drop(warnings);
    connection.close().map_err(|(_, error)| db_error(error))?;
    report.migrated_rows = after_rows;
    report.preserved_cells_sha256 = after;
    Ok(())
}
fn validate_stage(data: &Path, stage: &Path, record: &Manifest) -> io::Result<()> {
    if record.schema_version != 1 || record.target != data || stage != stage_path(data)? {
        return Err(failure("迁移目录与记录不匹配，未改动任何文件。"));
    }
    validate_stage_location(data, stage)
}
fn validate_stage_location(data: &Path, stage: &Path) -> io::Result<()> {
    if !data.is_absolute() || !stage.is_absolute() || stage != stage_path(data)? || !stage.is_dir()
    {
        return Err(failure("恢复目录不在预期位置，未改动任何文件。"));
    }
    let parent = data
        .parent()
        .ok_or_else(|| failure("数据路径无效。"))?
        .canonicalize()?;
    let actual = stage.canonicalize()?;
    if actual.parent() != Some(parent.as_path()) || actual.file_name() != stage.file_name() {
        return Err(failure("迁移副本不在预期目录。"));
    }
    Ok(())
}
fn commit(data: &Path, stage: &Path, record: &Manifest) -> io::Result<()> {
    validate_stage(data, stage, record)?;
    if record.phase != "Complete" {
        return Err(failure("迁移尚未完成，未启用副本。"));
    }
    for (name, expected) in [
        ("setting.json", &record.report.imported_settings_sha256),
        (
            "translation_history.db",
            &record.report.imported_database_sha256,
        ),
    ] {
        let expected = expected
            .as_ref()
            .ok_or_else(|| failure("迁移完成记录缺少文件校验，保留副本等待恢复。"))?;
        if sha256(&stage.join(name))? != *expected {
            return Err(failure("迁移副本校验不一致，未启用副本。"));
        }
    }
    if !empty(data)? {
        return Err(failure("数据目录已有内容，不会自动合并或覆盖。"));
    }
    if data.exists() {
        fs::remove_dir(data)?;
    }
    fs::rename(stage, data)
}
fn finish(
    data: &Path,
    stage: &Path,
    mut record: Manifest,
    settings: Settings,
) -> io::Result<Outcome> {
    validate_stage(data, stage, &record)?;
    if !empty(data)? {
        return Err(failure("数据目录已有内容，不会自动合并或覆盖。"));
    }
    let backup = stage.join("migration-backup");
    if let Some(expected) = &record.report.snapshot_sha256
        && sha256(&backup.join("snapshot.db"))? != *expected
    {
        return Err(failure("保留的数据库快照校验失败，未启用副本。"));
    }
    let setting_bytes = serde_json::to_vec_pretty(settings.json()).map_err(io::Error::other)?;
    atomic_file::write(&stage.join("setting.json"), |file| {
        file.write_all(&setting_bytes)
    })?;
    let working = stage.join("translation_history.db");
    // Delete only derivative working files, after validating their immutable
    // source snapshot. Backups and original inputs are never removal targets.
    for name in [
        "translation_history.db",
        "translation_history.db-journal",
        "translation_history.db-wal",
        "translation_history.db-shm",
    ] {
        let path = stage.join(name);
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    if record.report.snapshot_sha256.is_some() {
        let snapshot = backup.join("snapshot.db");
        // These are only working files inside our validated staging directory.
        // A prior interrupted conversion's journals must not be applied to a
        // fresh snapshot. The immutable backup and originals are never removed.
        copy_new(&snapshot, &working)?;
    } else {
        let database = Connection::open(&working).map_err(db_error)?;
        database.execute_batch(SCHEMA).map_err(db_error)?;
        database.close().map_err(|(_, error)| db_error(error))?;
    }
    record.report.converted_timestamps = 0;
    record.report.assumed_local_timestamps = 0;
    record.report.ambiguous_local_timestamps = 0;
    record.report.unparseable_timestamps = 0;
    let source_key = record.report.source_key.clone();
    convert_database(
        &working,
        &source_key,
        &stage.join("timestamp-warnings.jsonl"),
        &mut record.report,
    )?;
    record.report.imported_database_sha256 = Some(sha256(&working)?);
    record.report.imported_settings_sha256 = Some(sha256(&stage.join("setting.json"))?);
    record.phase = "Complete".into();
    save_manifest(stage, &record)?;
    atomic_file::write(&stage.join(".initialized"), |file| file.write_all(b"1\n"))?;
    commit(data, stage, &record)?;
    Ok(Outcome::Imported(record.report))
}

pub fn initialize(data: &Path, source: &Path, screen: Screen) -> io::Result<Outcome> {
    if !data.is_absolute() || !source.is_absolute() {
        return Err(failure("迁移路径必须是绝对路径。"));
    }
    if initialized(data) {
        return Ok(Outcome::AlreadyInitialized);
    }
    let stage = stage_path(data)?;
    if stage.exists() {
        if let Ok(record) = manifest(&stage)
            && record.phase == "Complete"
            && empty(data)?
        {
            commit(data, &stage, &record)?;
            return Ok(Outcome::Imported(record.report));
        }
        return Ok(Outcome::NeedsRecovery {
            stage,
            message: "上次迁移未完成，保留副本仍在；请在界面选择恢复或重试。".into(),
        });
    }
    if !empty(data)? {
        return Ok(Outcome::AlreadyInitialized);
    }
    let files = source_hashes(source)?;
    if !files
        .iter()
        .any(|file| file.name == "setting.json" || file.name == "translation_history.db")
    {
        return Ok(Outcome::NoLegacyFiles);
    }
    let mut source_key = Sha256::new();
    source_key.update(source.canonicalize()?.to_string_lossy().as_bytes());
    for file in &files {
        source_key.update(file.name.as_bytes());
        source_key.update(file.sha256.as_bytes());
    }
    fs::create_dir(&stage)?;
    let backup = stage.join("migration-backup");
    fs::create_dir(&backup)?;
    let mut record = Manifest {
        schema_version: 1,
        target: data.into(),
        source: Some(source.canonicalize()?),
        phase: "Preparing".into(),
        report: Report {
            source_key: hexadecimal(source_key.finalize()),
            source_files: files.clone(),
            ..Report::default()
        },
    };
    save_manifest(&stage, &record)?;
    for file in &files {
        copy_new(&source.join(&file.name), &backup.join(&file.name))?;
        if sha256(&backup.join(&file.name))? != file.sha256 {
            return Err(failure("旧数据在备份期间发生变化，请先退出旧程序后重试。"));
        }
    }
    if source.join("translation_history.db").is_file() {
        let path = backup.join("snapshot.db");
        snapshot(&source.join("translation_history.db"), &path)?;
        record.report.snapshot_sha256 = Some(sha256(&path)?);
    }
    if source_hashes(source)? != files {
        return Err(failure("旧数据在备份期间发生变化，请先退出旧程序后重试。"));
    }
    record.phase = "SnapshotReady".into();
    save_manifest(&stage, &record)?;
    let settings = if backup.join("setting.json").exists() {
        let bytes = fs::read(backup.join("setting.json"))?;
        match std::str::from_utf8(&bytes).ok().and_then(|text|Settings::parse(text,screen).ok()){
            Some((settings,report))=>{record.report.repaired_fields=report.fields;record.report.previous_api_name=report.previous_api_name;settings},
            None=>return Ok(Outcome::NeedsRecovery{stage,message:"旧配置损坏；原文件和数据库快照已保留，未覆盖旧数据。可重新选择配置或使用默认设置。".into()}),
        }
    } else {
        record.report.used_default_settings = true;
        Settings::defaults(screen)
    };
    finish(data, &stage, record, settings)
}
/// Invoked only by the product's explicit "use defaults" recovery action.
pub fn recover_with_defaults(data: &Path, screen: Screen) -> io::Result<Outcome> {
    let stage = stage_path(data)?;
    let mut record = manifest(&stage)?;
    validate_stage(data, &stage, &record)?;
    if record.phase != "SnapshotReady" {
        return Err(failure("备份尚未完成，请先重试迁移；不会用空历史替代。"));
    }
    record.report.used_default_settings = true;
    finish(data, &stage, record, Settings::defaults(screen))
}
/// A replacement is selected in the product UI; its original bytes are retained
/// too. This action keeps the already-backed-up history rather than reimporting.
pub fn recover_with_config(data: &Path, replacement: &Path, screen: Screen) -> io::Result<Outcome> {
    let stage = stage_path(data)?;
    let mut record = manifest(&stage)?;
    validate_stage(data, &stage, &record)?;
    if record.phase != "SnapshotReady" {
        return Err(failure("备份尚未完成，请先重试迁移。"));
    }
    let bytes = fs::read(replacement)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| failure("所选配置不是有效的 UTF-8。"))?;
    let (settings, report) = Settings::parse(text, screen).map_err(io::Error::other)?;
    let retained = stage.join("migration-backup").join(format!(
        "selected-setting-{}.json",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let mut retained_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&retained)?;
    retained_file.write_all(&bytes)?;
    retained_file.flush()?;
    retained_file.sync_all()?;
    drop(retained_file);
    record.report.used_default_settings = false;
    record.report.repaired_fields = report.fields;
    record.report.previous_api_name = report.previous_api_name;
    finish(data, &stage, record, settings)
}
/// The UI explicitly states that the displayed reserved staging directory will
/// be retained in full under a new name before retrying. This also handles a
/// crash before the first manifest write. Never delete an unrecognized stage.
pub fn retry(data: &Path, source: &Path, screen: Screen) -> io::Result<Outcome> {
    if initialized(data) {
        return Ok(Outcome::AlreadyInitialized);
    }
    if !source.is_absolute()
        || !source.is_dir()
        || !source_hashes(source)?
            .iter()
            .any(|file| file.name == "setting.json" || file.name == "translation_history.db")
    {
        return Err(failure(
            "请重新选择含旧版配置或历史的文件夹；保留副本尚未移动。",
        ));
    }
    let stage = stage_path(data)?;
    validate_stage_location(data, &stage)?;
    if let Ok(record) = manifest(&stage) {
        validate_stage(data, &stage, &record)?;
    }
    let parent = stage.parent().unwrap().canonicalize()?;
    let archived = parent.join(format!(
        "migration-interrupted-{}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id()
    ));
    if archived.parent() != Some(parent.as_path()) || archived.exists() {
        return Err(failure("无法安全保留上次迁移副本。"));
    }
    fs::rename(&stage, &archived)?;
    initialize(data, source, screen)
}

/// A restart must recover the selected origin, never silently substitute the
/// executable directory. Missing first/old manifests require an explicit choice.
pub fn retry_source(data: &Path) -> io::Result<Option<PathBuf>> {
    let stage = stage_path(data)?;
    validate_stage_location(data, &stage)?;
    let Ok(record) = manifest(&stage) else {
        return Ok(None);
    };
    validate_stage(data, &stage, &record)?;
    Ok(record.source.filter(|path| path.is_absolute()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../scratch/migration-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap()
            ));
        fs::create_dir_all(&root).unwrap();
        root.canonicalize().unwrap()
    }
    fn source(root: &Path, corrupt: bool) -> (PathBuf, Connection) {
        let source = root.join("old");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("setting.json"),if corrupt{b"{\"ApiKey\":\"fixture-key\",broken".as_slice()}else{br#"{"TargetLanguage":"custom-code","Future":{"keep":true},"Configs":{"OpenAI":[{"ApiKey":"fixture-key","FutureParam":7}]}}"#.as_slice()}).unwrap();
        let connection = Connection::open(source.join("translation_history.db")).unwrap();
        connection.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;CREATE TABLE TranslationHistory(Id INTEGER PRIMARY KEY AUTOINCREMENT,Timestamp TEXT,SourceText TEXT,TranslatedText TEXT,TargetLanguage TEXT,ApiUsed TEXT);CREATE TABLE ExtraLegacy(Value TEXT);INSERT INTO ExtraLegacy VALUES('keep');").unwrap();
        for timestamp in [
            "1792989296",
            "2026-10-26T12:34:56+08:00",
            "not-a-date",
            "2026-10-26 12:34:56",
        ] {
            connection.execute("INSERT INTO TranslationHistory(Timestamp,SourceText,TranslatedText,TargetLanguage,ApiUsed) VALUES(?1,'中文 😀, source','译文','zh-CN','Google')",[timestamp]).unwrap();
        }
        (source, connection)
    }
    fn cleanup(root: &Path) {
        let expected = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../scratch/migration-tests")
            .canonicalize()
            .unwrap();
        let actual = root.canonicalize().unwrap();
        assert_eq!(actual.parent(), Some(expected.as_path()));
        fs::remove_dir_all(actual).unwrap();
    }
    #[test]
    fn wal_snapshot_keeps_all_rows_and_original_bytes_and_import_is_idempotent() {
        let root = root();
        let (source, writer) = source(&root, false);
        let before = source_hashes(&source).unwrap();
        assert!(before.iter().any(|file| file.name.ends_with("-wal")));
        let data = root.join("data");
        fs::create_dir(&data).unwrap();
        let screen = Screen::new(1920.0, 1080.0).unwrap();
        let Outcome::Imported(report) = initialize(&data, &source, screen).unwrap() else {
            panic!("migration did not complete")
        };
        assert!(before == source_hashes(&source).unwrap());
        assert_eq!(
            (
                report.original_rows,
                report.migrated_rows,
                report.converted_timestamps,
                report.assumed_local_timestamps,
                report.unparseable_timestamps
            ),
            (4, 4, 2, 1, 1)
        );
        let connection = Connection::open(data.join("translation_history.db")).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT Timestamp FROM TranslationHistory WHERE Id=2",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "1792989296"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT Timestamp FROM TranslationHistory WHERE Id=3",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "not-a-date"
        );
        assert_eq!(
            connection
                .query_row("SELECT Value FROM ExtraLegacy", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "keep"
        );
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM RustMigration", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let settings: serde_json::Value =
            serde_json::from_slice(&fs::read(data.join("setting.json")).unwrap()).unwrap();
        assert_eq!(settings["TargetLanguage"], "custom-code");
        assert_eq!(settings["Configs"]["OpenAI"][0]["ApiKey"], "fixture-key");
        assert_eq!(settings["Configs"]["OpenAI"][0]["FutureParam"], 7);
        assert!(matches!(
            initialize(&data, &source, screen).unwrap(),
            Outcome::AlreadyInitialized
        ));
        drop(connection);
        drop(writer);
        cleanup(&root);
    }
    #[test]
    fn corrupt_settings_wait_for_recovery_and_defaults_do_not_discard_history() {
        let root = root();
        let (source, writer) = source(&root, true);
        let before = source_hashes(&source).unwrap();
        let data = root.join("data");
        fs::create_dir(&data).unwrap();
        let screen = Screen::new(1920.0, 1080.0).unwrap();
        let Outcome::NeedsRecovery { stage, message } = initialize(&data, &source, screen).unwrap()
        else {
            panic!("expected recovery")
        };
        assert!(!message.contains("fixture-key"));
        assert!(!data.join("setting.json").exists());
        assert!(!data.join("translation_history.db").exists());
        assert_eq!(
            fs::read(stage.join("migration-backup/setting.json")).unwrap(),
            fs::read(source.join("setting.json")).unwrap()
        );
        assert!(before == source_hashes(&source).unwrap());
        let Outcome::Imported(report) = recover_with_defaults(&data, screen).unwrap() else {
            panic!("expected restored copy")
        };
        assert!(report.used_default_settings);
        assert_eq!(report.migrated_rows, 4);
        assert!(before == source_hashes(&source).unwrap());
        drop(writer);
        cleanup(&root);
    }
    #[test]
    fn settings_only_import_also_records_its_source_and_creates_an_empty_history() {
        let root = root();
        let source = root.join("old");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("setting.json"), b"{}").unwrap();
        let before = sha256(&source.join("setting.json")).unwrap();
        let data = root.join("data");
        let Outcome::Imported(report) =
            initialize(&data, &source, Screen::new(1920.0, 1080.0).unwrap()).unwrap()
        else {
            panic!("settings import")
        };
        assert_eq!(report.migrated_rows, 0);
        assert_eq!(sha256(&source.join("setting.json")).unwrap(), before);
        let database = Connection::open(data.join("translation_history.db")).unwrap();
        assert_eq!(
            database
                .query_row("SELECT COUNT(*) FROM RustMigration", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        drop(database);
        cleanup(&root);
    }
    #[test]
    fn selecting_a_replacement_config_retains_both_inputs_and_the_backed_up_history() {
        let root = root();
        let (source, writer) = source(&root, true);
        let before = source_hashes(&source).unwrap();
        let data = root.join("data");
        let screen = Screen::new(1920.0, 1080.0).unwrap();
        assert!(matches!(
            initialize(&data, &source, screen).unwrap(),
            Outcome::NeedsRecovery { .. }
        ));
        let selected = root.join("selected.json");
        let selected_bytes = br#"{"TargetLanguage":"ja-JP","FutureChoice":7}"#;
        fs::write(&selected, selected_bytes).unwrap();
        let selected_hash = sha256(&selected).unwrap();
        let Outcome::Imported(report) = recover_with_config(&data, &selected, screen).unwrap()
        else {
            panic!("replacement recovery")
        };
        assert!(!report.used_default_settings);
        assert_eq!(report.migrated_rows, 4);
        assert!(before == source_hashes(&source).unwrap());
        assert_eq!(sha256(&selected).unwrap(), selected_hash);
        let settings: serde_json::Value =
            serde_json::from_slice(&fs::read(data.join("setting.json")).unwrap()).unwrap();
        assert_eq!(settings["TargetLanguage"], "ja-JP");
        assert_eq!(settings["FutureChoice"], 7);
        let retained = fs::read_dir(data.join("migration-backup"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("selected-setting-")
            })
            .unwrap();
        assert_eq!(fs::read(retained).unwrap(), selected_bytes);
        drop(writer);
        cleanup(&root);
    }
    #[test]
    fn interrupted_commit_checks_current_output_before_enabling_data() {
        let root = root();
        let (source, writer) = source(&root, false);
        let data = root.join("data");
        let screen = Screen::new(1920.0, 1080.0).unwrap();
        assert!(matches!(
            initialize(&data, &source, screen).unwrap(),
            Outcome::Imported(_)
        ));
        let stage = stage_path(&data).unwrap();
        assert_eq!(data.parent(), Some(root.as_path()));
        assert_eq!(stage.parent(), Some(root.as_path()));
        fs::rename(&data, &stage).unwrap();
        fs::remove_file(stage.join("translation_history.db")).unwrap();
        assert!(initialize(&data, &source, screen).is_err());
        assert!(!data.exists());
        assert!(stage.join("migration-backup/snapshot.db").exists());
        let Outcome::Imported(report) = retry(&data, &source, screen).unwrap() else {
            panic!("explicit retry")
        };
        assert_eq!(report.migrated_rows, 4);
        assert!(fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("migration-interrupted-")
        }));
        drop(writer);
        cleanup(&root);
    }
    #[test]
    fn interruption_before_first_manifest_can_be_archived_and_retried_without_manual_cleanup() {
        let root = root();
        let (source, writer) = source(&root, false);
        let before = source_hashes(&source).unwrap();
        let data = root.join("data");
        let stage = stage_path(&data).unwrap();
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("retained-partial-file"), b"keep every byte").unwrap();
        let screen = Screen::new(1920.0, 1080.0).unwrap();
        assert!(matches!(
            initialize(&data, &source, screen).unwrap(),
            Outcome::NeedsRecovery { .. }
        ));
        let Outcome::Imported(report) = retry(&data, &source, screen).unwrap() else {
            panic!("retry without manifest")
        };
        assert_eq!(report.migrated_rows, 4);
        assert!(before == source_hashes(&source).unwrap());
        let archived = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("migration-interrupted-")
            })
            .unwrap();
        assert_eq!(
            fs::read(archived.join("retained-partial-file")).unwrap(),
            b"keep every byte"
        );
        drop(writer);
        cleanup(&root);
    }
}
