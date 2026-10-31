use std::{io, os::windows::ffi::OsStrExt, path::Path};
use windows::{
    Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    core::PCWSTR,
};

/// Commit a fully flushed temporary sibling without exposing a partial target.
pub fn replace(temporary: &Path, target: &Path) -> io::Result<()> {
    if temporary.parent() != target.parent() {
        return Err(io::Error::other("临时文件必须与目标文件在同一目录。"));
    }
    let source: Vec<_> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: both NUL-terminated paths remain live throughout this call; no
    // pointers are retained. The caller has closed/flushed the temporary file.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| io::Error::other(error.to_string()))
}
