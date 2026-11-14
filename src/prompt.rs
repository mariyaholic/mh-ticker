//! A modal one-line prompt: a label, an edit box, OK and Cancel.
//!
//! Built from an in-memory dialog template (`DialogBoxIndirectParamW`), so there is no resource file
//! and nothing new to link. Enter accepts (OK is the default button), Esc and the close box cancel,
//! and the text is selected when the prompt opens, so typing replaces it.
//!
//! [`ask`] runs a modal loop: it must never be called from inside a `with_ui` closure (the loop
//! dispatches paint and feed messages, which call `with_ui`, and a nested call silently does nothing).
//! [`template`] is pure: it only builds the bytes, which is what the tests check.

use std::ptr::null;
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DialogBoxIndirectParamW, EndDialog, GetDlgItem, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, PostMessageW,
    SendMessageW, SetDlgItemTextW, SetForegroundWindow, SetWindowLongPtrW, DLGTEMPLATE, IDCANCEL, IDOK, WM_COMMAND,
    WM_INITDIALOG, WS_BORDER, WS_CAPTION, WS_CHILD, WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

/// The label's control id.
pub(crate) const ID_LABEL: u16 = 100;
/// The edit box's control id.
pub(crate) const ID_EDIT: u16 = 101;

// Control and dialog styles, defined here because the `windows-sys` features this crate enables do
// not export all of them (and the ones that are exported are `i32`, the window styles `u32`).
const DS_SETFONT: u32 = 0x0040;
const DS_MODALFRAME: u32 = 0x0080;
const DS_CENTER: u32 = 0x0800;
/// A `&` in the label (`S&P`) is drawn as itself, not as a mnemonic underline.
const SS_NOPREFIX: u32 = 0x0080;
const ES_AUTOHSCROLL: u32 = 0x0080;
const BS_DEFPUSHBUTTON: u32 = 0x0001;
const EM_SETSEL: u32 = 0x00B1;
const EM_LIMITTEXT: u32 = 0x00C5;
/// `DWLP_USER`: after the message result and the dialog procedure, one pointer each.
const DWLP_USER: i32 = 2 * std::mem::size_of::<isize>() as i32;
/// Predefined window-class atoms for a dialog item.
const ATOM_BUTTON: u16 = 0x0080;
const ATOM_EDIT: u16 = 0x0081;
const ATOM_STATIC: u16 = 0x0082;

/// The face and size the prompt asks for (`DS_SETFONT`). A machine without it gets the system's
/// substitute, which only moves pixels: the layout is in dialog units.
const FACE: &str = "Segoe UI";
const POINT_SIZE: u16 = 9;

/// A built template: the bytes `DialogBoxIndirectParamW` reads, and where each item starts.
pub(crate) struct Template {
    pub bytes: Vec<u8>,
    /// Byte offset of each `DLGITEMTEMPLATE` from the start of `bytes`, in item order (label, edit,
    /// OK, Cancel). Each is a multiple of 4, as Windows requires. Read by the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub items: Vec<usize>,
}

/// Build the prompt's `DLGTEMPLATE` with `DS_SETFONT` (`face` at `pt` points): a 220 x 66
/// dialog-unit frame titled `title`, then four items in this order, each DWORD aligned:
/// a static `label` (`ID_LABEL`, up to two lines), an empty edit box (`ID_EDIT`), a default push
/// button "OK" (`IDOK`) and a push button "Cancel" (`IDCANCEL`). Strings are UTF-16 and
/// NUL-terminated; a NUL inside one ends it there. The bytes start at offset 0, so the caller must
/// place them at a 4-byte-aligned address (see [`ask`]).
pub(crate) fn template(title: &str, label: &str, face: &str, pt: u16) -> Template {
    let mut b: Vec<u8> = Vec::with_capacity(256);
    let style = WS_POPUP | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME | DS_SETFONT | DS_CENTER;
    push_u32(&mut b, style);
    push_u32(&mut b, 0); // extended style
    push_u16(&mut b, 4); // item count
    for v in [0i16, 0, 220, 66] {
        push_u16(&mut b, v as u16); // x, y, cx, cy
    }
    push_u16(&mut b, 0); // no menu
    push_u16(&mut b, 0); // the predefined dialog class
    push_wsz(&mut b, title);
    push_u16(&mut b, pt);
    push_wsz(&mut b, face);
    let child = WS_CHILD | WS_VISIBLE;
    let items: [(u32, [i16; 4], u16, u16, &str); 4] = [
        (child | SS_NOPREFIX, [7, 6, 206, 18], ID_LABEL, ATOM_STATIC, label),
        (child | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL, [7, 26, 206, 14], ID_EDIT, ATOM_EDIT, ""),
        (child | WS_TABSTOP | BS_DEFPUSHBUTTON, [109, 46, 50, 14], IDOK as u16, ATOM_BUTTON, "OK"),
        (child | WS_TABSTOP, [163, 46, 50, 14], IDCANCEL as u16, ATOM_BUTTON, "Cancel"),
    ];
    let mut offsets = Vec::with_capacity(items.len());
    for (style, [x, y, cx, cy], id, atom, text) in items {
        while !b.len().is_multiple_of(4) {
            b.push(0);
        }
        offsets.push(b.len());
        push_u32(&mut b, style);
        push_u32(&mut b, 0); // extended style
        for v in [x, y, cx, cy] {
            push_u16(&mut b, v as u16);
        }
        push_u16(&mut b, id);
        push_u16(&mut b, 0xFFFF); // class by atom
        push_u16(&mut b, atom);
        push_wsz(&mut b, text);
        push_u16(&mut b, 0); // no creation data
    }
    Template { bytes: b, items: offsets }
}

fn push_u16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn push_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}

/// UTF-16, cut at the first NUL, then a terminating NUL.
fn push_wsz(b: &mut Vec<u8>, s: &str) {
    for u in s.encode_utf16().take_while(|u| *u != 0) {
        push_u16(b, u);
    }
    push_u16(b, 0);
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().take_while(|u| *u != 0).chain(std::iter::once(0)).collect()
}

/// What the dialog procedure reads and writes. Lives on `ask`'s stack for the whole modal loop.
struct State {
    initial: Vec<u16>,
    max_len: usize,
    text: Option<String>,
    /// Test only: accept as soon as the dialog exists.
    accept_at_once: bool,
}

#[cfg(test)]
thread_local! {
    /// The test-only switch: the next `ask` on this thread accepts from `WM_INITDIALOG`, so the
    /// round trip runs without a person and cannot hang.
    static ACCEPT_AT_ONCE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn accept_at_once() -> bool {
    ACCEPT_AT_ONCE.with(std::cell::Cell::get)
}

#[cfg(not(test))]
fn accept_at_once() -> bool {
    false
}

/// Ask for one line of text. Modal to `owner` (which may be null); returns the text as typed (not
/// trimmed) on OK or Enter, `None` on Cancel, Esc, the close box, or when Windows refuses the
/// template. `initial` is in the edit box, selected; at most `max_len` characters can be typed (at
/// least 1). Never call it from inside a `with_ui` closure.
pub(crate) fn ask(owner: HWND, title: &str, label: &str, initial: &str, max_len: usize) -> Option<String> {
    let t = template(title, label, FACE, POINT_SIZE);
    // `DialogBoxIndirectParamW` needs the template on a DWORD boundary; a `Vec<u8>` promises one byte.
    let mut words = vec![0u32; t.bytes.len().div_ceil(4)];
    for (i, chunk) in t.bytes.chunks(4).enumerate() {
        let mut w = [0u8; 4];
        w[..chunk.len()].copy_from_slice(chunk);
        words[i] = u32::from_le_bytes(w);
    }
    let mut state = State { initial: wide(initial), max_len: max_len.max(1), text: None, accept_at_once: accept_at_once() };
    let result = unsafe {
        DialogBoxIndirectParamW(
            GetModuleHandleW(null()),
            words.as_ptr() as *const DLGTEMPLATE,
            owner,
            Some(dialog_proc),
            &mut state as *mut State as LPARAM,
        )
    };
    if result == IDOK as isize {
        state.text
    } else {
        None
    }
}

unsafe extern "system" fn dialog_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            SetWindowLongPtrW(hwnd, DWLP_USER, lp);
            let state = lp as *const State;
            let edit = GetDlgItem(hwnd, i32::from(ID_EDIT));
            if !state.is_null() {
                SendMessageW(edit, EM_LIMITTEXT, (*state).max_len, 0);
                SetDlgItemTextW(hwnd, i32::from(ID_EDIT), (*state).initial.as_ptr());
            }
            SendMessageW(edit, EM_SETSEL, 0, -1);
            SetForegroundWindow(hwnd);
            SetFocus(edit);
            if !state.is_null() && (*state).accept_at_once {
                // Through the queue, so the test takes the same OK path a click does.
                PostMessageW(hwnd, WM_COMMAND, IDOK as usize, 0);
            }
            // Focus was set here: tell the dialog manager not to move it.
            0
        }
        WM_COMMAND => match (wp & 0xFFFF) as i32 {
            IDOK => {
                let state = GetWindowLongPtrW(hwnd, DWLP_USER) as *mut State;
                if !state.is_null() {
                    (*state).text = Some(edit_text(GetDlgItem(hwnd, i32::from(ID_EDIT))));
                }
                EndDialog(hwnd, IDOK as isize);
                1
            }
            IDCANCEL => {
                EndDialog(hwnd, IDCANCEL as isize);
                1
            }
            _ => 0,
        },
        _ => 0,
    }
}

unsafe fn edit_text(edit: HWND) -> String {
    let n = GetWindowTextLengthW(edit).max(0) as usize;
    let mut buf = vec![0u16; n + 1];
    let got = GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
    String::from_utf16_lossy(&buf[..got.min(n)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([b[at], b[at + 1]])
    }

    #[test]
    fn template_items_are_aligned_and_numbered() {
        let t = template("Add symbol", "CNBC symbol (S&P):", FACE, POINT_SIZE);
        assert_eq!(u16_at(&t.bytes, 8), 4, "the header's item count");
        assert_eq!(t.items.len(), 4);
        assert!(t.items.iter().all(|o| o.is_multiple_of(4)), "every item on a 4-byte boundary: {:?}", t.items);
        // DLGITEMTEMPLATE: style, exstyle (4 + 4), x, y, cx, cy (2 each), then the id.
        let ids: Vec<u16> = t.items.iter().map(|o| u16_at(&t.bytes, o + 16)).collect();
        assert_eq!(ids, [ID_LABEL, ID_EDIT, IDOK as u16, IDCANCEL as u16]);
        let atoms: Vec<u16> = t.items.iter().map(|o| u16_at(&t.bytes, o + 20)).collect();
        assert_eq!(atoms, [ATOM_STATIC, ATOM_EDIT, ATOM_BUTTON, ATOM_BUTTON]);
        // Each label edge case keeps every item aligned and the ids in order.
        for label in ["", "ABCD", "AB\0CD", "a\u{1F600}b"] {
            let t = template("", label, FACE, POINT_SIZE);
            assert_eq!(t.items.len(), 4, "{label:?}");
            assert!(t.items.iter().all(|o| o.is_multiple_of(4)), "{label:?}: {:?}", t.items);
            let ids: Vec<u16> = t.items.iter().map(|o| u16_at(&t.bytes, o + 16)).collect();
            assert_eq!(ids, [ID_LABEL, ID_EDIT, IDOK as u16, IDCANCEL as u16], "{label:?}");
        }
        // The label text starts after the class atom, at offset 22 of the first item; a NUL ends it.
        let read = |label: &str| {
            let t = template("", label, FACE, POINT_SIZE);
            let units: Vec<u16> = (0..).map(|i| u16_at(&t.bytes, t.items[0] + 22 + 2 * i)).take_while(|u| *u != 0).collect();
            String::from_utf16(&units).unwrap()
        };
        assert_eq!(read(""), "");
        assert_eq!(read("ABCD"), "ABCD", "an odd unit count (with the NUL) is padded before the next item");
        assert_eq!(read("AB\0CD"), "AB", "cut at the embedded NUL");
        assert_eq!(read("a\u{1F600}b"), "a\u{1F600}b", "a non-BMP character is two units");
    }

    #[test]
    fn the_dialog_accepts_itself_and_returns_the_text() {
        ACCEPT_AT_ONCE.with(|c| c.set(true));
        let got = ask(std::ptr::null_mut(), "Test prompt", "Type:", "4,200.50", 32);
        ACCEPT_AT_ONCE.with(|c| c.set(false));
        assert_eq!(got.as_deref(), Some("4,200.50"), "Windows took the template and the edit kept the text");

        // Not tested here: a `max_len` shorter than `initial`. Under Wine the full initial text stays
        // after EM_LIMITTEXT and SetDlgItemTextW, so the cut is not proven on this machine.
    }
}
