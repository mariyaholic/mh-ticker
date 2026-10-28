//! Embeds `assets/icon.ico` as the exe's icon (what Explorer, shortcuts and the taskbar show).
//!
//! windres (MinGW, which the windows-gnu toolchain ships with) compiles `assets/app.rc` to a COFF
//! object that is linked into the binary. Without windres the build still succeeds, just with the
//! generic exe icon, and says so. `WINDRES` overrides the tool name.
use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-env-changed=WINDRES");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("app_res.o");
    let override_tool = env::var("WINDRES").ok();
    let host = env::var("HOST").unwrap_or_default();
    let target = env::var("TARGET").unwrap_or_default();
    let prefix = if target.starts_with("x86_64-") {
        Some("x86_64")
    } else if target.starts_with("i686-") {
        Some("i686")
    } else if target.starts_with("aarch64-") {
        Some("aarch64")
    } else {
        None
    };
    let mut tools = Vec::new();
    if let Some(tool) = override_tool {
        tools.push(tool);
    } else {
        if !host.contains("windows") {
            if let Some(arch) = prefix {
                tools.push(format!("{arch}-w64-mingw32-windres"));
            }
        }
        tools.push("windres".into());
    }
    for windres in tools {
        let status = Command::new(&windres)
            .current_dir(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
            .args([
                "--include-dir",
                "assets",
                "--input",
                "assets/app.rc",
                "--output-format=coff",
                "--output",
            ])
            .arg(&out)
            .status();
        match status {
            Ok(s) if s.success() => {
                println!("cargo:rustc-link-arg-bins={}", out.display());
                return;
            }
            Ok(s) => {
                println!("cargo:warning={windres} failed ({s}); building without the app icon")
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                println!("cargo:warning={windres} failed ({e}); building without the app icon")
            }
        }
    }
    println!("cargo:warning=windres unavailable or failed; building without the app icon");
}
