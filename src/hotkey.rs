//! The global hotkey that opens and closes the panel. Pure and portable: `std` only, so it compiles
//! and tests on any host. The Win32 side (`RegisterHotKey`, `WM_HOTKEY`) lives in `ui.rs`; the
//! numbers it needs are defined here as plain constants.
//!
//! Grammar: zero or more modifiers `Ctrl`, `Alt`, `Shift`, `Win`, then exactly one key, joined by
//! `+`: `A`-`Z`, `0`-`9`, `F1`-`F24`, `Space`, `Home`, `End`, `Insert`, `Delete`, `PageUp`,
//! `PageDown`. Case-insensitive; spaces around parts are ignored. At least one of Ctrl, Alt or Win
//! is required (Shift alone, or no modifier, would steal ordinary typing).

/// `RegisterHotKey` modifier bits (`MOD_*`).
pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;
/// Holding the keys down does not repeat `WM_HOTKEY`.
pub const MOD_NOREPEAT: u32 = 0x4000;
/// The window message `RegisterHotKey` posts.
pub const WM_HOTKEY: u32 = 0x0312;

/// What the Hotkey submenu offers besides Off, in menu order. Each parses.
pub const PRESETS: [&str; 4] = ["Ctrl+Alt+M", "Ctrl+Alt+T", "Ctrl+Shift+F12", "Ctrl+Alt+Space"];

/// A parsed hotkey: `mods` is an OR of `MOD_ALT | MOD_CONTROL | MOD_SHIFT | MOD_WIN` (never
/// `MOD_NOREPEAT`; the caller adds that), `vk` a Windows virtual-key code (`A` = 0x41, `0` = 0x30,
/// `F1` = 0x70, Space 0x20, PageUp 0x21, PageDown 0x22, End 0x23, Home 0x24, Insert 0x2D,
/// Delete 0x2E).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combo {
    pub mods: u32,
    pub vk: u32,
}

/// Parse `text` per the grammar above. `None` for: an empty string, an unknown key or modifier,
/// no key, two keys, a repeated modifier, `F0` / `F25`, or none of Ctrl/Alt/Win present.
pub fn parse(text: &str) -> Option<Combo> {
    let mut mods = 0;
    let mut vk = None;
    for part in text.split('+') {
        // An empty part ("", "Ctrl++", "+M") is refused, and so is anything after the key.
        let part = part.trim();
        if part.is_empty() || vk.is_some() {
            return None;
        }
        let bit = match part.to_ascii_uppercase().as_str() {
            "CTRL" => MOD_CONTROL,
            "ALT" => MOD_ALT,
            "SHIFT" => MOD_SHIFT,
            "WIN" => MOD_WIN,
            _ => 0,
        };
        if bit != 0 {
            if mods & bit != 0 {
                return None;
            }
            mods |= bit;
        } else {
            vk = Some(key_code(part)?);
        }
    }
    if mods & (MOD_CONTROL | MOD_ALT | MOD_WIN) == 0 {
        return None;
    }
    Some(Combo { mods, vk: vk? })
}

/// The virtual-key code of one key name from the grammar, case-insensitive. `None` for anything
/// else, including `F0`, `F25`, `F01` (leading zeros) and `Esc`.
fn key_code(name: &str) -> Option<u32> {
    let upper = name.to_ascii_uppercase();
    // Single letters and digits: their ASCII codes are the virtual-key codes.
    if let [c] = upper.as_bytes() {
        return match c {
            b'A'..=b'Z' | b'0'..=b'9' => Some(u32::from(*c)),
            _ => None,
        };
    }
    if let Some(num) = upper.strip_prefix('F') {
        if !num.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let n: u32 = num.parse().ok()?;
        if n.to_string() != num || !(1..=24).contains(&n) {
            return None;
        }
        return Some(0x70 + n - 1);
    }
    match upper.as_str() {
        "SPACE" => Some(0x20),
        "PAGEUP" => Some(0x21),
        "PAGEDOWN" => Some(0x22),
        "END" => Some(0x23),
        "HOME" => Some(0x24),
        "INSERT" => Some(0x2D),
        "DELETE" => Some(0x2E),
        _ => None,
    }
}

/// The canonical spelling: modifiers in the order Ctrl, Alt, Shift, Win, then the key (`A`-`Z`
/// upper case, `F12`, `PageUp`, ...), joined by `+`. `label(&parse(s)?)` is canonical for any
/// accepted `s`, and `parse(&label(c)) == Some(c)`. A `vk` outside the grammar prints as `0x..`.
pub fn label(c: &Combo) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (bit, name) in [(MOD_CONTROL, "Ctrl"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift"), (MOD_WIN, "Win")] {
        if c.mods & bit != 0 {
            parts.push(name.into());
        }
    }
    parts.push(key_name(c.vk));
    parts.join("+")
}

/// The grammar's spelling of a virtual-key code, or `0x..` when it is not one of the grammar's keys.
fn key_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x20 => "Space".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        _ => format!("{vk:#x}"),
    }
}

/// The menu choice a configured hotkey string selects: `Some(i)` when `parse(cfg)` equals the
/// parse of `PRESETS[i]`, else `None` (shown as an extra "configured" entry).
pub fn preset_index(cfg: &str) -> Option<usize> {
    let c = parse(cfg)?;
    PRESETS.iter().position(|p| parse(p) == Some(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_parse_and_label_round_trip() {
        for preset in PRESETS {
            let c = parse(preset).unwrap_or_else(|| panic!("{preset} parses"));
            assert_eq!(label(&parse(preset).unwrap()), preset, "presets are stored canonically");
            assert_eq!(parse(&label(&c)), Some(c));
        }
        let keys: Vec<u32> = (0x41..=0x5A)
            .chain(0x30..=0x39)
            .chain(0x70..=0x87)
            .chain([0x20, 0x21, 0x22, 0x23, 0x24, 0x2D, 0x2E])
            .collect();
        for vk in keys {
            for mods in [MOD_CONTROL, MOD_ALT | MOD_CONTROL, MOD_WIN | MOD_SHIFT | MOD_ALT | MOD_CONTROL] {
                let c = Combo { mods, vk };
                let text = label(&c);
                assert_eq!(parse(&text), Some(c), "{text} did not parse back");
            }
        }
        assert_eq!(label(&Combo { mods: MOD_CONTROL, vk: 0x87 }), "Ctrl+F24");
        assert_eq!(label(&Combo { mods: MOD_CONTROL, vk: 0x22 }), "Ctrl+PageDown");
        // A key outside the grammar prints as hex, and is not a parseable hotkey.
        assert_eq!(label(&Combo { mods: MOD_CONTROL, vk: 0x1B }), "Ctrl+0x1b");
        assert_eq!(parse("Ctrl+0x1b"), None);
    }

    #[test]
    fn hotkey_is_case_and_space_insensitive() {
        let want = parse("Ctrl+Alt+M");
        assert!(want.is_some());
        assert_eq!(parse(" ctrl + ALT + m "), want);
        assert_eq!(parse("cTrL+aLt+M"), want);
        assert_eq!(parse("  Ctrl  +Alt+  m"), want);
        assert_eq!(label(&parse(" ctrl + ALT + m ").unwrap()), "Ctrl+Alt+M");
        assert_eq!(label(&parse("win+shift+pagedown").unwrap()), "Shift+Win+PageDown");
        assert_eq!(parse("ctrl+f12"), parse("Ctrl+F12"));
        assert_eq!(parse("CTRL+SPACE"), parse("Ctrl+Space"));
    }

    #[test]
    fn hotkey_refuses_unknown_keys_and_missing_modifiers() {
        for bad in [
            "", "   ", "M", "Shift+M", "Shift+Ctrl", "Ctrl+Alt", "Ctrl", "Ctrl+Q+W", "Ctrl+Ctrl+M", "Alt+Alt+M",
            "Ctrl+M+Alt", "Ctrl+F0", "Ctrl+F25", "Ctrl+F01", "Ctrl+F+1", "Ctrl+F 1", "Ctrl+Esc", "Hyper+M",
            "Control+M", "Ctrl++", "+M", "Ctrl+M+", "Ctrl+MM", "Ctrl+1+2", "Ctrl+Alt+Space+M", "Ctrl+-", "Ctrl+\u{e9}",
        ] {
            assert_eq!(parse(bad), None, "{bad:?} should be refused");
        }
        // The edges of the grammar are accepted.
        assert!(parse("Ctrl+F24").is_some());
        assert!(parse("Ctrl+F1").is_some());
        assert!(parse("Win+0").is_some());
        assert!(parse("Shift+Win+M").is_some(), "Win alone satisfies the modifier rule");
    }

    #[test]
    fn hotkey_constants_match_win32() {
        assert_eq!(parse("Ctrl+Alt+M"), Some(Combo { mods: MOD_CONTROL | MOD_ALT, vk: 0x4D }));
        assert_eq!(parse("Ctrl+Shift+F12"), Some(Combo { mods: MOD_CONTROL | MOD_SHIFT, vk: 0x7B }));
        assert_eq!(parse("Ctrl+Alt+Space"), Some(Combo { mods: MOD_CONTROL | MOD_ALT, vk: 0x20 }));
        assert_eq!((MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, MOD_NOREPEAT), (1, 2, 4, 8, 0x4000));
        assert_eq!(WM_HOTKEY, 0x0312);
    }

    #[test]
    fn preset_index_finds_presets_only() {
        assert_eq!(preset_index("ctrl+alt+t"), Some(1));
        assert_eq!(preset_index("Alt+Ctrl+M"), Some(0), "same combo, other order");
        assert_eq!(preset_index("alt+ctrl+space"), Some(3));
        assert_eq!(preset_index("Ctrl+Alt+Q"), None);
        assert_eq!(preset_index("garbage"), None);
        assert_eq!(preset_index(""), None);
        for (i, p) in PRESETS.iter().enumerate() {
            assert_eq!(preset_index(p), Some(i));
        }
    }
}
