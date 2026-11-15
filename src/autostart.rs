//! Start with Windows: one value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, named
//! after the profile id (`MH_Ticker` / `MH_Ticker_Ultra`), holding the quoted path of this exe.
//! The registry is the source of truth, not the config: the menu asks `enabled` each time it
//! opens. A stored command that is not this exe (a moved or older copy) counts as off, and
//! switching on rewrites it. Task Manager and Windows Settings keep a second fact under
//! `Explorer\StartupApproved\Run`; an entry turned off there counts as off here too, and switching
//! on removes that approval entry.
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
    current_command().is_some_and(|current| enabled_in(&Registry, id, &current))
}

/// Turn start-with-Windows on (write this exe's command under `id`, and drop a disabled approval) or
/// off (delete the value; an already-missing value is success). The error is a fixed short text, never
/// a path.
#[cfg(windows)]
pub fn set(id: &str, on: bool) -> Result<(), &'static str> {
    let current = if on { current_command().ok_or("exe path")? } else { String::new() };
    set_in(&mut Registry, id, on, &current)
}

#[cfg(windows)]
const APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// The real registry: the Run value and the StartupApproved entry that Task Manager and Windows
/// Settings write.
#[cfg(windows)]
struct Registry;

#[cfg(windows)]
impl Store for Registry {
    fn run(&self, id: &str) -> Option<String> {
        read(id)
    }

    fn set_run(&mut self, id: &str, value: Option<String>) -> Result<(), &'static str> {
        set_command(id, value)
    }

    fn approval(&self, id: &str) -> Option<Vec<u8>> {
        use windows_sys::Win32::Foundation::ERROR_SUCCESS;
        use windows_sys::Win32::System::Registry::*;
        let key = wide(APPROVED_KEY);
        let name = wide(id);
        // SAFETY: as in `read`.
        unsafe {
            let mut hkey: HKEY = std::ptr::null_mut();
            if RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_READ, &mut hkey) != ERROR_SUCCESS {
                return None;
            }
            let mut kind = 0u32;
            let mut buf = [0u8; 64];
            let mut bytes = buf.len() as u32;
            let rc = RegQueryValueExW(hkey, name.as_ptr(), std::ptr::null(), &mut kind, buf.as_mut_ptr(), &mut bytes);
            RegCloseKey(hkey);
            if rc != ERROR_SUCCESS || kind != REG_BINARY {
                return None;
            }
            Some(buf[..(bytes as usize).min(buf.len())].to_vec())
        }
    }

    fn clear_approval(&mut self, id: &str) -> Result<(), &'static str> {
        use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
        use windows_sys::Win32::System::Registry::*;
        let key = wide(APPROVED_KEY);
        let name = wide(id);
        // SAFETY: as in `read`.
        unsafe {
            let mut hkey: HKEY = std::ptr::null_mut();
            let opened = RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_SET_VALUE, &mut hkey);
            if opened == ERROR_FILE_NOT_FOUND || opened == ERROR_PATH_NOT_FOUND {
                return Ok(());
            }
            if opened != ERROR_SUCCESS {
                return Err("registry");
            }
            let rc = RegDeleteValueW(hkey, name.as_ptr());
            RegCloseKey(hkey);
            if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND { Ok(()) } else { Err("registry") }
        }
    }
}

/// Where the two autostart facts live: the Run value, and the approval entry Windows keeps for it.
/// The registry is one store; the tests use another.
pub trait Store {
    /// The Run value under `id`.
    fn run(&self, id: &str) -> Option<String>;
    /// Write (`Some`) or delete (`None`) the Run value under `id`.
    fn set_run(&mut self, id: &str, value: Option<String>) -> Result<(), &'static str>;
    /// The StartupApproved bytes under `id`, when there are any.
    fn approval(&self, id: &str) -> Option<Vec<u8>>;
    /// Delete the StartupApproved entry under `id`; a missing one is success.
    fn clear_approval(&mut self, id: &str) -> Result<(), &'static str>;
}

/// Does the StartupApproved entry let the Run value start? Windows writes a 12 byte value whose first
/// byte is 2 or 6 for an enabled entry and 3 or 7 for one turned off in Task Manager or Settings. No
/// entry, or one that says nothing known, leaves the Run value in force.
pub fn approval_allows(value: Option<&[u8]>) -> bool {
    !matches!(value.and_then(|v| v.first()), Some(3 | 7))
}

/// `enabled` over any store: the Run value names `current` and Windows has not turned the entry off.
pub fn enabled_in(store: &impl Store, id: &str, current: &str) -> bool {
    store.run(id).is_some_and(|stored| same_command(&stored, current)) && approval_allows(store.approval(id).as_deref())
}

/// `set` over any store. Switching on writes `current` and drops an approval that turned the entry
/// off, so the next sign-in starts the app; switching off deletes the Run value and leaves the
/// approval as the user set it.
pub fn set_in(store: &mut impl Store, id: &str, on: bool, current: &str) -> Result<(), &'static str> {
    if on {
        store.set_run(id, Some(current.to_string()))?;
        if !approval_allows(store.approval(id).as_deref()) {
            store.clear_approval(id)?;
        }
        Ok(())
    } else {
        store.set_run(id, None)
    }
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
        // The Run key is not there on a profile nothing has ever registered in (a fresh account, a
        // CI runner): switching on creates it, and switching off has nothing to delete.
        let opened = match &value {
            Some(_) => RegCreateKeyExW(
                HKEY_CURRENT_USER, key.as_ptr(), 0, std::ptr::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE,
                std::ptr::null(), &mut hkey, std::ptr::null_mut(),
            ),
            None => RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_SET_VALUE, &mut hkey),
        };
        if opened == ERROR_FILE_NOT_FOUND && value.is_none() {
            return Ok(());
        }
        if opened != ERROR_SUCCESS {
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

    /// The Run value in the real registry under a throwaway name; the StartupApproved key is never
    /// read or written, so the approval always reads as absent.
    #[cfg(windows)]
    struct RunOnly;

    #[cfg(windows)]
    impl Store for RunOnly {
        fn run(&self, id: &str) -> Option<String> {
            read(id)
        }
        fn set_run(&mut self, id: &str, value: Option<String>) -> Result<(), &'static str> {
            set_command(id, value)
        }
        fn approval(&self, _: &str) -> Option<Vec<u8>> {
            None
        }
        fn clear_approval(&mut self, _: &str) -> Result<(), &'static str> {
            Ok(())
        }
    }

    #[cfg(windows)]
    #[test]
    fn autostart_registry_round_trip_under_a_throwaway_name() {
        let id = format!("MH_Ticker_Test_{}", std::process::id());
        let me = current_command().expect("exe path");
        // Always clean up, even if an assertion fails.
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = set_command(&self.0, None);
            }
        }
        let _guard = Cleanup(id.clone());
        assert!(!enabled_in(&RunOnly, &id, &me), "a fresh name starts off");
        set_in(&mut RunOnly, &id, true, &me).expect("write");
        assert!(enabled_in(&RunOnly, &id, &me), "after set(on) the registry says on");
        // A different stored command counts as off.
        set_command(&id, Some(r#""C:\somewhere\else.exe""#.into())).expect("write other");
        assert!(!enabled_in(&RunOnly, &id, &me), "another exe's command counts as off");
        set_in(&mut RunOnly, &id, true, &me).expect("rewrite");
        assert!(enabled_in(&RunOnly, &id, &me));
        set_in(&mut RunOnly, &id, false, &me).expect("delete");
        assert!(!enabled_in(&RunOnly, &id, &me));
        set_in(&mut RunOnly, &id, false, &me).expect("deleting a missing value is fine");
    }

    /// A store held in memory: the Run values and the approval bytes.
    #[derive(Default)]
    struct Memory {
        run: Option<String>,
        approval: Option<Vec<u8>>,
    }

    impl Store for Memory {
        fn run(&self, _: &str) -> Option<String> {
            self.run.clone()
        }
        fn set_run(&mut self, _: &str, value: Option<String>) -> Result<(), &'static str> {
            self.run = value;
            Ok(())
        }
        fn approval(&self, _: &str) -> Option<Vec<u8>> {
            self.approval.clone()
        }
        fn clear_approval(&mut self, _: &str) -> Result<(), &'static str> {
            self.approval = None;
            Ok(())
        }
    }

    const ME: &str = r#""C:\A\MH_Ticker.exe""#;

    fn bytes(first: u8) -> Vec<u8> {
        let mut v = vec![0u8; 12];
        v[0] = first;
        v
    }

    #[test]
    fn startup_approved_bytes_say_whether_windows_lets_the_entry_start() {
        assert!(approval_allows(None), "no entry: the Run value decides");
        assert!(approval_allows(Some(&bytes(2))));
        assert!(approval_allows(Some(&bytes(6))));
        assert!(!approval_allows(Some(&bytes(3))), "turned off in Task Manager");
        assert!(!approval_allows(Some(&bytes(7))), "turned off in Settings");
        assert!(approval_allows(Some(&[])), "an empty value says nothing");
    }

    #[test]
    fn an_entry_turned_off_in_windows_reads_off_and_switching_on_clears_the_approval() {
        let mut m = Memory { run: Some(ME.into()), approval: Some(bytes(2)) };
        assert!(enabled_in(&m, "MH_Ticker", ME));
        m.approval = Some(bytes(3));
        assert!(!enabled_in(&m, "MH_Ticker", ME), "the Run value is there but Windows will not start it");
        set_in(&mut m, "MH_Ticker", true, ME).unwrap();
        assert_eq!(m.run.as_deref(), Some(ME));
        assert!(enabled_in(&m, "MH_Ticker", ME), "the toggle now matches what the next sign-in does");
        assert_eq!(m.approval, None, "the disabled approval is gone");
    }

    #[test]
    fn switching_on_leaves_an_enabled_approval_and_switching_off_leaves_the_approval() {
        let mut m = Memory { run: None, approval: Some(bytes(6)) };
        set_in(&mut m, "MH_Ticker", true, ME).unwrap();
        assert_eq!(m.approval, Some(bytes(6)));
        set_in(&mut m, "MH_Ticker", false, ME).unwrap();
        assert_eq!(m.run, None);
        assert_eq!(m.approval, Some(bytes(6)));
        assert!(!enabled_in(&m, "MH_Ticker", ME));
    }
}
