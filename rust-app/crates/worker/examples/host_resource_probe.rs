//! Development-only metadata probe, excluded from normal release builds.
use std::{fs::OpenOptions, io::Write, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() == 1 && args[0] == "--child-exit" {
        return Ok(());
    }
    if args.len() == 3 && args[0] == "--self-operation" {
        let before = windows_platform::resource_probe::own_handle_count()?;
        match args[1].as_str() {
            "child" => {
                use std::os::windows::process::CommandExt;
                let status = std::process::Command::new(std::env::current_exe()?)
                    .arg("--child-exit")
                    .creation_flags(0x08000000) // CREATE_NO_WINDOW, own diagnostic only
                    .status()?;
                if !status.success() {
                    return Err("Own diagnostic child failed".into());
                }
            }
            "thread" => {
                std::thread::Builder::new()
                    .name("self-diagnostic".into())
                    .spawn(|| {})?
                    .join()
                    .map_err(|_| "Own diagnostic thread panicked")?;
            }
            _ => return Err("Unknown self operation".into()),
        }
        let after = windows_platform::resource_probe::own_handle_count()?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        let settled = windows_platform::resource_probe::own_handle_count()?;
        let report = serde_json::json!({
            "scope":"Fresh diagnostic process and its own immediate-exit child/thread only; no desktop or target inspection",
            "operation":args[1],"before":before,"after":after,"after_500ms":settled,
            "delta":i64::from(settled)-i64::from(before),
        });
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        file.write_all(serde_json::to_string_pretty(&report)?.as_bytes())?;
        file.write_all(b"\n")?;
        println!("Self-only operation diagnostic saved");
        return Ok(());
    }
    if args.len() == 3 && args[0] == "--self-rng" {
        let counts = windows_platform::resource_probe::rng_handle_counts(&args[1])?;
        let report = serde_json::json!({
            "scope":"Fresh diagnostic process only; 100 calls; no random bytes exported; no target process inspected",
            "api":args[1],"before":counts[0],"after_first":counts[1],"after_100_and_500ms":counts[2],
            "delta":i64::from(counts[2])-i64::from(counts[0]),
        });
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        file.write_all(serde_json::to_string_pretty(&report)?.as_bytes())?;
        file.write_all(b"\n")?;
        println!("Self-only RNG diagnostic saved");
        return Ok(());
    }
    if args.len() != 4 {
        return Err("Usage: host_resource_probe PID CREATION_FILETIME EXE NEW_OUTPUT_JSON; or --self-rng bcrypt|process NEW_OUTPUT_JSON".into());
    }
    let result = windows_platform::resource_probe::inspect(
        args[0].parse()?,
        args[1].parse()?,
        &PathBuf::from(&args[2]),
    )?;
    let report = serde_json::json!({
        "scope":"Identity-checked built test host metadata only; no VA clone, memory bytes, thread contexts, object names, target-resource closing or injection",
        "pid":result.pid,"creation_filetime":result.creation_filetime,
        "target_handles_before":result.handles_before,"target_handles_after":result.handles_after,
        "handle_types":result.handle_types,"handles":result.handles,
        "threads":result.threads.iter().map(|thread|serde_json::json!({"id":thread.id,"description":thread.description,
            "description_error":thread.description_error,"start_address":format!("0x{:x}",thread.start_address),"module":thread.module})).collect::<Vec<_>>(),
        "api_reference":"https://learn.microsoft.com/windows/win32/api/_proc_snap/",
    });
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[3])?;
    file.write_all(serde_json::to_string_pretty(&report)?.as_bytes())?;
    file.write_all(b"\n")?;
    println!("Metadata snapshot saved; target PID {}", result.pid);
    Ok(())
}
