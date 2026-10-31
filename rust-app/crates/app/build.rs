use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=resources/tray.rc");
    println!("cargo:rerun-if-changed=resources/tray.ico");
    println!("cargo:rerun-if-changed=resources/tray-enabled.ico");
    println!("cargo:rerun-if-changed=resources/tray-disabled.ico");
    println!("cargo:rerun-if-changed=resources/app.manifest");
    for path in [
        "../../../LICENSE",
        "../../../NOTICE",
        "../../../licenses/LiveCaptions-TranslatorTray-Apache-2.0.txt",
        "../../../licenses/third-party/THIRD-PARTY-NOTICES.txt",
        "../../../licenses/third-party/SKIA-NOTICES.txt",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    let resource = output.join("tray.res");
    let status = Command::new("rc.exe")
        .current_dir("resources")
        .arg("/nologo")
        .arg(format!("/fo{}", resource.display()))
        .arg("tray.rc")
        .status()
        .expect("Windows SDK rc.exe must be available in the build environment");
    assert!(status.success(), "Windows resource compiler failed");
    println!("cargo:rustc-link-arg={}", resource.display());
    // One on-disk executable contains both roles. Keep worker-only system
    // libraries out of the tray process until their first actual API call.
    println!("cargo:rustc-link-lib=delayimp");
    for library in [
        "uiautomationcore.dll",
        "dwrite.dll",
        "d2d1.dll",
        "dwmapi.dll",
        "ws2_32.dll",
        "crypt32.dll",
        "secur32.dll",
    ] {
        println!("cargo:rustc-link-arg=/DELAYLOAD:{library}");
    }
}
