//! Start with Windows: one value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, named
//! after the profile id (`MH_Ticker` / `MH_Ticker_Ultra`), holding the quoted path of this exe.
//! The registry is the source of truth, not the config: the menu asks `enabled` each time it
//! opens. A stored command that is not this exe (a moved or older copy) counts as off, and
//! switching on rewrites it.
//!
//! `command` is pure; the registry functions exist only on Windows.

use std::path::Path;

/// The Run value for `exe`: the path in double quotes, nothing else (`"C:\Apps\MH_Ticker.exe"`),
/// so a path with spaces launches. Backslashes are untouched; a `"` inside the path (illegal on
/// Windows) is dropped so the quoting cannot be broken.
pub fn command(exe: &Path) -> String {
    format!("\"{}\"", exe.to_string_lossy().replace('"', ""))
}

/// Do two Run commands name the same launch? Case-insensitive (Windows paths), ignoring
/// surrounding whitespace; an empty `current` matches nothing.
pub fn same_command(stored: &str, current: &str) -> bool {
    let (stored, current) = (stored.trim(), current.trim());
    !current.is_empty() && stored.eq_ignore_ascii_case(current)
}

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// This exe's Run command, or None when its path cannot be read.
#[cfg(windows)]
fn current_command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(command(&exe))
}

/// The stored command under `id`, or None when the value is absent, not a string, or unreadable.
#[cfg(windows)]
fn read(id: &str) -> Option<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::*;
    let key = wide(RUN_KEY);
    let name = wide(id);
    // SAFETY: all pointers are to live, NUL-terminated buffers; the handle is closed on every path.
    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_READ, &mut hkey) != ERROR_SUCCESS {
            return None;
        }
        let mut kind = 0u32;
        let mut buf = vec![0u16; 2048];
        let mut bytes = (buf.len() * 2) as u32;
        let rc = RegQueryValueExW(hkey, name.as_ptr(), std::ptr::null(), &mut kind, buf.as_mut_ptr() as *mut u8, &mut bytes);
        RegCloseKey(hkey);
        if rc != ERROR_SUCCESS || kind != REG_SZ {
            return None;
        }
        let n = (bytes as usize / 2).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..n]);
        Some(s.trim_end_matches('\0').to_string())
    }
}

/// Is `id` set to start this exe? Reads the registry now. False when the value is absent, is not
/// a string, or names a different command.
#[cfg(windows)]
pub fn enabled(id: &str) -> bool {
    match (read(id), current_command()) {
        (Some(stored), Some(current)) => same_command(&stored, &current),
        _ => false,
    }
}

/// Turn start-with-Windows on (write this exe's command under `id`) or off (delete the value; an
/// already-missing value is success). The error is a fixed short text, never a path.
#[cfg(windows)]
pub fn set(id: &str, on: bool) -> Result<(), &'static str> {
    set_command(id, if on { Some(current_command().ok_or("exe path")?) } else { None })
}

#[cfg(windows)]
fn set_command(id: &str, value: Option<String>) -> Result<(), &'static str> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::*;
    let key = wide(RUN_KEY);
    let name = wide(id);
    // SAFETY: as in `read`; the data pointer and byte length describe `data` exactly.
    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_SET_VALUE, &mut hkey) != ERROR_SUCCESS {
            return Err("registry");
        }
        let rc = match &value {
            Some(v) => {
                let data = wide(v);
                RegSetValueExW(hkey, name.as_ptr(), 0, REG_SZ, data.as_ptr() as *const u8, (data.len() * 2) as u32)
            }
            None => {
                let rc = RegDeleteValueW(hkey, name.as_ptr());
                if rc == ERROR_FILE_NOT_FOUND { ERROR_SUCCESS } else { rc }
            }
        };
        RegCloseKey(hkey);
        if rc == ERROR_SUCCESS { Ok(()) } else { Err("registry") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_command_quotes_the_path() {
        assert_eq!(command(Path::new(r"C:\Program Files\MH\MH_Ticker.exe")), "\"C:\\Program Files\\MH\\MH_Ticker.exe\"");
        assert_eq!(command(Path::new(r"C:\NoSpaces\MH_Ticker.exe")), r#""C:\NoSpaces\MH_Ticker.exe""#);
        // A quote is illegal in a Windows path; dropping it keeps the quoting intact.
        assert_eq!(command(Path::new(r#"C:\a"b\x.exe"#)), r#""C:\ab\x.exe""#);
    }

    #[test]
    fn autostart_same_command() {
        assert!(same_command(r#""C:\A\x.exe""#, r#""c:\a\X.EXE""#));
        assert!(same_command(r#""C:\A\x.exe" "#, r#""C:\A\x.exe""#), "trailing space on the stored value");
        assert!(!same_command(r#""C:\A\x.exe""#, r#""C:\B\x.exe""#));
        assert!(!same_command(r#""C:\A\x.exe""#, r#""C:\A\x.exe.bak""#));
        assert!(!same_command(r#""C:\A\x.exe""#, ""), "an empty current matches nothing");
        assert!(!same_command(r#""C:\A\x.exe""#, "   "));
        assert!(!same_command("", ""));
    }

    #[cfg(windows)]
    #[test]
    fn autostart_registry_round_trip_under_a_throwaway_name() {
        let id = format!("MH_Ticker_Test_{}", std::process::id());
        // Always clean up, even if an assertion fails.
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = set_command(&self.0, None);
            }
        }
        let _guard = Cleanup(id.clone());
        assert!(!enabled(&id), "a fresh name starts off");
        set(&id, true).expect("write");
        assert!(enabled(&id), "after set(on) the registry says on");
        // A different stored command counts as off.
        set_command(&id, Some(r#""C:\somewhere\else.exe""#.into())).expect("write other");
        assert!(!enabled(&id), "another exe's command counts as off");
        set(&id, true).expect("rewrite");
        assert!(enabled(&id));
        set(&id, false).expect("delete");
        assert!(!enabled(&id));
        set(&id, false).expect("deleting a missing value is fine");
    }
}
