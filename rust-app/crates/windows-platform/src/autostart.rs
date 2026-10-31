//! Per-user launch-at-login registration. No worker arguments or credentials.
use std::{io, os::windows::ffi::OsStrExt, path::Path};
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::*,
    },
    core::{PCWSTR, w},
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: PCWSTR = w!("LiveCaptionsTray");

pub struct Registration {
    key: Vec<u16>,
    command: Vec<u16>,
}

impl Registration {
    pub fn current(data_dir: &Path) -> io::Result<Self> {
        let exe = std::env::current_exe()?;
        Ok(Self {
            key: RUN_KEY.encode_utf16().chain(Some(0)).collect(),
            command: command(&exe, data_dir),
        })
    }

    pub fn enabled(&self) -> io::Result<bool> {
        let mut buffer = [0_u16; 261];
        let mut size = std::mem::size_of_val(&buffer) as u32;
        // SAFETY: terminated key/value names and writable buffer of the supplied size.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(self.key.as_ptr()),
                VALUE,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(false);
        }
        status.ok().map_err(io::Error::other)?;
        Ok(buffer[..size as usize / 2] == self.command)
    }

    pub fn set_enabled(&self, enabled: bool) -> io::Result<()> {
        if !enabled {
            // SAFETY: delete only this app's value; never remove the shared Run key.
            let status =
                unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(self.key.as_ptr()), VALUE) };
            return if matches!(status, ERROR_SUCCESS | ERROR_FILE_NOT_FOUND) {
                Ok(())
            } else {
                status.ok().map_err(io::Error::other)
            };
        }
        // Run command lines are limited to 260 characters by Windows.
        if self.command.len() > 261 {
            return Err(io::Error::other(
                "程序或数据目录路径过长，请移到较短路径后再开启自启动。",
            ));
        }
        let mut key = HKEY::default();
        // SAFETY: create/open this user's Run key; the returned handle is closed below.
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(self.key.as_ptr()),
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
            .ok()
            .map_err(io::Error::other)?;
            let result = RegSetKeyValueW(
                key,
                None,
                VALUE,
                REG_SZ.0,
                Some(self.command.as_ptr().cast()),
                (self.command.len() * 2) as u32,
            );
            let _ = RegCloseKey(key);
            result.ok().map_err(io::Error::other)
        }
    }
}

fn command(exe: &Path, data_dir: &Path) -> Vec<u16> {
    let mut result = quoted_path(exe);
    if exe.parent().map(|path| path.join("data")).as_deref() != Some(data_dir) {
        result.extend(" --data-dir ".encode_utf16());
        result.extend(quoted_path(data_dir));
    }
    result.push(0);
    result
}

fn quoted_path(path: &Path) -> Vec<u16> {
    let path: Vec<_> = path.as_os_str().encode_wide().collect();
    let mut result = vec![b'"' as u16];
    result.extend(&path);
    // Windows paths cannot contain quotes. Double trailing backslashes before
    // the closing quote so a custom data directory such as D:\ stays intact.
    result.extend(path.iter().rev().take_while(|&&c| c == b'\\' as u16));
    result.push(b'"' as u16);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_command_preserves_unicode_spaces_and_custom_data_path() {
        let exe = Path::new(r"D:\字幕 Tools\LiveCaptionsTray.exe");
        assert_eq!(
            String::from_utf16(&command(exe, Path::new(r"D:\字幕 Tools\data"))).unwrap(),
            "\"D:\\字幕 Tools\\LiveCaptionsTray.exe\"\0"
        );
        assert_eq!(
            String::from_utf16(&command(exe, Path::new(r"E:\My Data\"))).unwrap(),
            "\"D:\\字幕 Tools\\LiveCaptionsTray.exe\" --data-dir \"E:\\My Data\\\\\"\0"
        );
    }

    #[test]
    fn registration_round_trip_preserves_other_values_and_rejects_long_command() {
        // Test an isolated, non-startup key. No test registers a real login task.
        let key: Vec<_> = format!(
            r"Software\LiveCaptionsTray.Tests\Autostart-{}",
            std::process::id()
        )
        .encode_utf16()
        .chain(Some(0))
        .collect();
        let mut registration = Registration {
            key,
            command: command(
                Path::new(r"D:\字幕 Tools\LiveCaptionsTray.exe"),
                Path::new(r"D:\字幕 Tools\data"),
            ),
        };
        assert!(!registration.enabled().unwrap());
        registration.set_enabled(false).unwrap();
        registration.set_enabled(true).unwrap();
        assert!(registration.enabled().unwrap());
        // A second value represents another application's registration.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(registration.key.as_ptr()),
                w!("Unrelated"),
                REG_SZ.0,
                Some(registration.command.as_ptr().cast()),
                (registration.command.len() * 2) as u32,
            )
            .ok()
            .unwrap();
        }
        registration.set_enabled(false).unwrap();
        assert!(!registration.enabled().unwrap());
        unsafe {
            let mut size = 0;
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(registration.key.as_ptr()),
                w!("Unrelated"),
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut size),
            )
            .ok()
            .unwrap();
            assert!(size > 0);
        }
        registration.command = vec![b'x' as u16; 262];
        assert!(registration.set_enabled(true).is_err());
        assert!(!registration.enabled().unwrap());
        unsafe {
            RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(registration.key.as_ptr()))
                .ok()
                .unwrap();
        }
    }
}
