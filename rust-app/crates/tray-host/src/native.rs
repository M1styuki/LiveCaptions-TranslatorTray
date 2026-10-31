use std::{
    ffi::OsStr,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{FromRawHandle, OwnedHandle, RawHandle},
    },
};
use windows::Win32::Foundation::HANDLE;

pub fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain(Some(0)).collect()
}

pub fn os_error(error: windows::core::Error) -> io::Error {
    io::Error::other(error.to_string())
}

/// Transfers ownership of a newly created kernel handle to RAII.
pub unsafe fn own(handle: HANDLE) -> OwnedHandle {
    // SAFETY: caller guarantees a valid, uniquely owned CloseHandle-compatible handle.
    unsafe { OwnedHandle::from_raw_handle(handle.0 as RawHandle) }
}

pub fn handle(value: &impl std::os::windows::io::AsRawHandle) -> HANDLE {
    HANDLE(value.as_raw_handle())
}
