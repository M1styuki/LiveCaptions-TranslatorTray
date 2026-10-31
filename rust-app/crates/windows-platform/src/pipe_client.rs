use std::{
    fs::OpenOptions,
    io,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
};
use windows::{
    Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::FILE_FLAG_OVERLAPPED,
        System::Pipes::{GetNamedPipeServerProcessId, WaitNamedPipeW},
    },
    core::PCWSTR,
};

pub fn connect(name: &str, expected_host: u32) -> io::Result<crate::pipe_stream::PipeStream> {
    let suffix = name
        .strip_prefix(r"\\.\pipe\LiveCaptionsTray-")
        .filter(|suffix| suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid host pipe name"))?;
    let _ = suffix;
    let wide: Vec<u16> = std::ffi::OsStr::new(name)
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: UTF-16 name remains live for the bounded startup-only wait. No poll
    // or Win32 objects survive into the ordinary disabled-engine idle path.
    if !unsafe { WaitNamedPipeW(PCWSTR(wide.as_ptr()), 10_000) }.as_bool() {
        return Err(io::Error::last_os_error());
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(FILE_FLAG_OVERLAPPED.0)
        .open(name)?;
    let mut actual_host = 0;
    // SAFETY: File owns a valid connected pipe handle and output is a live u32.
    unsafe { GetNamedPipeServerProcessId(HANDLE(file.as_raw_handle()), &mut actual_host) }
        .map_err(|error| io::Error::other(error.to_string()))?;
    if actual_host != expected_host {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Unexpected host process",
        ));
    }
    // SAFETY: this file was opened above exclusively for overlapped pipe I/O.
    unsafe { crate::pipe_stream::PipeStream::new(file) }
}
