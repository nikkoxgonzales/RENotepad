//! Windows 11 content dialogs (the WinUI ContentDialog look, as in Windows 11
//! Notepad's "Do you want to save changes" prompt): a rounded card with a
//! title, a message, an optional text box, and up to three equal-width
//! buttons, the first one in the accent color. The owner window is dimmed
//! behind it. Painted by hand with GDI and run in its own modal loop.
//!
//! Test hook: a posted `WM_COMMAND(id)` with lParam 0 presses the button with
//! that id, as a click would.

use crate::{muldiv, theme, wide};
use std::cell::Cell;
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_BINARY};
use windows_sys::Win32::UI::Controls::{EM_SETMARGINS, EM_SETSEL, WM_MOUSELEAVE};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const CLASS: &str = "RENotepadDialog";
const SMOKE_CLASS: &str = "RENotepadSmoke";
/// Control id of the text box, so tests can find it with GetDlgItem.
pub const INPUT_ID: i32 = 258;

// Geometry at 96 DPI, measured from Windows 11 Notepad.
const MIN_W: i32 = 320;
const MAX_W: i32 = 548;
const PAD: i32 = 24;
const TITLE_H: i32 = 28;
const TITLE_GAP: i32 = 12;
const ICON: i32 = 32;
const ICON_GAP: i32 = 12;
const BUTTON_H: i32 = 32;
const BUTTON_GAP: i32 = 8;
const BUTTON_PAD: i32 = 8;
const BUTTON_RADIUS: i32 = 4;
const INPUT_GAP: i32 = 8;
const INPUT_H: i32 = 32;
const INPUT_PAD: i32 = 11;
const TITLE_PX: i32 = 20;
const TEXT_PX: i32 = 14;
/// How dark the owner gets behind the dialog (0-255).
const SMOKE_ALPHA: u32 = 77;

pub struct Input<'a> {
    pub text: &'a str,
    pub numeric: bool,
    /// Runs when the first button is pressed; `Err(message)` keeps the dialog open
    /// and shows the message.
    pub check: &'a dyn Fn(&str) -> Result<(), String>,
}

pub struct Spec<'a> {
    pub title: &'a str,
    pub text: &'a str,
    /// (caption, id). The first is the accent button and has the focus; `&` marks
    /// an access key.
    pub buttons: &'a [(&'a str, i32)],
    /// The app icon beside the title.
    pub icon: bool,
    pub input: Option<Input<'a>>,
}

pub struct Answer {
    pub id: i32,
    pub input: String,
}

struct Colors {
    body: COLORREF,
    footer: COLORREF,
    divider: COLORREF,
    border: COLORREF,
    title: COLORREF,
    text: COLORREF,
    button: COLORREF,
    button_hover: COLORREF,
    button_pressed: COLORREF,
    button_border: COLORREF,
    button_text: COLORREF,
    button_text_pressed: COLORREF,
    input_rest: COLORREF,
    input_focus: COLORREF,
    input_border: COLORREF,
    input_bottom: COLORREF,
    focus_outer: COLORREF,
    focus_inner: COLORREF,
    on_accent: COLORREF,
}

const DARK: Colors = Colors {
    body: theme::rgb(43, 43, 43),
    footer: theme::rgb(32, 32, 32),
    divider: theme::rgb(29, 29, 29),
    border: theme::rgb(61, 61, 61),
    title: theme::rgb(255, 255, 255),
    text: theme::rgb(240, 240, 240),
    button: theme::rgb(45, 45, 45),
    button_hover: theme::rgb(50, 50, 50),
    button_pressed: theme::rgb(40, 40, 40),
    button_border: theme::rgb(53, 53, 53),
    button_text: theme::rgb(242, 242, 242),
    button_text_pressed: theme::rgb(206, 206, 206),
    input_rest: theme::rgb(55, 55, 55),
    input_focus: theme::rgb(31, 31, 31),
    input_border: theme::rgb(58, 58, 58),
    input_bottom: theme::rgb(154, 154, 154),
    focus_outer: theme::rgb(255, 255, 255),
    focus_inner: theme::rgb(10, 10, 10),
    on_accent: theme::rgb(0, 0, 0),
};

const LIGHT: Colors = Colors {
    body: theme::rgb(255, 255, 255),
    footer: theme::rgb(243, 243, 243),
    divider: theme::rgb(229, 229, 229),
    border: theme::rgb(202, 202, 202),
    title: theme::rgb(26, 26, 26),
    text: theme::rgb(26, 26, 26),
    button: theme::rgb(251, 251, 251),
    button_hover: theme::rgb(246, 246, 246),
    button_pressed: theme::rgb(245, 245, 245),
    button_border: theme::rgb(229, 229, 229),
    button_text: theme::rgb(26, 26, 26),
    button_text_pressed: theme::rgb(95, 95, 95),
    input_rest: theme::rgb(251, 251, 251),
    input_focus: theme::rgb(255, 255, 255),
    input_border: theme::rgb(229, 229, 229),
    input_bottom: theme::rgb(134, 134, 134),
    focus_outer: theme::rgb(28, 28, 28),
    focus_inner: theme::rgb(255, 255, 255),
    on_accent: theme::rgb(255, 255, 255),
};

/// The accent the system uses for buttons: AccentPalette entry Light2 in dark
/// mode, Dark1 in light mode (8 RGBA entries, Light3 first). Falls back to
/// Windows 11's default blue.
fn accent(dark: bool) -> COLORREF {
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent");
    let name = wide("AccentPalette");
    let mut buf = [0u8; 32];
    let mut size = buf.len() as u32;
    let ok = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_BINARY, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size)
    } == 0
        && size >= 32;
    let i = if dark { 1 } else { 4 } * 4;
    match (ok, dark) {
        (true, _) => theme::rgb(buf[i], buf[i + 1], buf[i + 2]),
        (false, true) => theme::rgb(96, 205, 255),
        (false, false) => theme::rgb(0, 95, 184),
    }
}

/// The access key after the first `&`, lowercased.
fn hotkey(caption: &str) -> Option<char> {
    caption.split('&').nth(1).and_then(|s| s.chars().next()).and_then(|c| c.to_lowercase().next())
}

/// Esc / close answers with Cancel if there is one, else the last button.
fn cancel_id(buttons: &[(&str, i32)]) -> i32 {
    if buttons.iter().any(|b| b.1 == IDCANCEL) {
        IDCANCEL
    } else {
        buttons.last().map_or(IDCANCEL, |b| b.1)
    }
}

/// Left edge and width of each button across `content` px. One button takes the
/// right half, as in WinUI; more split the row evenly.
fn button_columns(n: usize, content: i32, gap: i32) -> Vec<(i32, i32)> {
    let cols = n.max(2) as i32;
    let w = (content - gap * (cols - 1)) / cols;
    let first = cols - n as i32;
    (0..n as i32)
        .map(|i| {
            let x = (first + i) * (w + gap);
            let last = i == n as i32 - 1;
            (x, if last { content - x } else { w })
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Input,
    Button(usize),
}

struct Button {
    caption: Vec<u16>,
    id: i32,
    key: Option<char>,
    rc: RECT,
}

struct State {
    w: i32,
    h: i32,
    body_h: i32,
    title: Vec<u16>,
    text: Vec<u16>,
    title_rc: RECT,
    text_rc: RECT,
    icon: HICON,
    icon_px: i32,
    input_rc: Option<RECT>,
    buttons: Vec<Button>,
    title_font: HFONT,
    text_font: HFONT,
    colors: &'static Colors,
    accent: COLORREF,
    radius: i32,
    cancel: i32,
    /// Windows 10 has no DWM border or rounding: draw a plain border.
    own_border: Cell<bool>,
    edit: Cell<HWND>,
    focus: Cell<Focus>,
    hover: Cell<Option<usize>>,
    pressed: Cell<Option<usize>>,
    tracking: Cell<bool>,
    show_keys: Cell<bool>,
    /// A button was pressed; the modal loop decides (it may need to validate).
    request: Cell<Option<usize>>,
    result: Cell<Option<i32>>,
}

thread_local! {
    /// Open dialogs; only the outermost one dims the owner.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

fn register() {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| unsafe {
        let hinst = crate::dialogs::hinstance();
        let cls = wide(CLASS);
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.style = CS_DROPSHADOW;
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = hinst;
        wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
        wc.lpszClassName = cls.as_ptr();
        RegisterClassW(&wc);
        let smoke = wide(SMOKE_CLASS);
        wc.style = 0;
        wc.lpfnWndProc = Some(DefWindowProcW);
        wc.hbrBackground = GetStockObject(BLACK_BRUSH) as HBRUSH;
        wc.lpszClassName = smoke.as_ptr();
        RegisterClassW(&wc);
    });
}

fn measure(font: HFONT, text: &[u16], width: Option<i32>) -> (i32, i32) {
    unsafe {
        let hdc = GetDC(std::ptr::null_mut());
        let old = SelectObject(hdc, font);
        let mut rc = RECT { left: 0, top: 0, right: width.unwrap_or(0), bottom: 0 };
        let wrap = if width.is_some() { DT_WORDBREAK | DT_EDITCONTROL } else { 0 };
        DrawTextW(hdc, text.as_ptr(), text.len() as i32, &mut rc, DT_CALCRECT | DT_NOPREFIX | wrap);
        SelectObject(hdc, old);
        ReleaseDC(std::ptr::null_mut(), hdc);
        (rc.right - rc.left, rc.bottom - rc.top)
    }
}

fn line_height(font: HFONT) -> i32 {
    measure(font, &[b'A' as u16, b'g' as u16], None).1
}

impl State {
    fn new(spec: &Spec, dpi: u32) -> State {
        let s = |v: i32| muldiv(v, dpi as i32, 96);
        let title_font = theme::font_weight(theme::ui_face(), TITLE_PX, dpi, 600);
        let text_font = theme::font(theme::ui_face(), TEXT_PX, dpi);
        let title: Vec<u16> = spec.title.encode_utf16().collect();
        let text: Vec<u16> = spec.text.encode_utf16().collect();
        let icon_off = if spec.icon { s(ICON + ICON_GAP) } else { 0 };

        // As wide as the content wants, within WinUI's 320-548.
        let n = spec.buttons.len().max(1) as i32;
        let widest_caption = spec.buttons.iter().map(|b| measure(text_font, &b.0.replace('&', "").encode_utf16().collect::<Vec<_>>(), None).0).max().unwrap_or(0);
        let buttons_need = (widest_caption + s(BUTTON_PAD) * 2) * n.max(2) + s(BUTTON_GAP) * (n.max(2) - 1);
        let content_need = measure(text_font, &text, None).0.max(measure(title_font, &title, None).0 + icon_off).max(buttons_need);
        let w = (content_need + s(PAD) * 2).clamp(s(MIN_W), s(MAX_W));
        let content = w - s(PAD) * 2;

        let title_h = if spec.icon { s(ICON) } else { s(TITLE_H) };
        let title_rc = RECT { left: s(PAD) + icon_off, top: s(PAD), right: w - s(PAD), bottom: s(PAD) + title_h };
        let text_top = title_rc.bottom + s(TITLE_GAP);
        let text_h = if text.is_empty() { 0 } else { measure(text_font, &text, Some(content)).1 };
        let text_rc = RECT { left: s(PAD), top: text_top, right: w - s(PAD), bottom: text_top + text_h };
        let mut y = text_rc.bottom;
        let input_rc = spec.input.as_ref().map(|_| {
            y += s(INPUT_GAP);
            let rc = RECT { left: s(PAD), top: y, right: w - s(PAD), bottom: y + s(INPUT_H) };
            y = rc.bottom;
            rc
        });
        let body_h = y + s(PAD);
        let buttons_top = body_h + 1 + s(PAD);
        let buttons = spec
            .buttons
            .iter()
            .zip(button_columns(spec.buttons.len(), content, s(BUTTON_GAP)))
            .map(|(&(cap, id), (x, bw))| Button {
                caption: cap.encode_utf16().collect(),
                id,
                key: hotkey(cap),
                rc: RECT { left: s(PAD) + x, top: buttons_top, right: s(PAD) + x + bw, bottom: buttons_top + s(BUTTON_H) },
            })
            .collect();
        let icon_px = s(ICON);
        let icon = if spec.icon {
            unsafe { LoadImageW(crate::dialogs::hinstance(), 1 as _, IMAGE_ICON, icon_px, icon_px, LR_DEFAULTCOLOR) as HICON }
        } else {
            std::ptr::null_mut()
        };
        let dark = theme::is_dark();
        State {
            w,
            h: buttons_top + s(BUTTON_H) + s(PAD),
            body_h,
            title,
            text,
            title_rc,
            text_rc,
            icon,
            icon_px,
            input_rc,
            buttons,
            title_font,
            text_font,
            colors: if dark { &DARK } else { &LIGHT },
            accent: accent(dark),
            radius: s(BUTTON_RADIUS),
            cancel: cancel_id(spec.buttons),
            own_border: Cell::new(false),
            edit: Cell::new(std::ptr::null_mut()),
            focus: Cell::new(if spec.input.is_some() { Focus::Input } else { Focus::Button(0) }),
            hover: Cell::new(None),
            pressed: Cell::new(None),
            tracking: Cell::new(false),
            show_keys: Cell::new(false),
            request: Cell::new(None),
            result: Cell::new(None),
        }
    }

    fn button_at(&self, x: i32, y: i32) -> Option<usize> {
        self.buttons.iter().position(|b| x >= b.rc.left && x < b.rc.right && y >= b.rc.top && y < b.rc.bottom)
    }

    /// Tab order: the text box, then the buttons.
    fn focus_order(&self) -> Vec<Focus> {
        let input = self.input_rc.map(|_| Focus::Input);
        input.into_iter().chain((0..self.buttons.len()).map(Focus::Button)).collect()
    }
}

impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.title_font);
            DeleteObject(self.text_font);
            if !self.icon.is_null() {
                DestroyIcon(self.icon);
            }
        }
    }
}

unsafe fn state<'a>(hwnd: HWND) -> Option<&'a State> {
    (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State).as_ref()
}

/// Wake the modal loop: a sent message (tests use them) is handled inside
/// GetMessage, which would otherwise keep waiting.
unsafe fn wake(hwnd: HWND) {
    PostMessageW(hwnd, WM_NULL, 0, 0);
}

unsafe fn set_focus(st: &State, dlg: HWND, f: Focus) {
    st.focus.set(f);
    let edit = st.edit.get();
    if GetActiveWindow() == dlg {
        match f {
            Focus::Input => {
                SetFocus(edit);
                SendMessageW(edit, EM_SETSEL, 0, -1);
            }
            Focus::Button(_) => {
                SetFocus(dlg);
            }
        }
    }
    InvalidateRect(dlg, std::ptr::null(), 0);
    if !edit.is_null() {
        InvalidateRect(edit, std::ptr::null(), 1);
    }
}

/// Keyboard, handled before TranslateMessage. true = consumed.
unsafe fn pre_translate(st: &State, dlg: HWND, msg: &MSG) -> bool {
    if msg.hwnd != dlg && IsChild(dlg, msg.hwnd) == 0 {
        return false;
    }
    let focus = st.focus.get();
    match msg.message {
        WM_KEYDOWN => match msg.wParam as u16 {
            VK_TAB => {
                let order = st.focus_order();
                let i = order.iter().position(|&f| f == focus).unwrap_or(0);
                let back = GetKeyState(VK_SHIFT as i32) < 0;
                let n = order.len();
                set_focus(st, dlg, order[if back { (i + n - 1) % n } else { (i + 1) % n }]);
                true
            }
            VK_RETURN => {
                st.request.set(Some(match focus {
                    Focus::Button(i) => i,
                    Focus::Input => 0,
                }));
                true
            }
            VK_ESCAPE => {
                st.result.set(Some(st.cancel));
                true
            }
            VK_SPACE => match focus {
                Focus::Button(i) => {
                    st.request.set(Some(i));
                    true
                }
                Focus::Input => false,
            },
            VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN => match focus {
                Focus::Button(i) => {
                    let forward = matches!(msg.wParam as u16, VK_RIGHT | VK_DOWN);
                    let j = if forward { (i + 1).min(st.buttons.len() - 1) } else { i.saturating_sub(1) };
                    set_focus(st, dlg, Focus::Button(j));
                    true
                }
                Focus::Input => false,
            },
            _ => false,
        },
        WM_SYSKEYDOWN if msg.wParam as u16 == VK_MENU => {
            if !st.show_keys.replace(true) {
                InvalidateRect(dlg, std::ptr::null(), 0);
            }
            false
        }
        // Alt+letter anywhere; a plain letter while a button has the focus.
        WM_SYSCHAR | WM_CHAR => {
            if msg.message == WM_CHAR && focus == Focus::Input {
                return false;
            }
            let c = char::from_u32(msg.wParam as u32).and_then(|c| c.to_lowercase().next());
            if let Some(i) = st.buttons.iter().position(|b| b.key.is_some() && b.key == c) {
                st.request.set(Some(i));
            }
            true
        }
        _ => false,
    }
}

/// Covers the owner with translucent black, like WinUI's smoke layer.
unsafe fn create_smoke(root: HWND, alpha: u32) -> HWND {
    let mut fr: RECT = std::mem::zeroed();
    if DwmGetWindowAttribute(root, 9 /* DWMWA_EXTENDED_FRAME_BOUNDS */, (&mut fr as *mut RECT).cast(), 16) != 0 {
        GetWindowRect(root, &mut fr);
    }
    let cls = wide(SMOKE_CLASS);
    let smoke = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        cls.as_ptr(),
        std::ptr::null(),
        WS_POPUP,
        fr.left,
        fr.top,
        fr.right - fr.left,
        fr.bottom - fr.top,
        root,
        std::ptr::null_mut(),
        crate::dialogs::hinstance(),
        std::ptr::null(),
    );
    if smoke.is_null() {
        return smoke;
    }
    SetLayeredWindowAttributes(smoke, 0, alpha as u8, LWA_ALPHA);
    let corners: i32 = if IsZoomed(root) != 0 { 1 } else { 2 }; // DWMWCP_DONOTROUND / ROUND
    DwmSetWindowAttribute(smoke, 33, (&corners as *const i32).cast(), 4);
    ShowWindow(smoke, SW_SHOWNOACTIVATE);
    smoke
}

/// Where the dialog goes: centered on the owner's visible frame, kept on its monitor.
unsafe fn place(root: HWND, w: i32, h: i32) -> (i32, i32) {
    let mon = MonitorFromWindow(root, MONITOR_DEFAULTTOPRIMARY);
    let mut mi: MONITORINFO = std::mem::zeroed();
    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(mon, &mut mi);
    let work = mi.rcWork;
    let mut fr = work;
    if !root.is_null() && IsWindowVisible(root) != 0 && IsIconic(root) == 0
        && DwmGetWindowAttribute(root, 9, (&mut fr as *mut RECT).cast(), 16) != 0
    {
        GetWindowRect(root, &mut fr);
    }
    let x = fr.left + (fr.right - fr.left - w) / 2;
    let y = fr.top + (fr.bottom - fr.top - h) / 2;
    (x.clamp(work.left, (work.right - w).max(work.left)), y.clamp(work.top, (work.bottom - h).max(work.top)))
}

/// Show a content dialog and wait for an answer.
pub fn show(owner: HWND, spec: &Spec) -> Answer {
    register();
    unsafe {
        let root = if owner.is_null() { owner } else { GetAncestor(owner, GA_ROOTOWNER) };
        let dpi = if owner.is_null() { 96 } else { GetDpiForWindow(owner) };
        let st = Box::new(State::new(spec, dpi));
        let st_ptr = &*st as *const State;

        // A see-through owner (the UI tests' "ghost") gets a see-through dialog that
        // doesn't take the focus.
        let root_ex = if root.is_null() { 0 } else { GetWindowLongW(root, GWL_EXSTYLE) as u32 };
        let mut root_alpha = 255u8;
        if root_ex & WS_EX_LAYERED != 0 {
            let mut flags = 0;
            if GetLayeredWindowAttributes(root, std::ptr::null_mut(), &mut root_alpha, &mut flags) == 0 || flags & LWA_ALPHA == 0 {
                root_alpha = 255;
            }
        }
        let quiet = root_ex & WS_EX_TRANSPARENT != 0;

        let depth = DEPTH.with(|d| d.replace(d.get() + 1));
        let smoke = if depth == 0 && !root.is_null() && IsWindowVisible(root) != 0 && IsIconic(root) == 0 {
            create_smoke(root, (SMOKE_ALPHA * root_alpha as u32).div_ceil(255))
        } else {
            std::ptr::null_mut()
        };

        let (x, y) = place(root, st.w, st.h);
        let ex = if root_alpha < 255 { WS_EX_LAYERED | (root_ex & WS_EX_TRANSPARENT) } else { 0 };
        let cls = wide(CLASS);
        let title = wide(spec.title);
        let dlg = CreateWindowExW(
            ex,
            cls.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_CLIPCHILDREN,
            x,
            y,
            st.w,
            st.h,
            owner,
            std::ptr::null_mut(),
            crate::dialogs::hinstance(),
            st_ptr.cast(),
        );
        if root_alpha < 255 {
            SetLayeredWindowAttributes(dlg, 0, root_alpha, LWA_ALPHA);
        }
        let round: i32 = 2; // DWMWCP_ROUND: Windows 11's 8 px corners
        let rounded = DwmSetWindowAttribute(dlg, 33, (&round as *const i32).cast(), 4) == 0;
        if rounded {
            DwmSetWindowAttribute(dlg, 34 /* DWMWA_BORDER_COLOR */, (&st.colors.border as *const COLORREF).cast(), 4);
        }
        st.own_border.set(!rounded);

        if let (Some(input), Some(rc)) = (&spec.input, st.input_rc) {
            let s = |v: i32| muldiv(v, dpi as i32, 96);
            let eh = line_height(st.text_font);
            let style = WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL as u32 | if input.numeric { ES_NUMBER as u32 } else { 0 };
            let edit_cls = wide("EDIT");
            let text = wide(input.text);
            let edit = CreateWindowExW(
                0,
                edit_cls.as_ptr(),
                text.as_ptr(),
                style,
                rc.left + s(INPUT_PAD),
                rc.top + (rc.bottom - rc.top - eh) / 2,
                rc.right - rc.left - s(INPUT_PAD) * 2,
                eh,
                dlg,
                INPUT_ID as usize as _,
                crate::dialogs::hinstance(),
                std::ptr::null(),
            );
            SendMessageW(edit, WM_SETFONT, st.text_font as usize, 0);
            SendMessageW(edit, EM_SETMARGINS, (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize, 0);
            SendMessageW(edit, EM_SETSEL, 0, -1);
            st.edit.set(edit);
        }

        // Modal: the owner (and the main window behind it) stop taking input.
        let mut disabled = Vec::new();
        for w in [owner, root] {
            if !w.is_null() && !disabled.contains(&w) && IsWindowEnabled(w) != 0 {
                EnableWindow(w, 0);
                disabled.push(w);
            }
        }
        ShowWindow(dlg, if quiet { SW_SHOWNA } else { SW_SHOW });
        set_focus(&st, dlg, st.focus.get());

        let mut quit = None;
        let id = loop {
            if let Some(id) = st.result.get() {
                break id;
            }
            if let Some(i) = st.request.take() {
                let ok = match (&spec.input, i) {
                    (Some(input), 0) => match (input.check)(&crate::dialogs::get_text(st.edit.get())) {
                        Ok(()) => true,
                        Err(e) => {
                            show(dlg, &Spec { title: spec.title, text: &e, buttons: &[("OK", IDOK)], icon: false, input: None });
                            set_focus(&st, dlg, Focus::Input);
                            false
                        }
                    },
                    _ => true,
                };
                if ok {
                    break st.buttons[i].id;
                }
                continue;
            }
            let mut msg: MSG = std::mem::zeroed();
            if GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) <= 0 {
                quit = Some(msg.wParam as i32);
                break st.cancel;
            }
            if pre_translate(&st, dlg, &msg) {
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        };
        let input = if st.edit.get().is_null() { String::new() } else { crate::dialogs::get_text(st.edit.get()) };

        for w in disabled.into_iter().rev() {
            EnableWindow(w, 1);
        }
        if !smoke.is_null() {
            DestroyWindow(smoke);
        }
        DestroyWindow(dlg);
        DEPTH.with(|d| d.set(depth));
        drop(st);
        if let Some(code) = quit {
            PostQuitMessage(code);
        }
        Answer { id, input }
    }
}

unsafe fn paint(st: &State, hdc: HDC) {
    let c = st.colors;
    let (w, h) = (st.w, st.h);
    theme::fill(hdc, &RECT { left: 0, top: 0, right: w, bottom: st.body_h }, c.body);
    theme::fill(hdc, &RECT { left: 0, top: st.body_h, right: w, bottom: st.body_h + 1 }, c.divider);
    theme::fill(hdc, &RECT { left: 0, top: st.body_h + 1, right: w, bottom: h }, c.footer);
    SetBkMode(hdc, TRANSPARENT as i32);

    if !st.icon.is_null() {
        DrawIconEx(hdc, st.text_rc.left, st.title_rc.top, st.icon, st.icon_px, st.icon_px, 0, std::ptr::null_mut(), DI_NORMAL);
    }
    let mut rc = st.title_rc;
    SelectObject(hdc, st.title_font);
    SetTextColor(hdc, c.title);
    DrawTextW(hdc, st.title.as_ptr(), st.title.len() as i32, &mut rc, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX);
    let mut rc = st.text_rc;
    SelectObject(hdc, st.text_font);
    SetTextColor(hdc, c.text);
    DrawTextW(hdc, st.text.as_ptr(), st.text.len() as i32, &mut rc, DT_WORDBREAK | DT_EDITCONTROL | DT_NOPREFIX);

    let focus = st.focus.get();
    if let Some(rc) = st.input_rc {
        let focused = focus == Focus::Input;
        let r = st.radius;
        theme::round_rect(hdc, rc, r, false, c.input_border, c.body);
        let inner = RECT { left: rc.left + 1, top: rc.top + 1, right: rc.right - 1, bottom: rc.bottom - 1 };
        theme::round_rect(hdc, inner, r - 1, false, if focused { c.input_focus } else { c.input_rest }, c.input_border);
        let (line, color) = if focused { (2, st.accent) } else { (1, c.input_bottom) };
        theme::fill(hdc, &RECT { left: rc.left + r, top: rc.bottom - line, right: rc.right - r, bottom: rc.bottom }, color);
    }

    let one = muldiv(1, GetDeviceCaps(hdc, LOGPIXELSX as i32), 96).max(1);
    for (i, b) in st.buttons.iter().enumerate() {
        let r = st.radius;
        let pressed = st.pressed.get() == Some(i) && st.hover.get() == Some(i);
        let hover = st.hover.get() == Some(i) && st.pressed.get().is_none_or(|p| p == i);
        let mut behind = c.footer;
        if focus == Focus::Button(i) {
            let grow = |rc: RECT, d: i32| RECT { left: rc.left - d, top: rc.top - d, right: rc.right + d, bottom: rc.bottom + d };
            theme::round_rect(hdc, grow(b.rc, one * 3), r + one * 3, false, c.focus_outer, c.footer);
            theme::round_rect(hdc, grow(b.rc, one), r + one, false, c.focus_inner, c.focus_outer);
            behind = c.focus_inner;
        }
        let text_color = if i == 0 {
            let fill = if pressed { theme::mix(st.accent, c.footer, 0.2) } else if hover { theme::mix(st.accent, c.footer, 0.1) } else { st.accent };
            theme::round_rect(hdc, b.rc, r, false, fill, behind);
            if pressed { theme::mix(c.on_accent, st.accent, 0.3) } else { c.on_accent }
        } else {
            let fill = if pressed { c.button_pressed } else if hover { c.button_hover } else { c.button };
            theme::round_rect(hdc, b.rc, r, false, c.button_border, behind);
            let inner = RECT { left: b.rc.left + 1, top: b.rc.top + 1, right: b.rc.right - 1, bottom: b.rc.bottom - 1 };
            theme::round_rect(hdc, inner, r - 1, false, fill, c.button_border);
            if pressed { c.button_text_pressed } else { c.button_text }
        };
        SetTextColor(hdc, text_color);
        let mut rc = b.rc;
        let keys = if st.show_keys.get() { 0 } else { DT_HIDEPREFIX };
        DrawTextW(hdc, b.caption.as_ptr(), b.caption.len() as i32, &mut rc, DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_END_ELLIPSIS | keys);
    }

    if st.own_border.get() {
        SetDCBrushColor(hdc, c.border);
        FrameRect(hdc, &RECT { left: 0, top: 0, right: w, bottom: h }, GetStockObject(DC_BRUSH) as HBRUSH);
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
    }
    let Some(st) = state(hwnd) else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };
    let point = || ((lparam & 0xFFFF) as i16 as i32, ((lparam >> 16) & 0xFFFF) as i16 as i32);
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mem = CreateCompatibleDC(hdc);
            let bmp = CreateCompatibleBitmap(hdc, st.w, st.h);
            let old = SelectObject(mem, bmp);
            paint(st, mem);
            BitBlt(hdc, 0, 0, st.w, st.h, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old);
            DeleteObject(bmp);
            DeleteDC(mem);
            EndPaint(hwnd, &ps);
            0
        }
        WM_ERASEBKGND => 1,
        WM_CTLCOLOREDIT => {
            let hdc = wparam as HDC;
            let fill = if st.focus.get() == Focus::Input { st.colors.input_focus } else { st.colors.input_rest };
            SetTextColor(hdc, st.colors.text);
            SetBkColor(hdc, fill);
            SetDCBrushColor(hdc, fill);
            GetStockObject(DC_BRUSH) as LRESULT
        }
        WM_ACTIVATE => {
            if wparam & 0xFFFF != WA_INACTIVE as usize {
                match st.focus.get() {
                    Focus::Input => SetFocus(st.edit.get()),
                    Focus::Button(_) => SetFocus(hwnd),
                };
            }
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = point();
            let hit = st.button_at(x, y);
            if st.hover.replace(hit) != hit {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            if !st.tracking.replace(true) {
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                TrackMouseEvent(&mut tme);
            }
            0
        }
        WM_MOUSELEAVE => {
            st.tracking.set(false);
            if st.hover.replace(None).is_some() {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = point();
            if let Some(i) = st.button_at(x, y) {
                st.pressed.set(Some(i));
                st.hover.set(Some(i));
                SetCapture(hwnd);
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_LBUTTONUP => {
            if let Some(i) = st.pressed.take() {
                ReleaseCapture();
                let (x, y) = point();
                if st.button_at(x, y) == Some(i) {
                    st.request.set(Some(i));
                    wake(hwnd);
                }
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_CAPTURECHANGED => {
            if st.pressed.take().is_some() {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as i32;
            let code = ((wparam >> 16) & 0xFFFF) as u32;
            if lparam != 0 {
                if code == EN_SETFOCUS && st.focus.get() != Focus::Input {
                    set_focus(st, hwnd, Focus::Input);
                }
            } else if let Some(i) = st.buttons.iter().position(|b| b.id == id) {
                st.request.set(Some(i));
                wake(hwnd);
            } else if id == IDCANCEL {
                st.result.set(Some(st.cancel));
                wake(hwnd);
            }
            0
        }
        WM_CLOSE => {
            st.result.set(Some(st.cancel));
            wake(hwnd);
            0
        }
        // Destroyed from outside (its owner went away): end the modal loop.
        WM_DESTROY => {
            if st.result.get().is_none() {
                st.result.set(Some(st.cancel));
                wake(hwnd);
            }
            0
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_button_takes_the_right_half() {
        assert_eq!(button_columns(1, 272, 8), vec![(140, 132)]);
    }

    #[test]
    fn buttons_split_the_row_and_end_flush() {
        assert_eq!(button_columns(2, 272, 8), vec![(0, 132), (140, 132)]);
        assert_eq!(button_columns(3, 272, 8), vec![(0, 85), (93, 85), (186, 86)]);
    }

    #[test]
    fn access_keys_and_escape() {
        assert_eq!(hotkey("Do&n't save"), Some('n'));
        assert_eq!(hotkey("&Save"), Some('s'));
        assert_eq!(hotkey("Cancel"), None);
        assert_eq!(cancel_id(&[("&Save", IDYES), ("Do&n't save", IDNO), ("Cancel", IDCANCEL)]), IDCANCEL);
        assert_eq!(cancel_id(&[("OK", IDOK)]), IDOK);
    }
}
