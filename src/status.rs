//! Status bar in the Windows 11 layout, painted by hand so it follows the theme:
//! Ln, Col | characters | message ... zoom | line endings | encoding.

use crate::{muldiv, theme, wide};
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const POS: usize = 0;
pub const CHARS: usize = 1;
pub const MESSAGE: usize = 2;
pub const ZOOM: usize = 3;
pub const EOL: usize = 4;
pub const ENCODING: usize = 5;
/// Posted to the parent as WM_COMMAND when a clickable message is clicked.
pub const MESSAGE_CLICKED: u16 = 0x7F01;

const CLASS: &str = "RENotepadStatus";
// Geometry at 96 DPI, measured from Windows 11 Notepad.
const HEIGHT: i32 = 32;
const INSET: i32 = 11;
const PAD: i32 = 12;
/// Minimum widths of the left parts (Ln/Col, characters); they grow to fit.
const LEFT_MIN: [i32; 2] = [85, 208];
/// Fixed widths of the right parts: zoom, line endings, encoding.
const RIGHT: [i32; 3] = [68, 168, 136];

pub struct Status {
    pub hwnd: HWND,
    font: HFONT,
    dpi: u32,
    texts: [Vec<u16>; 6],
    /// The message part is clickable (hand cursor, WM_COMMAND MESSAGE_CLICKED).
    pub link: bool,
}

impl Status {
    pub fn empty() -> Status {
        Status { hwnd: std::ptr::null_mut(), font: std::ptr::null_mut(), dpi: 96, texts: Default::default(), link: false }
    }

    pub fn create(parent: HWND, dpi: u32) -> Status {
        unsafe {
            let class = wide(CLASS);
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc_),
                hInstance: crate::dialogs::hinstance(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(0, class.as_ptr(), std::ptr::null(), WS_CHILD, 0, 0, 0, 0, parent, std::ptr::null_mut(), wc.hInstance, std::ptr::null());
            let mut s = Status { hwnd, font: std::ptr::null_mut(), dpi, texts: Default::default(), link: false };
            s.set_dpi(dpi);
            s
        }
    }

    fn s(&self, v: i32) -> i32 {
        muldiv(v, self.dpi as i32, 96)
    }

    pub fn height(&self) -> i32 {
        self.s(HEIGHT)
    }

    pub fn set_dpi(&mut self, dpi: u32) {
        self.dpi = dpi;
        unsafe {
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
        }
        self.font = theme::font(theme::ui_face(), 12, dpi);
    }

    pub fn set(&mut self, part: usize, text: &str) {
        let w: Vec<u16> = text.encode_utf16().collect();
        if self.texts[part] != w {
            self.texts[part] = w;
            unsafe { InvalidateRect(self.hwnd, std::ptr::null(), 0) };
        }
    }

    /// Is client x inside the message text (not just its slot)?
    fn over_message(&self, x: i32) -> bool {
        let t = &self.texts[MESSAGE];
        if t.is_empty() {
            return false;
        }
        unsafe {
            let hdc = GetDC(self.hwnd);
            let old = SelectObject(hdc, self.font);
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(self.hwnd, &mut rc);
            let (l, r) = self.parts(hdc, rc.right)[MESSAGE];
            let mut tr = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            DrawTextW(hdc, t.as_ptr(), t.len() as i32, &mut tr, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
            SelectObject(hdc, old);
            ReleaseDC(self.hwnd, hdc);
            let start = l + self.s(PAD);
            x >= start && x < (start + tr.right).min(r)
        }
    }

    /// Left/right x of every part for a bar `width` wide.
    fn parts(&self, hdc: HDC, width: i32) -> [(i32, i32); 6] {
        let mut out = [(0, 0); 6];
        let mut x = self.s(INSET);
        for (part, min) in [POS, CHARS].into_iter().zip(LEFT_MIN) {
            let t = &self.texts[part];
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            if !t.is_empty() {
                unsafe { DrawTextW(hdc, t.as_ptr(), t.len() as i32, &mut r, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX) };
            }
            let w = self.s(min).max(r.right + 2 * self.s(PAD));
            out[part] = (x, x + w);
            x += w;
        }
        let mut right = width;
        for (part, w) in [ENCODING, EOL, ZOOM].into_iter().zip(RIGHT.iter().rev()) {
            out[part] = (right - self.s(*w), right);
            right -= self.s(*w);
        }
        out[MESSAGE] = (x, right.max(x));
        out
    }

    fn paint(&self) {
        unsafe {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(self.hwnd, &mut ps);
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(self.hwnd, &mut rc);
            let p = theme::palette();
            theme::fill(hdc, &rc, p.surface);
            theme::fill(hdc, &RECT { bottom: rc.top + 1, ..rc }, p.line);
            let old = SelectObject(hdc, self.font);
            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, p.text_secondary);
            let mid = (rc.top + 1 + rc.bottom) / 2;
            for (i, (l, r)) in self.parts(hdc, rc.right).iter().enumerate() {
                if i != POS && r > l {
                    let sep = RECT { left: *l, top: mid - self.s(9), right: *l + 1, bottom: mid + self.s(9) };
                    theme::fill(hdc, &sep, p.separator);
                }
                let t = &self.texts[i];
                if t.is_empty() || r <= l {
                    continue; // an empty Vec's pointer is dangling; DrawText may still touch it
                }
                let mut tr = RECT { left: l + self.s(PAD), top: rc.top + 1, right: r - self.s(4), bottom: rc.bottom };
                DrawTextW(hdc, t.as_ptr(), t.len() as i32, &mut tr, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS);
            }
            SelectObject(hdc, old);
            EndPaint(self.hwnd, &ps);
        }
    }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    match msg {
        WM_PAINT => {
            if let Some(app) = crate::app::try_app() {
                if app.status.hwnd == hwnd {
                    app.status.paint();
                    return 0;
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_ERASEBKGND => 1,
        WM_SETCURSOR | WM_LBUTTONUP => {
            let Some(app) = crate::app::try_app().filter(|a| a.status.hwnd == hwnd && a.status.link) else {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            };
            let x = if msg == WM_LBUTTONUP {
                (lparam & 0xFFFF) as i16 as i32
            } else {
                let mut pt = std::mem::zeroed();
                GetCursorPos(&mut pt);
                ScreenToClient(hwnd, &mut pt);
                pt.x
            };
            if !app.status.over_message(x) {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            if msg == WM_SETCURSOR {
                SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_HAND));
                return 1;
            }
            PostMessageW(GetParent(hwnd), WM_COMMAND, MESSAGE_CLICKED as usize, 0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
