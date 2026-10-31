use crate::atomic_file;
use captions_core::settings::{RepairReport, Screen, Settings};
use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static BACKUP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct SettingsFile {
    path: PathBuf,
    last_saved: Option<Settings>,
}
pub struct Loaded {
    pub file: SettingsFile,
    pub settings: Settings,
    pub report: RepairReport,
    pub preserved_original: Option<PathBuf>,
}
#[derive(Debug)]
pub enum LoadError {
    Io(io::Error),
    Recovery { backup: PathBuf, message: String },
}
impl From<io::Error> for LoadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "配置读写失败：{error}"),
            Self::Recovery { backup, message } => write!(
                f,
                "{message} 原文件已保留在 {}，请在界面选择恢复方式。",
                backup.display()
            ),
        }
    }
}
impl std::error::Error for LoadError {}

fn preserve(directory: &Path, bytes: &[u8], reason: &str) -> io::Result<PathBuf> {
    let directory = directory.join("recovery");
    fs::create_dir_all(&directory)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let path = directory.join(format!(
        "setting-{reason}-{stamp}-{}-{}.json",
        std::process::id(),
        BACKUP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let result = file
        .write_all(bytes)
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = result {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(path)
}
impl SettingsFile {
    pub fn open(directory: &Path, screen: Screen) -> Result<Loaded, LoadError> {
        fs::create_dir_all(directory)?;
        let path = directory.join("setting.json");
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Loaded {
                    file: Self {
                        path,
                        last_saved: None,
                    },
                    settings: Settings::defaults(screen),
                    report: RepairReport::default(),
                    preserved_original: None,
                });
            }
            Err(error) => return Err(error.into()),
        };
        let parsed = std::str::from_utf8(&bytes)
            .map_err(|_| "配置文件不是有效的 UTF-8。".to_owned())
            .and_then(|text| Settings::parse(text, screen).map_err(|error| error.to_string()));
        let (settings, report) = match parsed {
            Ok(result) => result,
            Err(message) => {
                let backup = preserve(directory, &bytes, "corrupt")?;
                return Err(LoadError::Recovery { backup, message });
            }
        };
        let repaired = !report.fields.is_empty();
        let backup = if repaired {
            Some(preserve(directory, &bytes, "before-repair")?)
        } else {
            None
        };
        Ok(Loaded {
            file: Self {
                path,
                last_saved: (!repaired).then(|| settings.clone()),
            },
            settings,
            report,
            preserved_original: backup,
        })
    }
    pub fn save(&mut self, settings: &Settings) -> io::Result<bool> {
        if self.last_saved.as_ref() == Some(settings) {
            return Ok(false);
        }
        let mut bytes = serde_json::to_vec_pretty(settings.json()).map_err(io::Error::other)?;
        bytes.push(b'\n');
        atomic_file::write(&self.path, |file| file.write_all(&bytes))?;
        self.last_saved = Some(settings.clone());
        Ok(true)
    }
    /// A product recovery action calls this only after the user chooses defaults.
    /// `open` already preserved the corrupt bytes; it never calls this itself.
    pub fn defaults_after_recovery(directory: &Path, screen: Screen) -> (Self, Settings) {
        (
            Self {
                path: directory.join("setting.json"),
                last_saved: None,
            },
            Settings::defaults(screen),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "LiveCaptionsTray-settings-test-{}-{}",
            std::process::id(),
            BACKUP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }
    #[test]
    fn unchanged_settings_are_not_written_and_last_edit_is_atomically_saved() {
        let directory = directory();
        let screen = Screen::new(1920.0, 1080.0).unwrap();
        let mut loaded = SettingsFile::open(&directory, screen).unwrap();
        assert!(loaded.file.save(&loaded.settings).unwrap());
        assert!(!loaded.file.save(&loaded.settings).unwrap());
        let changed = loaded.settings.with_prompt("Translate to {0}").unwrap();
        assert!(loaded.file.save(&changed).unwrap());
        let reopened = SettingsFile::open(&directory, screen).unwrap();
        assert!(reopened.settings == changed);
        assert!(!reopened.report.fields.iter().any(|field| field == "Prompt"));
        fs::remove_file(directory.join("setting.json")).unwrap();
        fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn corrupt_config_remains_untouched_and_a_complete_copy_is_retained() {
        let directory = directory();
        let bytes = b"{\"ApiKey\":\"fixture-secret\",broken";
        fs::write(directory.join("setting.json"), bytes).unwrap();
        let error = SettingsFile::open(&directory, Screen::new(1920.0, 1080.0).unwrap())
            .err()
            .unwrap();
        assert!(!error.to_string().contains("fixture-secret"));
        let backup = match error {
            LoadError::Recovery { backup, .. } => backup,
            other => panic!("{other}"),
        };
        assert_eq!(fs::read(&backup).unwrap(), bytes);
        assert_eq!(fs::read(directory.join("setting.json")).unwrap(), bytes);
        fs::remove_file(backup).unwrap();
        fs::remove_dir(directory.join("recovery")).unwrap();
        fs::remove_file(directory.join("setting.json")).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
