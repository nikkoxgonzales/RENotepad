//! Windows 11 style window chrome, painted by hand in the client area. The tab
//! strip doubles as the title bar (app icon, tabs, new-tab button, caption
//! buttons) and a flat menu bar sits under it. This file owns geometry and
//! pixels; app.rs owns what clicking does.

use crate::theme::{self, Palette};
use crate::{dialogs, muldiv, wide};
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, LoadImageW, DI_NORMAL, HICON, IMAGE_ICON, LR_DEFAULTCOLOR};

/// File, Edit, View, then the settings gear at the far right.
pub const MENU_COUNT: usize = 4;
pub const SETTINGS_MENU: usize = 3;
const MENU_LABELS: [&str; 3] = ["&File", "&Edit", "&View"];

// Geometry at 96 DPI, measured from Windows 11 Notepad.
const STRIP_H: i32 = 42;
const TAB_TOP: i32 = 11;
const TAB_LEFT: i32 = 48;
const TAB_MAX_W: i32 = 240;
const TAB_MIN_W: i32 = 100;
const TAB_RADIUS: i32 = 8;
const TAB_PAD: i32 = 10;
const ICON: i32 = 16;
const ICON_X: i32 = 16;
const NEW_TAB_W: i32 = 40;
/// Empty strip kept free for dragging the window, however many tabs are open.
const DRAG_MIN: i32 = 46;
const BUTTON_W: i32 = 46;
const BUTTON_H: i32 = 30;
const MENU_H: i32 = 32;
const MENU_X: i32 = 4;
const MENU_PAD: i32 = 11;
const MENU_GAP: i32 = 8;
const MENU_INSET: i32 = 4;

// Segoe Fluent Icons / Segoe MDL2 Assets code points.
const GLYPH_MIN: u16 = 0xE921;
const GLYPH_MAX: u16 = 0xE922;
const GLYPH_RESTORE: u16 = 0xE923;
const GLYPH_CLOSE: u16 = 0xE8BB;
const GLYPH_ADD: u16 = 0xE710;
const GLYPH_SETTINGS: u16 = 0xE713;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    Nothing,
    Caption,
    Tab(usize),
    TabClose(usize),
    NewTab,
    Menu(usize),
    Min,
    Max,
    Close,
}

impl Hit {
    pub fn is_caption_button(self) -> bool {
        matches!(self, Hit::Min | Hit::Max | Hit::Close)
    }
}

pub struct TabView {
    pub title: String,
    pub modified: bool,
}

pub struct Layout {
    /// (document index, rect) of the tabs that fit.
    pub tabs: Vec<(usize, RECT)>,
    pub new_tab: RECT,
    pub buttons: [RECT; 3],
    pub menus: [RECT; MENU_COUNT],
}

pub struct Chrome {
    dpi: u32,
    menu_font: HFONT,
    tab_font: HFONT,
    glyph_10: HFONT,
    glyph_12: HFONT,
    glyph_16: HFONT,
    icon: HICON,
    menu_w: [i32; 3],
    pub hover: Hit,
    pub pressed: Hit,
    pub open_menu: Option<usize>,
    /// Underline menu mnemonics (menu opened from the keyboard).
    pub show_accel: bool,
    first_tab: usize,
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT { left, top, right, bottom }
}

fn contains(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

impl Chrome {
    pub fn new(dpi: u32) -> Chrome {
        let null = std::ptr::null_mut();
        let mut c = Chrome {
            dpi,
            menu_font: null,
            tab_font: null,
            glyph_10: null,
            glyph_12: null,
            glyph_16: null,
            icon: null,
            menu_w: [0; 3],
            hover: Hit::Nothing,
            pressed: Hit::Nothing,
            open_menu: None,
            show_accel: false,
            first_tab: 0,
        };
        c.set_dpi(dpi);
        c
    }

    fn s(&self, v: i32) -> i32 {
        muldiv(v, self.dpi as i32, 96)
    }

    pub fn set_dpi(&mut self, dpi: u32) {
        self.dpi = dpi;
        unsafe {
            for f in [self.menu_font, self.tab_font, self.glyph_10, self.glyph_12, self.glyph_16] {
                if !f.is_null() {
                    DeleteObject(f);
                }
            }
            if !self.icon.is_null() {
                DestroyIcon(self.icon);
            }
            self.menu_font = theme::font(theme::ui_face(), 14, dpi);
            self.tab_font = theme::font(theme::ui_face(), 12, dpi);
            self.glyph_10 = theme::font(theme::glyph_face(), 10, dpi);
            self.glyph_12 = theme::font(theme::glyph_face(), 12, dpi);
            self.glyph_16 = theme::font(theme::glyph_face(), 16, dpi);
            let px = self.s(ICON);
            self.icon = LoadImageW(dialogs::hinstance(), 1 as _, IMAGE_ICON, px, px, LR_DEFAULTCOLOR) as HICON;

            let hdc = GetDC(std::ptr::null_mut());
            let old = SelectObject(hdc, self.menu_font);
            for (w, label) in self.menu_w.iter_mut().zip(MENU_LABELS) {
                let t = wide(label);
                let mut r = rect(0, 0, 0, 0);
                DrawTextW(hdc, t.as_ptr(), -1, &mut r, DT_CALCRECT | DT_SINGLELINE);
                *w = r.right;
            }
            SelectObject(hdc, old);
            ReleaseDC(std::ptr::null_mut(), hdc);
        }
    }

    /// Strip + menu bar + the seam under it: where the editor starts.
    pub fn top_height(&self) -> i32 {
        self.s(STRIP_H) + self.s(MENU_H) + 1
    }

    pub fn layout(&mut self, width: i32, count: usize, active: usize) -> Layout {
        let dpi = self.dpi as i32;
        let s = move |v| muldiv(v, dpi, 96);
        let buttons_left = width - 3 * s(BUTTON_W);
        let room = (buttons_left - s(DRAG_MIN) - s(NEW_TAB_W) - s(TAB_LEFT)).max(s(TAB_MIN_W));
        let w = (room / count.max(1) as i32).clamp(s(TAB_MIN_W), s(TAB_MAX_W));
        let fit = ((room / w).max(1) as usize).min(count);
        // Scroll just enough to keep the active tab in view.
        if active < self.first_tab {
            self.first_tab = active;
        } else if active >= self.first_tab + fit {
            self.first_tab = active + 1 - fit;
        }
        self.first_tab = self.first_tab.min(count - fit);
        let tabs: Vec<(usize, RECT)> = (0..fit)
            .map(|k| {
                let x = s(TAB_LEFT) + k as i32 * w;
                (self.first_tab + k, rect(x, s(TAB_TOP), x + w, s(STRIP_H)))
            })
            .collect();
        let end = s(TAB_LEFT) + fit as i32 * w;
        let new_tab = rect(end + s(4), s(TAB_TOP) + s(1), end + s(36), s(STRIP_H) - s(2));
        let buttons = [0, 1, 2].map(|k| rect(buttons_left + k * s(BUTTON_W), 0, buttons_left + (k + 1) * s(BUTTON_W), s(BUTTON_H)));
        let (y0, y1) = (s(STRIP_H) + s(MENU_INSET), s(STRIP_H) + s(MENU_H) - s(MENU_INSET));
        let mut menus = [rect(0, 0, 0, 0); MENU_COUNT];
        let mut x = s(MENU_X);
        for (m, text_w) in menus.iter_mut().zip(self.menu_w) {
            let w = text_w + 2 * s(MENU_PAD);
            *m = rect(x, y0, x + w, y1);
            x += w + s(MENU_GAP);
        }
        menus[SETTINGS_MENU] = rect(width - s(44), y0, width - s(8), y1);
        Layout { tabs, new_tab, buttons, menus }
    }

    fn tab_close_rect(&self, tab: &RECT) -> RECT {
        let cx = tab.right - self.s(22);
        let cy = (tab.top + tab.bottom) / 2;
        let h = self.s(12);
        rect(cx - h, cy - h, cx + h, cy + h)
    }

    pub fn hit(&self, l: &Layout, x: i32, y: i32) -> Hit {
        for (r, hit) in l.buttons.iter().zip([Hit::Min, Hit::Max, Hit::Close]) {
            if contains(r, x, y) {
                return hit;
            }
        }
        if y < self.s(STRIP_H) {
            for &(i, r) in &l.tabs {
                if contains(&r, x, y) {
                    return if contains(&self.tab_close_rect(&r), x, y) { Hit::TabClose(i) } else { Hit::Tab(i) };
                }
            }
            if contains(&l.new_tab, x, y) {
                return Hit::NewTab;
            }
            return Hit::Caption;
        }
        for (i, r) in l.menus.iter().enumerate() {
            if contains(r, x, y) {
                return Hit::Menu(i);
            }
        }
        Hit::Nothing
    }

    /// Paint the whole chrome band (strip + menu bar + seam), double-buffered.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(&self, hdc: HDC, l: &Layout, width: i32, tabs: &[TabView], active: usize, window_active: bool, maximized: bool) {
        let p = theme::palette();
        let h = self.top_height();
        unsafe {
            let mem = CreateCompatibleDC(hdc);
            let bmp = CreateCompatibleBitmap(hdc, width, h);
            let old_bmp = SelectObject(mem, bmp);
            SetBkMode(mem, TRANSPARENT as i32);

            let strip_h = self.s(STRIP_H);
            theme::fill(mem, &rect(0, 0, width, strip_h), p.strip);
            theme::fill(mem, &rect(0, strip_h, width, h - 1), p.surface);
            theme::fill(mem, &rect(0, h - 1, width, h), p.line);

            let ic = self.s(ICON);
            let iy = (self.s(TAB_TOP) + strip_h - ic) / 2;
            DrawIconEx(mem, self.s(ICON_X), iy, self.icon, ic, ic, 0, std::ptr::null_mut(), DI_NORMAL);

            self.paint_tabs(mem, l, tabs, active, p);

            if self.hover == Hit::NewTab {
                theme::round_rect(mem, l.new_tab, self.s(4), false, p.tab_hover, p.strip);
            }
            glyph(mem, self.glyph_12, GLYPH_ADD, &l.new_tab, p.text);

            let glyphs = [GLYPH_MIN, if maximized { GLYPH_RESTORE } else { GLYPH_MAX }, GLYPH_CLOSE];
            for (k, (r, hit)) in l.buttons.iter().zip([Hit::Min, Hit::Max, Hit::Close]).enumerate() {
                let close = k == 2;
                let mut fg = if window_active { p.text } else { p.text_inactive };
                if self.pressed == hit {
                    theme::fill(mem, r, if close { p.close_pressed } else { p.caption_pressed });
                    fg = if close { p.close_text } else { p.text };
                } else if self.hover == hit {
                    theme::fill(mem, r, if close { p.close_hover } else { p.caption_hover });
                    fg = if close { p.close_text } else { p.text };
                }
                glyph(mem, self.glyph_10, glyphs[k], r, fg);
            }

            for (i, r) in l.menus.iter().enumerate() {
                if self.open_menu == Some(i) {
                    theme::round_rect(mem, *r, self.s(4), false, p.pressed, p.surface);
                } else if self.hover == Hit::Menu(i) {
                    theme::round_rect(mem, *r, self.s(4), false, p.hover, p.surface);
                }
                if i == SETTINGS_MENU {
                    glyph(mem, self.glyph_16, GLYPH_SETTINGS, r, p.text);
                } else {
                    let flags = DT_CENTER | DT_VCENTER | DT_SINGLELINE | if self.show_accel { 0 } else { DT_HIDEPREFIX };
                    text(mem, self.menu_font, MENU_LABELS[i], r, p.text, flags);
                }
            }

            BitBlt(hdc, 0, 0, width, h, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old_bmp);
            DeleteObject(bmp);
            DeleteDC(mem);
        }
    }

    fn paint_tabs(&self, hdc: HDC, l: &Layout, tabs: &[TabView], active: usize, p: &Palette) {
        let hovered_tab = match self.hover {
            Hit::Tab(i) | Hit::TabClose(i) => Some(i),
            _ => None,
        };
        for (k, &(i, r)) in l.tabs.iter().enumerate() {
            let is_active = i == active;
            let hovered = hovered_tab == Some(i);
            let bg = if is_active {
                theme::round_rect(hdc, r, self.s(TAB_RADIUS), true, p.surface, p.strip);
                p.surface
            } else if hovered {
                let hr = rect(r.left + self.s(2), r.top + self.s(2), r.right - self.s(2), r.bottom - self.s(4));
                theme::round_rect(hdc, hr, self.s(6), false, p.tab_hover, p.strip);
                p.tab_hover
            } else {
                // Thin divider between two resting tabs.
                let next = l.tabs.get(k + 1).map(|t| t.0);
                if next.is_some_and(|n| n != active && Some(n) != hovered_tab) {
                    let y = (r.top + r.bottom) / 2;
                    theme::fill(hdc, &rect(r.right - 1, y - self.s(8), r.right, y + self.s(8)), p.separator);
                }
                p.strip
            };
            let Some(tab) = tabs.get(i) else { continue };
            let close = self.tab_close_rect(&r);
            let title = rect(r.left + self.s(TAB_PAD), r.top, close.left - self.s(2), r.bottom);
            let fg = if is_active { p.text } else { p.text_secondary };
            text(hdc, self.tab_font, &tab.title, &title, fg, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
            if tab.modified && !hovered {
                let (cx, cy) = ((close.left + close.right) as f32 / 2.0, (close.top + close.bottom) as f32 / 2.0);
                theme::circle(hdc, cx, cy, 3.25 * self.dpi as f32 / 96.0, p.text_secondary, bg);
            } else if is_active || hovered {
                if self.hover == Hit::TabClose(i) {
                    let cr = rect(close.left + self.s(2), close.top + self.s(2), close.right - self.s(2), close.bottom - self.s(2));
                    theme::round_rect(hdc, cr, self.s(4), false, p.hover, bg);
                }
                glyph(hdc, self.glyph_10, GLYPH_CLOSE, &close, fg);
            }
        }
    }
}

fn text(hdc: HDC, font: HFONT, s: &str, r: &RECT, color: u32, flags: u32) {
    let t: Vec<u16> = s.encode_utf16().collect();
    if t.is_empty() {
        return; // an empty Vec's pointer is dangling; DrawText may still touch it
    }
    unsafe {
        let old = SelectObject(hdc, font);
        SetTextColor(hdc, color);
        let mut rc = *r;
        DrawTextW(hdc, t.as_ptr(), t.len() as i32, &mut rc, flags);
        SelectObject(hdc, old);
    }
}

fn glyph(hdc: HDC, font: HFONT, code: u16, r: &RECT, color: u32) {
    unsafe {
        let old = SelectObject(hdc, font);
        SetTextColor(hdc, color);
        let mut rc = *r;
        DrawTextW(hdc, &code, 1, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        SelectObject(hdc, old);
    }
}
