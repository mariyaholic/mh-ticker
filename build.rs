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
    let windres = env::var("WINDRES").unwrap_or_else(|_| "windres".into());
    let status = Command::new(&windres)
        .current_dir(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
        .args(["--include-dir", "assets", "--input", "assets/app.rc", "--output-format=coff", "--output"])
        .arg(&out)
        .status();
    match status {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", out.display()),
        Ok(s) => println!("cargo:warning={windres} failed ({s}); building without the app icon"),
        Err(e) => println!("cargo:warning={windres} not found ({e}); building without the app icon"),
    }
}
