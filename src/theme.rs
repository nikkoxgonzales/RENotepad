//! Dark and light mode for a plain Win32 app. The window chrome (tab strip,
//! menu bar, status bar) is painted by hand from `palette()`. The rest is the
//! usual set of pieces: uxtheme's (undocumented, ordinal-only) app-mode calls
//! for popup menus and scrollbars, and WM_CTLCOLOR* for the edit and dialogs.

use crate::{muldiv, wide};
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    CreateFontIndirectW, CreateSolidBrush, EnumFontFamiliesExW, FillRect, GetDC, GetStockObject, ReleaseDC, SetBkColor,
    SetDCBrushColor, SetPixelV, SetTextColor, CLEARTYPE_QUALITY, DC_BRUSH, DEFAULT_CHARSET, HBRUSH, HDC, HFONT, LOGFONTW,
    TEXTMETRICW,
};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
use windows_sys::Win32::UI::Controls::SetWindowTheme;
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetClassNameW, GetWindowLongW, GWL_STYLE};

pub const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

// Dialog colors (dark mode only; light dialogs are stock Windows).
pub const EDIT_BG: COLORREF = rgb(39, 39, 39);
pub const EDIT_FG: COLORREF = rgb(230, 230, 230);
pub const CHROME_BG: COLORREF = rgb(32, 32, 32);
pub const CHROME_FG: COLORREF = rgb(225, 225, 225);
pub const CONTROL_BG: COLORREF = rgb(45, 45, 45);

/// Window chrome colors, sampled from Windows 11 Notepad.
pub struct Palette {
    /// Tab strip / title bar.
    pub strip: COLORREF,
    /// Active tab, menu bar and status bar.
    pub surface: COLORREF,
    /// Seams between the bars and the editor.
    pub line: COLORREF,
    /// Status bar separators and the lines between inactive tabs.
    pub separator: COLORREF,
    pub text: COLORREF,
    pub text_secondary: COLORREF,
    pub text_inactive: COLORREF,
    /// Menu-bar item and tab-close hover.
    pub hover: COLORREF,
    pub pressed: COLORREF,
    pub tab_hover: COLORREF,
    pub caption_hover: COLORREF,
    pub caption_pressed: COLORREF,
    pub close_hover: COLORREF,
    pub close_pressed: COLORREF,
    pub close_text: COLORREF,
}

const DARK_PALETTE: Palette = Palette {
    strip: rgb(12, 12, 12),
    surface: rgb(31, 31, 31),
    line: rgb(28, 28, 28),
    separator: rgb(50, 50, 50),
    text: rgb(242, 242, 242),
    text_secondary: rgb(206, 206, 206),
    text_inactive: rgb(120, 120, 120),
    hover: rgb(45, 45, 45),
    pressed: rgb(40, 40, 40),
    tab_hover: rgb(30, 30, 30),
    caption_hover: rgb(40, 40, 40),
    caption_pressed: rgb(34, 34, 34),
    close_hover: rgb(196, 43, 28),
    close_pressed: rgb(200, 60, 49),
    close_text: rgb(255, 255, 255),
};

const LIGHT_PALETTE: Palette = Palette {
    strip: rgb(238, 238, 238),
    surface: rgb(249, 249, 249),
    line: rgb(229, 229, 229),
    separator: rgb(210, 210, 210),
    text: rgb(26, 26, 26),
    text_secondary: rgb(80, 80, 80),
    text_inactive: rgb(150, 150, 150),
    hover: rgb(234, 234, 234),
    pressed: rgb(237, 237, 237),
    tab_hover: rgb(230, 230, 230),
    caption_hover: rgb(226, 226, 226),
    caption_pressed: rgb(231, 231, 231),
    close_hover: rgb(196, 43, 28),
    close_pressed: rgb(200, 60, 49),
    close_text: rgb(255, 255, 255),
};

static DARK: AtomicBool = AtomicBool::new(true);

pub fn is_dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

pub fn palette() -> &'static Palette {
    if is_dark() {
        &DARK_PALETTE
    } else {
        &LIGHT_PALETTE
    }
}

struct Brushes {
    edit: HBRUSH,
    chrome: HBRUSH,
    control: HBRUSH,
}

fn brushes() -> &'static Brushes {
    static B: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let p = *B.get_or_init(|| unsafe {
        Box::into_raw(Box::new(Brushes {
            edit: CreateSolidBrush(EDIT_BG),
            chrome: CreateSolidBrush(CHROME_BG),
            control: CreateSolidBrush(CONTROL_BG),
        })) as usize
    });
    unsafe { &*(p as *const Brushes) }
}

// ---------------------------------------------------------------- drawing

pub fn fill(hdc: HDC, rc: &RECT, color: COLORREF) {
    unsafe {
        SetDCBrushColor(hdc, color);
        FillRect(hdc, rc, GetStockObject(DC_BRUSH) as HBRUSH);
    }
}

pub fn mix(bg: COLORREF, fg: COLORREF, t: f32) -> COLORREF {
    let ch = |shift: u32| {
        let (b, f) = ((bg >> shift) & 0xFF, (fg >> shift) & 0xFF);
        ((b as f32 + (f as f32 - b as f32) * t).round() as u32) << shift
    };
    ch(0) | ch(8) | ch(16)
}

/// One pixel of a disc edge, antialiased with 4x4 supersampling against a solid `bg`.
#[allow(clippy::too_many_arguments)]
unsafe fn disc_pixel(hdc: HDC, px: i32, py: i32, cx: f32, cy: f32, r: f32, fg: COLORREF, bg: COLORREF) {
    let mut n = 0;
    for sy in 0..4 {
        for sx in 0..4 {
            let dx = px as f32 + (sx as f32 + 0.5) / 4.0 - cx;
            let dy = py as f32 + (sy as f32 + 0.5) / 4.0 - cy;
            if dx * dx + dy * dy <= r * r {
                n += 1;
            }
        }
    }
    SetPixelV(hdc, px, py, mix(bg, fg, n as f32 / 16.0));
}

/// Fill a rectangle with rounded corners (all four, or the top two only).
/// GDI has no antialiasing, so the corners are blended by hand; that needs the
/// solid color behind them.
pub fn round_rect(hdc: HDC, rc: RECT, radius: i32, top_only: bool, fg: COLORREF, bg: COLORREF) {
    fill(hdc, &rc, fg);
    let r = radius.min((rc.right - rc.left) / 2).min((rc.bottom - rc.top) / 2);
    if r <= 0 {
        return;
    }
    let mut corners = vec![(rc.left, rc.top), (rc.right - r, rc.top)];
    if !top_only {
        corners.extend([(rc.left, rc.bottom - r), (rc.right - r, rc.bottom - r)]);
    }
    for (x0, y0) in corners {
        // Circle center: the corner square's inner vertex.
        let cx = if x0 == rc.left { rc.left + r } else { rc.right - r } as f32;
        let cy = if y0 == rc.top { rc.top + r } else { rc.bottom - r } as f32;
        for py in y0..y0 + r {
            for px in x0..x0 + r {
                unsafe { disc_pixel(hdc, px, py, cx, cy, r as f32, fg, bg) };
            }
        }
    }
}

pub fn circle(hdc: HDC, cx: f32, cy: f32, r: f32, fg: COLORREF, bg: COLORREF) {
    for py in (cy - r).floor() as i32..(cy + r).ceil() as i32 {
        for px in (cx - r).floor() as i32..(cx + r).ceil() as i32 {
            unsafe { disc_pixel(hdc, px, py, cx, cy, r, fg, bg) };
        }
    }
}

// ---------------------------------------------------------------- fonts

fn font_exists(face: &str) -> bool {
    unsafe extern "system" fn found(_: *const LOGFONTW, _: *const TEXTMETRICW, _: u32, l: LPARAM) -> i32 {
        *(l as *mut bool) = true;
        0
    }
    let mut lf: LOGFONTW = unsafe { std::mem::zeroed() };
    lf.lfCharSet = DEFAULT_CHARSET;
    for (d, s) in lf.lfFaceName.iter_mut().zip(face.encode_utf16().take(31)) {
        *d = s;
    }
    let mut hit = false;
    unsafe {
        let hdc = GetDC(std::ptr::null_mut());
        EnumFontFamiliesExW(hdc, &lf, Some(found), &mut hit as *mut bool as LPARAM, 0);
        ReleaseDC(std::ptr::null_mut(), hdc);
    }
    hit
}

/// Windows 11's UI font when present, else Windows 10's.
pub fn ui_face() -> &'static str {
    static F: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    F.get_or_init(|| if font_exists("Segoe UI Variable Text") { "Segoe UI Variable Text" } else { "Segoe UI" })
}

/// Icon font for caption buttons and the like: Windows 11's, else Windows 10's.
pub fn glyph_face() -> &'static str {
    static F: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    F.get_or_init(|| if font_exists("Segoe Fluent Icons") { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" })
}

/// A font `px` pixels tall at 96 DPI, scaled to `dpi`.
pub fn font(face: &str, px: i32, dpi: u32) -> HFONT {
    font_weight(face, px, dpi, 400)
}

pub fn font_weight(face: &str, px: i32, dpi: u32, weight: i32) -> HFONT {
    let mut lf: LOGFONTW = unsafe { std::mem::zeroed() };
    lf.lfHeight = -muldiv(px, dpi as i32, 96);
    lf.lfWeight = weight;
    lf.lfCharSet = DEFAULT_CHARSET;
    lf.lfQuality = CLEARTYPE_QUALITY;
    for (d, s) in lf.lfFaceName.iter_mut().zip(face.encode_utf16().take(31)) {
        *d = s;
    }
    unsafe { CreateFontIndirectW(&lf) }
}

// ---------------------------------------------------------------- app mode

// uxtheme ordinals (stable since Windows 10 1903).
type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32; // #135
type AllowDarkModeForWindow = unsafe extern "system" fn(HWND, i32) -> i32; // #133
type FlushMenuThemes = unsafe extern "system" fn(); // #136
type RefreshImmersiveColorPolicyState = unsafe extern "system" fn(); // #104

struct UxTheme {
    set_app_mode: Option<SetPreferredAppMode>,
    allow_window: Option<AllowDarkModeForWindow>,
    flush_menus: Option<FlushMenuThemes>,
    refresh_policy: Option<RefreshImmersiveColorPolicyState>,
}

fn uxtheme() -> &'static UxTheme {
    static U: std::sync::OnceLock<UxTheme> = std::sync::OnceLock::new();
    U.get_or_init(|| unsafe {
        let name = wide("uxtheme.dll");
        let lib = LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        let ord = |n: usize| if lib.is_null() { None } else { GetProcAddress(lib, n as *const u8) };
        UxTheme {
            set_app_mode: ord(135).map(|f| std::mem::transmute(f)),
            allow_window: ord(133).map(|f| std::mem::transmute(f)),
            flush_menus: ord(136).map(|f| std::mem::transmute(f)),
            refresh_policy: ord(104).map(|f| std::mem::transmute(f)),
        }
    })
}

/// Process-wide mode: popup menus, context menus and scrollbars follow this.
pub fn set_app_dark(dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
    let u = uxtheme();
    unsafe {
        if let Some(f) = u.set_app_mode {
            f(if dark { 2 } else { 3 }); // ForceDark / ForceLight
        }
        if let Some(f) = u.refresh_policy {
            f();
        }
        if let Some(f) = u.flush_menus {
            f();
        }
    }
}

/// Title bar (dialogs) / window border (main window) + per-window opt-in.
pub fn apply_window(hwnd: HWND) {
    let dark = is_dark();
    unsafe {
        if let Some(f) = uxtheme().allow_window {
            f(hwnd, dark as i32);
        }
        let v: i32 = dark as i32;
        DwmSetWindowAttribute(hwnd, 20, (&v as *const i32).cast(), 4); // DWMWA_USE_IMMERSIVE_DARK_MODE
    }
}

/// Dark scrollbars etc. on a child control.
pub fn apply_control(hwnd: HWND, dark_theme: &str) {
    unsafe {
        if let Some(f) = uxtheme().allow_window {
            f(hwnd, is_dark() as i32);
        }
        let t = wide(if is_dark() { dark_theme } else { "Explorer" });
        SetWindowTheme(hwnd, t.as_ptr(), std::ptr::null());
    }
}

/// WM_CTLCOLOREDIT answer for the editor in dark mode; None in light mode (use DefWindowProc).
pub fn edit_color(hdc: HDC) -> Option<isize> {
    if !is_dark() {
        return None;
    }
    unsafe {
        SetTextColor(hdc, EDIT_FG);
        SetBkColor(hdc, EDIT_BG);
    }
    Some(brushes().edit as isize)
}

/// Dialog controls: edits/combos get the control background, everything else chrome.
pub fn dialog_ctlcolor(msg: u32, hdc: HDC) -> Option<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX};
    if !is_dark() {
        return None;
    }
    unsafe {
        SetTextColor(hdc, CHROME_FG);
        if msg == WM_CTLCOLOREDIT || msg == WM_CTLCOLORLISTBOX {
            SetBkColor(hdc, CONTROL_BG);
            Some(brushes().control as isize)
        } else {
            SetBkColor(hdc, CHROME_BG);
            Some(brushes().chrome as isize)
        }
    }
}

/// Theme every child of a dialog by its window class.
pub fn apply_dialog(hwnd: HWND) {
    apply_window(hwnd);
    unsafe extern "system" fn each(child: HWND, _: LPARAM) -> i32 {
        let mut cls = [0u16; 64];
        let n = GetClassNameW(child, cls.as_mut_ptr(), 64) as usize;
        let cls = String::from_utf16_lossy(&cls[..n]).to_ascii_lowercase();
        match cls.as_str() {
            "button" => apply_control(child, "DarkMode_Explorer"),
            "edit" | "combobox" | "comboboxex32" => apply_control(child, "DarkMode_CFD"),
            "listbox" | "scrollbar" | "syslistview32" => apply_control(child, "DarkMode_Explorer"),
            _ => {}
        }
        1
    }
    unsafe { EnumChildWindows(hwnd, Some(each), 0) };
}

/// Is this button a checkbox/radio? Their themed text ignores WM_CTLCOLOR, so our
/// own dialogs draw them caption-less with a separate label.
pub fn is_check_or_radio(hwnd: HWND) -> bool {
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32 & 0xF;
    matches!(style, 2 | 3 | 4 | 5 | 6 | 9)
}

/// For dialog procs: handle the color messages in one place.
pub fn dialog_color_msg(msg: u32, wparam: WPARAM) -> Option<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WM_CTLCOLORBTN, WM_CTLCOLORDLG, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC,
    };
    match msg {
        WM_CTLCOLORDLG | WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            dialog_ctlcolor(msg, wparam as HDC)
        }
        _ => None,
    }
}
