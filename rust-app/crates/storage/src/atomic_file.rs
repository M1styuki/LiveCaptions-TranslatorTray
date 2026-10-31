use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static COUNTER: AtomicU64 = AtomicU64::new(0);
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn write(target: &Path, writer: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("文件缺少父目录。"))?;
    let file_name = target
        .file_name()
        .ok_or_else(|| io::Error::other("文件名为空。"))?;
    let mut temporary_name = std::ffi::OsString::from(".");
    temporary_name.push(file_name);
    temporary_name.push(format!(
        ".{}-{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let temporary_path = parent.join(temporary_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)?;
    // The removal guard owns only a file we successfully created, never a
    // pre-existing collision left by another process or an earlier run.
    let temporary = Temporary(temporary_path);
    let written = writer(&mut file)
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_all());
    drop(file);
    written?;
    windows_platform::files::replace(&temporary.0, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_export_keeps_existing_target_and_removes_the_partial_file() {
        let directory = std::env::temp_dir().join(format!(
            "LiveCaptionsTray-atomic-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let target = directory.join("history.csv");
        fs::write(&target, b"original").unwrap();
        let result = write(&target, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("fixture failure"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"original");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        write(&target, |file| file.write_all(b"complete")).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"complete");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_file(target).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
