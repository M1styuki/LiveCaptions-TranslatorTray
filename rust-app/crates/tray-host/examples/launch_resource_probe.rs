//! Development-only fresh-process diagnostic using the actual host modules.
#![allow(dead_code)]
#[path = "../src/native.rs"]
mod native;
#[path = "../src/pipe.rs"]
mod pipe;
#[path = "../src/security.rs"]
mod security;
#[path = "../src/tray.rs"]
mod tray;
#[path = "../src/worker.rs"]
mod worker;

use std::{fs::OpenOptions, io::Write};
use windows::Win32::System::{
    JobObjects::{
        CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    },
    Threading::{GetCurrentProcess, GetProcessHandleCount},
};

fn count() -> Result<u32, Box<dyn std::error::Error>> {
    let mut value = 0;
    // SAFETY: querying only the current diagnostic process.
    unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut value) }?;
    Ok(value)
}
fn security_stages() -> Result<(), Box<dyn std::error::Error>> {
    use windows::{
        Win32::{
            Foundation::{HANDLE, HLOCAL, LocalFree},
            Security::{
                Authorization::{
                    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                    SDDL_REVISION_1,
                },
                GetTokenInformation, PSECURITY_DESCRIPTOR, TOKEN_GROUPS, TOKEN_QUERY,
                TokenLogonSid,
            },
            System::Threading::OpenProcessToken,
        },
        core::{PCWSTR, PWSTR},
    };
    // Same call sequence as PipeSecurity::current_logon, reporting only counts.
    // No SID/object name or security descriptor is exported.
    unsafe {
        let mut raw = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw)?;
        let token = native::own(raw);
        println!("token-open={}", count()?);
        let mut size = 0;
        let _ = GetTokenInformation(native::handle(&token), TokenLogonSid, None, 0, &mut size);
        println!("token-size={}", count()?);
        let mut buffer = vec![0_usize; (size as usize).div_ceil(size_of::<usize>())];
        GetTokenInformation(
            native::handle(&token),
            TokenLogonSid,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )?;
        println!("token-filled={}", count()?);
        let groups = &*buffer.as_ptr().cast::<TOKEN_GROUPS>();
        if groups.GroupCount != 1 {
            return Err("Unexpected logon SID count".into());
        }
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(groups.Groups[0].Sid, &mut text)?;
        println!("sid-to-string={}", count()?);
        let sid_string = text.to_string();
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        let sddl = native::wide(format!("D:P(A;;GA;;;{})", sid_string?));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
        println!("sddl-to-descriptor={}", count()?);
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        drop(token);
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("Usage: launch_resource_probe security|pipe|job NEW_OUTPUT_CSV".into());
    }
    let before = count()?;
    match args[0].as_str() {
        "security" => drop(security::PipeSecurity::current_logon()?),
        "security-stages" => security_stages()?,
        "pipe" => {
            let name = format!(r"\\.\pipe\LCT-Self-Diagnostic-{}", std::process::id());
            drop(pipe::Pipe::create(&name)?);
        }
        "job" => {
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
            // SAFETY: create/query/close only a new empty self-owned job.
            let job = unsafe { native::own(CreateJobObjectW(None, None)?) };
            unsafe {
                SetInformationJobObject(
                    native::handle(&job),
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const _,
                    size_of_val(&limits) as u32,
                )?;
            }
            drop(job);
        }
        _ => return Err("Unknown self diagnostic operation".into()),
    }
    let after = count()?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    let settled = count()?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?;
    writeln!(file, "operation,before,after,after_500ms,delta")?;
    writeln!(
        file,
        "{},{before},{after},{settled},{}",
        args[0],
        i64::from(settled) - i64::from(before)
    )?;
    println!("{}: {before} -> {after} -> {settled}", args[0]);
    Ok(())
}
#[path = "../src/menu.rs"]
mod menu;
