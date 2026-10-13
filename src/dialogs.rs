//! Dialogs. Find/Replace are built from in-memory templates, so they can be
//! themed dark (comdlg's can't be). Message boxes, Go to line and About are
//! Windows 11 content dialogs (`content_dialog`).

use crate::{theme, wide};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const BUTTON: u16 = 0x0080;
pub const EDIT: u16 = 0x0081;
pub const STATIC: u16 = 0x0082;

pub const SS_NOTIFY: u32 = 0x100;
pub const STN_CLICKED: u32 = 0;
/// Labels for caption-less checkboxes/radios use id + LABEL_OFFSET.
pub const LABEL_OFFSET: u16 = 100;

/// DLGTEMPLATE + DLGITEMTEMPLATEs, Segoe UI 9.
pub struct Template {
    words: Vec<u16>,
    count: u16,
}

impl Template {
    pub fn new(title: &str, extra_style: u32, cx: i16, cy: i16) -> Template {
        let style = DS_SETFONT as u32 | DS_MODALFRAME as u32 | WS_POPUP | WS_CAPTION | WS_SYSMENU | extra_style;
        let mut t = Template { words: Vec::new(), count: 0 };
        t.dword(style);
        t.dword(0);
        t.words.push(0); // item count, patched in build()
        for v in [0i16, 0, cx, cy] {
            t.words.push(v as u16);
        }
        t.words.push(0); // no menu
        t.words.push(0); // default class
        t.string(title);
        t.words.push(9);
        t.string("Segoe UI");
        t
    }

    fn dword(&mut self, v: u32) {
        self.words.push(v as u16);
        self.words.push((v >> 16) as u16);
    }

    fn string(&mut self, s: &str) {
        self.words.extend(s.encode_utf16());
        self.words.push(0);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn item(&mut self, class: u16, text: &str, id: u16, style: u32, x: i16, y: i16, cx: i16, cy: i16) -> &mut Self {
        if self.words.len() % 2 == 1 {
            self.words.push(0);
        }
        self.dword(WS_CHILD | WS_VISIBLE | style);
        self.dword(0);
        for v in [x, y, cx, cy] {
            self.words.push(v as u16);
        }
        self.words.push(id);
        self.words.push(0xFFFF);
        self.words.push(class);
        self.string(text);
        self.words.push(0);
        self.count += 1;
        self
    }

    pub fn label(&mut self, text: &str, x: i16, y: i16, cx: i16, cy: i16) -> &mut Self {
        self.item(STATIC, text, 0xFFFF, 0, x, y, cx, cy)
    }

    pub fn button(&mut self, text: &str, id: u16, default: bool, x: i16, y: i16) -> &mut Self {
        let style = WS_TABSTOP | if default { BS_DEFPUSHBUTTON as u32 } else { BS_PUSHBUTTON as u32 };
        self.item(BUTTON, text, id, style, x, y, 56, 14)
    }

    /// Checkbox or radio with a separate clickable label (themed check text ignores
    /// dialog colors in dark mode).
    #[allow(clippy::too_many_arguments)]
    pub fn check(&mut self, text: &str, id: u16, radio: bool, group: bool, x: i16, y: i16, cx: i16) -> &mut Self {
        let kind = if radio { BS_AUTORADIOBUTTON } else { BS_AUTOCHECKBOX } as u32;
        let style = kind | WS_TABSTOP | if group { WS_GROUP } else { 0 };
        self.item(BUTTON, "", id, style, x, y, 10, 10);
        self.item(STATIC, text, id + LABEL_OFFSET, SS_NOTIFY, x + 12, y + 1, cx - 12, 9)
    }

    /// Bytes for DialogBoxIndirectParam, DWORD-aligned as the API requires.
    pub fn build(&self) -> Vec<u32> {
        let mut w = self.words.clone();
        w[4] = self.count;
        if w.len() % 2 == 1 {
            w.push(0);
        }
        w.as_chunks::<2>().0.iter().map(|c| c[0] as u32 | (c[1] as u32) << 16).collect()
    }
}

pub fn hinstance() -> windows_sys::Win32::Foundation::HINSTANCE {
    unsafe { GetModuleHandleW(std::ptr::null()) }
}

pub fn center_on_owner(dlg: HWND) {
    unsafe {
        let owner = GetWindow(dlg, GW_OWNER);
        if owner.is_null() {
            return;
        }
        let (mut o, mut d): (RECT, RECT) = (std::mem::zeroed(), std::mem::zeroed());
        GetWindowRect(owner, &mut o);
        GetWindowRect(dlg, &mut d);
        let x = o.left + ((o.right - o.left) - (d.right - d.left)) / 2;
        let y = o.top + ((o.bottom - o.top) - (d.bottom - d.top)) / 3;
        SetWindowPos(dlg, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
    }
}

/// Toggle a checkbox/radio when its separate label is clicked.
pub fn forward_label_click(dlg: HWND, id: u16, code: u32) -> bool {
    if code == STN_CLICKED && id > LABEL_OFFSET && id != 0xFFFF {
        unsafe {
            let target = GetDlgItem(dlg, (id - LABEL_OFFSET) as i32);
            if !target.is_null() && theme::is_check_or_radio(target) {
                SendMessageW(target, BM_CLICK, 0, 0);
                return true;
            }
        }
    }
    false
}

/// Alt+letter on a separate check/radio label: click the box, as Windows does
/// for a normal checkbox caption. Call from the message loop before IsDialogMessage.
pub fn check_mnemonic(dlg: HWND, msg: &MSG) -> bool {
    if msg.message != WM_SYSCHAR {
        return false;
    }
    let key = char::from_u32(msg.wParam as u32).map(|c| c.to_lowercase().next().unwrap_or(c));
    unsafe {
        let mut child = GetWindow(dlg, GW_CHILD);
        while !child.is_null() {
            let id = GetDlgCtrlID(child) as u16;
            if id > LABEL_OFFSET && id != 0xFFFF {
                let target = GetDlgItem(dlg, (id - LABEL_OFFSET) as i32);
                if !target.is_null() && theme::is_check_or_radio(target) {
                    let text = get_text(child);
                    let hot = text.split('&').nth(1).and_then(|s| s.chars().next()).map(|c| c.to_lowercase().next().unwrap_or(c));
                    if hot.is_some() && hot == key {
                        SetFocus(target);
                        SendMessageW(target, BM_CLICK, 0, 0);
                        return true;
                    }
                }
            }
            child = GetWindow(child, GW_HWNDNEXT);
        }
    }
    false
}

pub fn get_text(hwnd: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(hwnd, buf.as_mut_ptr(), n + 1);
        String::from_utf16_lossy(&buf[..got.max(0) as usize])
    }
}

pub fn set_text(hwnd: HWND, s: &str) {
    let w = wide(s);
    unsafe { SetWindowTextW(hwnd, w.as_ptr()) };
}

// ---------------------------------------------------------------- Windows 11 dialogs

use crate::content_dialog::{self, Input, Spec};

/// Replacement for MessageBox in the Windows 11 style. `buttons` are (caption, id);
/// the first is the accent button and has the focus.
pub fn message_box(owner: HWND, title: &str, text: &str, buttons: &[(&str, i32)]) -> i32 {
    content_dialog::show(owner, &Spec { title, text, buttons, icon: false, input: None }).id
}

/// Returns the 1-based line the user asked for.
pub fn goto_line(owner: HWND, current: u32, max: u32) -> Option<u32> {
    let check = |s: &str| match s.trim().parse::<u32>() {
        Ok(n) if n >= 1 && n <= max => Ok(()),
        _ => Err("The line number is beyond the total number of lines".to_string()),
    };
    let current = current.to_string();
    let spec = Spec {
        title: "Go to line",
        text: "Line number",
        buttons: &[("Go to", IDOK), ("Cancel", IDCANCEL)],
        icon: false,
        input: Some(Input { text: &current, numeric: true, check: &check }),
    };
    let answer = content_dialog::show(owner, &spec);
    if answer.id == IDOK { answer.input.trim().parse().ok() } else { None }
}

// ---------------------------------------------------------------- about

const STORY: &str = "Windows 10's Notepad, reverse-engineered and rebuilt in Rust.

The real notepad.exe from Windows 10 22H2 was taken apart with REA and Ghidra to learn exactly how it behaves: how it detects encodings, keeps line endings, stamps .LOG files, finds and replaces. Every one of those rules was then written again from scratch. No Microsoft code is inside.

It wears the Windows 11 look, with tabs and dark mode by default, and leaves out the rest: no Copilot, no Bing, no sign-in, no telemetry.

MIT licensed.";

pub fn about(owner: HWND) {
    let text = format!("Version {} \u{b7} by Nikko Gonzales\n\n{STORY}", env!("CARGO_PKG_VERSION"));
    let spec = Spec { title: crate::app::APP_NAME, text: &text, buttons: &[("OK", IDOK)], icon: true, input: None };
    content_dialog::show(owner, &spec);
}
