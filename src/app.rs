//! Main window: tabs, menus, edit controls, commands. Behavior follows
//! docs/WIN10-NOTEPAD-SPEC.md; the look (custom title bar with tabs, flat menu
//! bar, status bar) follows Windows 11 Notepad and is drawn by chrome.rs.

use crate::chrome::{Chrome, Hit, Layout, TabView, MENU_COUNT, SETTINGS_MENU};
use crate::dialogs::{self, Template, BUTTON, EDIT, LABEL_OFFSET};
use crate::encoding::{self, Encoding};
use crate::settings::Settings;
use crate::status::{self, Status};
use crate::{filedlg, muldiv, print, text, theme, update, wide};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::DataExchange::IsClipboardFormatAvailable;
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::Controls::Dialogs::{ChooseFontW, CF_ENABLEHOOK, CF_INITTOLOGFONTSTRUCT, CF_NOVERTFONTS, CF_SCREENFONTS, CHOOSEFONTW};
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetCapture, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, VK_DOWN,
    VK_LEFT, VK_RIGHT,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, DragAcceptFiles, DragFinish, DragQueryFileW, SHAddToRecentDocs, SetWindowSubclass, HDROP, SHARD_PATHW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const APP_NAME: &str = "RENotepad";

// Menu commands.
const IDM_NEW: u16 = 1;
const IDM_NEW_WINDOW: u16 = 2;
const IDM_OPEN: u16 = 3;
const IDM_SAVE: u16 = 4;
const IDM_SAVE_AS: u16 = 5;
const IDM_PAGE_SETUP: u16 = 6;
const IDM_PRINT: u16 = 7;
const IDM_EXIT: u16 = 8;
const IDM_SAVE_ALL: u16 = 9;
const IDM_CLOSE_TAB: u16 = 10;
const IDM_CLOSE_WINDOW: u16 = 11;
const IDM_NEXT_TAB: u16 = 12;
const IDM_PREV_TAB: u16 = 13;
const IDM_UNDO: u16 = 16;
const IDM_CUT: u16 = 17;
const IDM_COPY: u16 = 18;
const IDM_PASTE: u16 = 19;
const IDM_DELETE: u16 = 20;
const IDM_FIND: u16 = 21;
const IDM_FIND_NEXT: u16 = 22;
const IDM_FIND_PREV: u16 = 23;
const IDM_REPLACE: u16 = 24;
const IDM_GOTO: u16 = 25;
const IDM_SELECT_ALL: u16 = 26;
const IDM_TIME_DATE: u16 = 27;
const IDM_WRAP: u16 = 32;
const IDM_FONT: u16 = 33;
const IDM_ZOOM_IN: u16 = 40;
const IDM_ZOOM_OUT: u16 = 41;
const IDM_ZOOM_RESET: u16 = 42;
const IDM_STATUS_BAR: u16 = 43;
const IDM_DARK: u16 = 44;
const IDM_ABOUT: u16 = 48;
const IDM_CHECK_UPDATES: u16 = 49;
const IDM_AUTO_UPDATES: u16 = 50;

const EDIT_ID: usize = 15;
/// Editor text inset at 96 DPI (Windows 11 Notepad: 16 px left and top).
const EDIT_PAD: i32 = 16;

// Edit control extensions (Windows 10 1809+, commctrl.h ECM_FIRST = 0x1500).
const EM_SETEXTENDEDSTYLE: u32 = 0x150A;
const EM_SETENDOFLINE: u32 = 0x150C;
const EM_GETENDOFLINE: u32 = 0x150D;
const EM_GETCARETINDEX: u32 = 0x1512;
const EM_FILELINEFROMCHAR: u32 = 0x1513;
const EM_FILELINEINDEX: u32 = 0x1514;
const EM_GETFILELINECOUNT: u32 = 0x1517;
const EM_GETZOOM: u32 = 0x04E0;
const EM_SETZOOM: u32 = 0x04E1;
const ES_EX_ALLOWEOL_ALL: usize = 0x3;
const ES_EX_CONVERT_EOL_ON_PASTE: usize = 0x4;
const ES_EX_ZOOMABLE: usize = 0x10;
const EC_ENDOFLINE_DETECTFROMCONTENT: usize = 0;
const EC_ENDOFLINE_CRLF: usize = 1;

const MAX_FILE: u64 = 1 << 30;

// Find / Replace dialog control ids.
const IDC_FIND_TEXT: u16 = 1152;
const IDC_REPLACE_TEXT: u16 = 1153;
const IDC_MATCH_CASE: u16 = 1041;
const IDC_WRAP_AROUND: u16 = 1042;
const IDC_UP: u16 = 1056;
const IDC_DOWN: u16 = 1057;
const IDC_REPLACE_ONE: u16 = 1024;
const IDC_REPLACE_ALL: u16 = 1025;

/// One tab: its own edit control (so undo stays per tab), file and encoding.
struct Doc {
    edit: HWND,
    path: Option<String>,
    encoding: Encoding,
    zoom: i32,
}

/// Menu-bar keyboard/mouse navigation while a popup is open. Written by the
/// message hook and WM_MENUSELECT *during* TrackPopupMenuEx, so it lives
/// outside App: `track_menu` holds `&mut self` across that call, and the
/// optimizer may assume nothing else changes `self` meanwhile (it did, in release).
#[derive(Default, Clone, Copy)]
struct MenuNav {
    current: usize,
    next: Option<usize>,
    from_keyboard: bool,
    /// The highlighted item opens a submenu (Right should open it, not switch).
    on_popup_item: bool,
    /// Focus is inside a submenu (Left should close it, not switch).
    in_submenu: bool,
}

thread_local! {
    static NAV: std::cell::Cell<MenuNav> = std::cell::Cell::new(MenuNav::default());
}

fn nav() -> MenuNav {
    NAV.with(|n| n.get())
}

fn update_nav(f: impl FnOnce(&mut MenuNav)) {
    NAV.with(|n| {
        let mut v = n.get();
        f(&mut v);
        n.set(v);
    });
}

pub struct App {
    pub hwnd: HWND,
    docs: Vec<Doc>,
    cur: usize,
    pub status: Status,
    pub find_dlg: HWND,
    find_is_replace: bool,
    font: HFONT,
    dpi: u32,
    pub settings: Settings,
    chrome: Chrome,
    menus: [HMENU; MENU_COUNT],
    window_active: bool,
    tracking_mouse: bool,
    /// Scrollbars the active editor needs (vertical, horizontal); the rest are
    /// pushed under the window edge / status bar, as Windows 11 shows none at rest.
    bars: (bool, bool),
    shown_title: String,
    hook: HWINEVENTHOOK,
    pub accel: HACCEL,
    pub global_accel: HACCEL,
}

static mut APP: *mut App = std::ptr::null_mut();

/// The single UI-thread app state (one main window per process).
pub fn app() -> &'static mut App {
    unsafe { &mut *APP }
}

pub fn try_app() -> Option<&'static mut App> {
    unsafe { APP.as_mut() }
}

pub fn create(cmd_show: i32, args: &text::Args) -> Option<()> {
    unsafe {
        let settings = Settings::load();
        theme::set_app_dark(settings.dark);
        let hinst = dialogs::hinstance();
        let class = wide(APP_NAME);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinst,
            hIcon: LoadIconW(hinst, 1 as _),
            hIconSm: LoadImageW(hinst, 1 as _, IMAGE_ICON, 16, 16, LR_DEFAULTCOLOR) as HICON,
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);

        let a = Box::new(App {
            hwnd: std::ptr::null_mut(),
            docs: Vec::new(),
            cur: 0,
            status: Status::empty(),
            find_dlg: std::ptr::null_mut(),
            find_is_replace: false,
            font: std::ptr::null_mut(),
            dpi: 96,
            settings,
            chrome: Chrome::new(96),
            menus: build_menus(),
            window_active: true,
            tracking_mouse: false,
            bars: (true, true),
            shown_title: String::new(),
            hook: std::ptr::null_mut(),
            accel: accelerators(false),
            global_accel: accelerators(true),
        });
        APP = Box::into_raw(a);
        let a = app();

        let title = wide(APP_NAME);
        let hwnd = CreateWindowExW(
            0, class.as_ptr(), title.as_ptr(), WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
            std::ptr::null_mut(), std::ptr::null_mut(), hinst, std::ptr::null(),
        );
        if hwnd.is_null() {
            return None;
        }
        a.hwnd = hwnd;
        a.dpi = GetDpiForWindow(hwnd).max(96);
        a.chrome.set_dpi(a.dpi);
        theme::apply_window(hwnd);
        if let Some((x, y, dx, dy)) = a.settings.window {
            let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
            wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
            wp.rcNormalPosition = RECT { left: x, top: y, right: x + dx, bottom: y + dy };
            wp.showCmd = SW_HIDE as u32;
            SetWindowPlacement(hwnd, &wp);
        }
        // Re-run WM_NCCALCSIZE now that the title bar is ours.
        SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        DragAcceptFiles(hwnd, 1);
        a.status = Status::create(hwnd, a.dpi);
        a.make_font();
        a.new_tab();
        a.hook = SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE, std::ptr::null_mut(), Some(caret_event), GetCurrentProcessId(), GetCurrentThreadId(), WINEVENT_OUTOFCONTEXT);

        if args.print {
            // /P: print and leave without ever showing the window.
            if let Some(f) = &args.file {
                if a.load(f, None) {
                    a.print(false);
                }
            }
            DestroyWindow(hwnd);
            return Some(());
        }
        let show = if a.settings.maximized && a.settings.window.is_some() { SW_SHOWMAXIMIZED } else { cmd_show };
        ShowWindow(hwnd, show);
        UpdateWindow(hwnd);
        if let Some(f) = &args.file {
            let forced = if args.force_ansi {
                Some(Encoding::Ansi)
            } else if args.force_unicode {
                Some(Encoding::Utf16Le)
            } else {
                None
            };
            a.open_from_command_line(f, forced);
        }
        if a.settings.check_updates {
            update::check_async(hwnd, false);
        }
        SetFocus(a.edit());
        Some(())
    }
}

fn accelerators(global: bool) -> HACCEL {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    let c = FVIRTKEY | FCONTROL;
    let cs = c | FSHIFT;
    let mut list: Vec<(u8, u16, u16)> = vec![
        (FVIRTKEY, VK_F3, IDM_FIND_NEXT),
        (FVIRTKEY | FSHIFT, VK_F3, IDM_FIND_PREV),
    ];
    if !global {
        list.extend([
            (c, b'N' as u16, IDM_NEW),
            (c, b'T' as u16, IDM_NEW),
            (cs, b'N' as u16, IDM_NEW_WINDOW),
            (c, b'O' as u16, IDM_OPEN),
            (c, b'S' as u16, IDM_SAVE),
            (cs, b'S' as u16, IDM_SAVE_AS),
            (c, b'P' as u16, IDM_PRINT),
            (c, b'W' as u16, IDM_CLOSE_TAB),
            (c, VK_F4, IDM_CLOSE_TAB),
            (cs, b'W' as u16, IDM_CLOSE_WINDOW),
            (c, VK_TAB, IDM_NEXT_TAB),
            (cs, VK_TAB, IDM_PREV_TAB),
            (c, VK_NEXT, IDM_NEXT_TAB),
            (c, VK_PRIOR, IDM_PREV_TAB),
            (c, b'F' as u16, IDM_FIND),
            (c, b'H' as u16, IDM_REPLACE),
            (c, b'G' as u16, IDM_GOTO),
            (c, b'A' as u16, IDM_SELECT_ALL),
            (FVIRTKEY, VK_F5, IDM_TIME_DATE),
            (c, VK_OEM_PLUS, IDM_ZOOM_IN),
            (c, VK_ADD, IDM_ZOOM_IN),
            (c, VK_OEM_MINUS, IDM_ZOOM_OUT),
            (c, VK_SUBTRACT, IDM_ZOOM_OUT),
            (c, b'0' as u16, IDM_ZOOM_RESET),
            (c, VK_NUMPAD0, IDM_ZOOM_RESET),
        ]);
    }
    let table: Vec<ACCEL> = list.iter().map(|&(f, k, cmd)| ACCEL { fVirt: f, key: k, cmd }).collect();
    unsafe { CreateAcceleratorTableW(table.as_ptr(), table.len() as i32) }
}

/// The popups behind File, Edit, View and the settings gear (sentence case, as in Windows 11).
fn build_menus() -> [HMENU; MENU_COUNT] {
    unsafe {
        let add = |m: HMENU, id: u16, s: &str| {
            let w = wide(s);
            AppendMenuW(m, MF_STRING, id as usize, w.as_ptr());
        };
        let sep = |m: HMENU| {
            AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
        };

        let file = CreatePopupMenu();
        add(file, IDM_NEW, "&New tab\tCtrl+N");
        add(file, IDM_NEW_WINDOW, "New &window\tCtrl+Shift+N");
        add(file, IDM_OPEN, "&Open...\tCtrl+O");
        add(file, IDM_SAVE, "&Save\tCtrl+S");
        add(file, IDM_SAVE_AS, "Save &as...\tCtrl+Shift+S");
        add(file, IDM_SAVE_ALL, "Save a&ll");
        sep(file);
        add(file, IDM_PAGE_SETUP, "Page set&up...");
        add(file, IDM_PRINT, "&Print...\tCtrl+P");
        sep(file);
        add(file, IDM_CLOSE_TAB, "&Close tab\tCtrl+W");
        add(file, IDM_CLOSE_WINDOW, "Close win&dow\tCtrl+Shift+W");
        add(file, IDM_EXIT, "E&xit");

        let edit = CreatePopupMenu();
        add(edit, IDM_UNDO, "&Undo\tCtrl+Z");
        sep(edit);
        add(edit, IDM_CUT, "Cu&t\tCtrl+X");
        add(edit, IDM_COPY, "&Copy\tCtrl+C");
        add(edit, IDM_PASTE, "&Paste\tCtrl+V");
        add(edit, IDM_DELETE, "De&lete\tDel");
        sep(edit);
        add(edit, IDM_FIND, "&Find...\tCtrl+F");
        add(edit, IDM_FIND_NEXT, "Find &next\tF3");
        add(edit, IDM_FIND_PREV, "Find pre&vious\tShift+F3");
        add(edit, IDM_REPLACE, "&Replace...\tCtrl+H");
        add(edit, IDM_GOTO, "&Go to...\tCtrl+G");
        sep(edit);
        add(edit, IDM_SELECT_ALL, "Select &all\tCtrl+A");
        add(edit, IDM_TIME_DATE, "Time/&date\tF5");
        sep(edit);
        add(edit, IDM_FONT, "F&ont...");

        let view = CreatePopupMenu();
        let zoom = CreatePopupMenu();
        add(zoom, IDM_ZOOM_IN, "Zoom &in\tCtrl+Plus");
        add(zoom, IDM_ZOOM_OUT, "Zoom &out\tCtrl+Minus");
        add(zoom, IDM_ZOOM_RESET, "&Restore default zoom\tCtrl+0");
        let w = wide("&Zoom");
        AppendMenuW(view, MF_POPUP, zoom as usize, w.as_ptr());
        add(view, IDM_STATUS_BAR, "&Status bar");
        add(view, IDM_WRAP, "&Word wrap");

        let settings = CreatePopupMenu();
        add(settings, IDM_DARK, "&Dark mode");
        sep(settings);
        add(settings, IDM_CHECK_UPDATES, "Check for &updates");
        add(settings, IDM_AUTO_UPDATES, "Check for updates at &startup");
        sep(settings);
        add(settings, IDM_ABOUT, &format!("&About {APP_NAME}"));
        [file, edit, view, settings]
    }
}

impl App {
    fn s(&self, v: i32) -> i32 {
        muldiv(v, self.dpi as i32, 96)
    }

    fn doc(&self) -> &Doc {
        &self.docs[self.cur]
    }

    fn doc_mut(&mut self) -> &mut Doc {
        &mut self.docs[self.cur]
    }

    /// The active tab's edit control.
    pub fn edit(&self) -> HWND {
        self.docs.get(self.cur).map_or(std::ptr::null_mut(), |d| d.edit)
    }

    fn make_font(&mut self) {
        unsafe {
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
            let mut lf: LOGFONTW = std::mem::zeroed();
            lf.lfHeight = -muldiv(self.settings.point_size, self.dpi as i32, 720);
            lf.lfWeight = self.settings.weight;
            lf.lfItalic = self.settings.italic as u8;
            lf.lfCharSet = self.settings.charset;
            for (d, s) in lf.lfFaceName.iter_mut().zip(self.settings.face.encode_utf16().take(31)) {
                *d = s;
            }
            self.font = CreateFontIndirectW(&lf);
            for d in &self.docs {
                SendMessageW(d.edit, WM_SETFONT, self.font as usize, 1);
            }
        }
        self.layout();
        self.fit_scrollbars();
    }

    /// A new edit control (hidden). Word wrap is decided by WS_HSCROLL, so
    /// toggling it means making new controls and carrying the text across.
    fn make_edit(&self, text: &[u16], sel: (usize, usize), modified: bool, zoom: i32) -> HWND {
        unsafe {
            let mut style = WS_CHILD | WS_CLIPSIBLINGS | WS_VSCROLL | ES_MULTILINE as u32 | ES_NOHIDESEL as u32;
            if !self.settings.wrap {
                style |= WS_HSCROLL;
            }
            let class = wide("Edit");
            let edit = CreateWindowExW(0, class.as_ptr(), std::ptr::null(), style, 0, 0, 0, 0, self.hwnd, EDIT_ID as HMENU, dialogs::hinstance(), std::ptr::null());
            SendMessageW(edit, EM_SETEXTENDEDSTYLE, ES_EX_ALLOWEOL_ALL | ES_EX_CONVERT_EOL_ON_PASTE | ES_EX_ZOOMABLE, (ES_EX_ALLOWEOL_ALL | ES_EX_CONVERT_EOL_ON_PASTE | ES_EX_ZOOMABLE) as isize);
            SendMessageW(edit, EM_SETLIMITTEXT, 0, 0);
            SendMessageW(edit, WM_SETFONT, self.font as usize, 0);
            SendMessageW(edit, EM_SETMARGINS, (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize, 0);
            SendMessageW(edit, EM_SETENDOFLINE, if text.is_empty() { EC_ENDOFLINE_CRLF } else { EC_ENDOFLINE_DETECTFROMCONTENT }, 0);
            SetWindowSubclass(edit, Some(edit_subclass), 1, 0);
            theme::apply_control(edit, "DarkMode_Explorer");
            if !text.is_empty() {
                set_edit_text(edit, text);
            }
            SendMessageW(edit, EM_SETMODIFY, modified as usize, 0);
            SendMessageW(edit, EM_SETSEL, sel.0, sel.1 as isize);
            if zoom != 100 {
                SendMessageW(edit, EM_SETZOOM, zoom as usize, 100);
            }
            // The status bar must stay above editors that reach under it.
            SetWindowPos(self.status.hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            edit
        }
    }

    fn client_width(&self) -> i32 {
        let mut rc: RECT = unsafe { std::mem::zeroed() };
        unsafe { GetClientRect(self.hwnd, &mut rc) };
        rc.right
    }

    fn layout(&self) {
        unsafe {
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(self.hwnd, &mut rc);
            let top = self.chrome.top_height();
            let sh = if self.settings.status_bar { self.status.height() } else { 0 };
            let edit = self.edit();
            if !edit.is_null() {
                // An unneeded scrollbar is pushed past the window edge / under the status bar.
                let (v, h) = self.bars;
                let extra_w = if v { 0 } else { GetSystemMetricsForDpi(SM_CXVSCROLL, self.dpi) };
                let extra_h = if h || self.settings.wrap { 0 } else { GetSystemMetricsForDpi(SM_CYHSCROLL, self.dpi) };
                MoveWindow(edit, 0, top, rc.right + extra_w, (rc.bottom - sh - top).max(0) + extra_h, 1);
                self.pad_edit(edit);
            }
            MoveWindow(self.status.hwnd, 0, rc.bottom - sh, rc.right, sh, 1);
            ShowWindow(self.status.hwnd, if self.settings.status_bar { SW_SHOWNA } else { SW_HIDE });
        }
    }

    /// Does the active editor need its (vertical, horizontal) scrollbar? The
    /// control keeps the ranges current because the bars always exist.
    fn scrollbar_need(&self) -> (bool, bool) {
        let need = |bar| unsafe {
            let mut si: SCROLLINFO = std::mem::zeroed();
            si.cbSize = std::mem::size_of::<SCROLLINFO>() as u32;
            si.fMask = SIF_ALL;
            GetScrollInfo(self.edit(), bar, &mut si) != 0 && si.nPage > 0 && (si.nMax - si.nMin + 1 > si.nPage as i32 || si.nPos > si.nMin)
        };
        (need(SB_VERT), !self.settings.wrap && need(SB_HORZ))
    }

    /// Show only the scrollbars there is something to scroll for. Showing one
    /// narrows the text, which can only add lines, so this settles in a pass or two.
    fn fit_scrollbars(&mut self) {
        for _ in 0..3 {
            let need = self.scrollbar_need();
            if need == self.bars {
                return;
            }
            self.bars = need;
            self.layout();
            // Resizing re-insets the text without re-scrolling; keep the caret in view.
            unsafe { SendMessageW(self.edit(), EM_SCROLLCARET, 0, 0) };
        }
    }

    /// Inset the text like Windows 11 Notepad. The control resets this rect on
    /// every resize, font change and zoom, so it is re-applied after each.
    fn pad_edit(&self, edit: HWND) {
        unsafe {
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(edit, &mut rc);
            let pad = self.s(EDIT_PAD);
            rc.left += pad;
            rc.top += pad;
            rc.right = (rc.right - pad).max(rc.left + 1);
            SendMessageW(edit, EM_SETRECTNP, 0, &rc as *const RECT as isize);
        }
    }

    fn invalidate_chrome(&self) {
        unsafe {
            let rc = RECT { left: 0, top: 0, right: self.client_width(), bottom: self.chrome.top_height() };
            InvalidateRect(self.hwnd, &rc, 0);
        }
    }

    fn chrome_layout(&mut self) -> Layout {
        let w = self.client_width();
        self.chrome.layout(w, self.docs.len(), self.cur)
    }

    fn hit(&mut self, x: i32, y: i32) -> Hit {
        let l = self.chrome_layout();
        self.chrome.hit(&l, x, y)
    }

    fn set_hover(&mut self, h: Hit) {
        if self.chrome.hover != h {
            self.chrome.hover = h;
            self.invalidate_chrome();
        }
    }

    fn paint_chrome(&mut self, hdc: HDC) {
        let tabs: Vec<TabView> = (0..self.docs.len()).map(|i| TabView { title: self.tab_title(i), modified: self.doc_modified(i) }).collect();
        let l = self.chrome_layout();
        let maximized = unsafe { IsZoomed(self.hwnd) } != 0;
        self.chrome.paint(hdc, &l, self.client_width(), &tabs, self.cur, self.window_active, maximized);
    }

    fn doc_modified(&self, i: usize) -> bool {
        unsafe { SendMessageW(self.docs[i].edit, EM_GETMODIFY, 0, 0) != 0 }
    }

    fn modified(&self) -> bool {
        self.doc_modified(self.cur)
    }

    fn tab_title(&self, i: usize) -> String {
        match &self.docs[i].path {
            Some(p) => std::path::Path::new(p).file_name().map_or(p.clone(), |n| n.to_string_lossy().into_owned()),
            None => "Untitled".into(),
        }
    }

    /// Window title (taskbar, Alt+Tab) and the tab strip, when anything shown changed.
    fn update_title(&mut self) {
        let m = self.modified();
        let t = format!("{}{} - {APP_NAME}", if m { "*" } else { "" }, self.tab_title(self.cur));
        if t != self.shown_title {
            dialogs::set_text(self.hwnd, &t);
            self.shown_title = t;
            self.invalidate_chrome();
        }
    }

    fn caret(&self) -> (usize, usize) {
        unsafe {
            let e = self.edit();
            let idx = SendMessageW(e, EM_GETCARETINDEX, 0, 0) as usize;
            let line = SendMessageW(e, EM_FILELINEFROMCHAR, idx, 0) as usize;
            let start = SendMessageW(e, EM_FILELINEINDEX, line, 0) as usize;
            (line + 1, idx.saturating_sub(start) + 1)
        }
    }

    /// (selected, total) characters, a CRLF line break counting as one.
    fn char_counts(&self) -> (usize, usize) {
        unsafe {
            let e = self.edit();
            let len = GetWindowTextLengthW(e).max(0) as usize;
            let crlf = SendMessageW(e, EM_GETENDOFLINE, 0, 0) as usize == EC_ENDOFLINE_CRLF;
            let line_of = |i: usize| SendMessageW(e, EM_FILELINEFROMCHAR, i, 0).max(0) as usize;
            let (s, end) = self.sel();
            if !crlf {
                return (end - s, len);
            }
            let lines = SendMessageW(e, EM_GETFILELINECOUNT, 0, 0).max(1) as usize;
            ((end - s).saturating_sub(line_of(end) - line_of(s)), len.saturating_sub(lines - 1))
        }
    }

    pub fn update_status(&mut self) {
        if !self.settings.status_bar || self.docs.is_empty() {
            return;
        }
        let (ln, col) = self.caret();
        let (sel, total) = self.char_counts();
        let noun = if total == 1 { "character" } else { "characters" };
        let chars = if sel > 0 { format!("{} of {} {noun}", group(sel), group(total)) } else { format!("{} {noun}", group(total)) };
        let eol = match unsafe { SendMessageW(self.edit(), EM_GETENDOFLINE, 0, 0) } {
            2 => "Macintosh (CR)",
            3 => "Unix (LF)",
            _ => "Windows (CRLF)",
        };
        self.status.set(status::POS, &format!("Ln {ln}, Col {col}"));
        self.status.set(status::CHARS, &chars);
        self.status.set(status::ZOOM, &format!("{}%", self.doc().zoom));
        self.status.set(status::EOL, eol);
        self.status.set(status::ENCODING, self.doc().encoding.label());
    }

    fn set_message(&mut self, msg: &str) {
        self.status.set(status::MESSAGE, msg);
        self.status.link = false;
    }

    /// A finished update check. The startup check only speaks up, in the status
    /// bar, when there is something new; a check from the menu always answers.
    fn update_result(&mut self, outcome: update::Outcome, manual: bool) {
        use update::Outcome::*;
        let current = update::current();
        if manual {
            self.set_message("");
        }
        match (outcome, manual) {
            (Available { version, .. }, false) => {
                // Clicking it runs the check from the menu, which offers the download.
                self.set_message(&format!("Update available: {version}"));
                self.status.link = true;
            }
            (Available { version, url }, true) => {
                let text = format!("{APP_NAME} {version} is available. You have {current}.\n\nThe download page has the installer and the portable version.");
                if dialogs::message_box(self.hwnd, "Update available", &text, &[("&Download", IDOK), ("Later", IDCANCEL)]) == IDOK {
                    open_url(self.hwnd, &url);
                }
            }
            (UpToDate, true) => {
                let text = format!("You're up to date. {APP_NAME} {current} is the latest version.");
                dialogs::message_box(self.hwnd, "Check for updates", &text, &[("OK", IDOK)]);
            }
            (Failed(e), true) => {
                let text = format!("Couldn't check for updates.\n\n{e}");
                dialogs::message_box(self.hwnd, "Check for updates", &text, &[("OK", IDOK)]);
            }
            _ => {}
        }
    }

    fn sel(&self) -> (usize, usize) {
        let (mut s, mut e) = (0u32, 0u32);
        unsafe { SendMessageW(self.edit(), EM_GETSEL, &mut s as *mut u32 as usize, &mut e as *mut u32 as isize) };
        (s as usize, e as usize)
    }

    fn replace_sel(&self, text: &[u16]) {
        let mut t = text.to_vec();
        t.push(0);
        unsafe { SendMessageW(self.edit(), EM_REPLACESEL, 1, t.as_ptr() as isize) };
    }

    // ------------------------------------------------------------ tabs

    fn new_tab(&mut self) {
        let edit = self.make_edit(&[], (0, 0), false, 100);
        self.docs.push(Doc { edit, path: None, encoding: Encoding::Utf8, zoom: 100 });
        self.switch_to(self.docs.len() - 1);
    }

    fn switch_to(&mut self, i: usize) {
        if i >= self.docs.len() {
            return;
        }
        self.cur = i;
        unsafe {
            for (k, d) in self.docs.iter().enumerate() {
                if k != i {
                    ShowWindow(d.edit, SW_HIDE);
                }
            }
            self.layout();
            ShowWindow(self.edit(), SW_SHOW);
            SendMessageW(self.edit(), EM_SCROLLCARET, 0, 0);
            SetFocus(self.edit());
        }
        self.fit_scrollbars();
        self.update_title();
        self.invalidate_chrome();
        self.set_message("");
        self.update_status();
    }

    /// Untouched Untitled tab: Open and drag-drop reuse it instead of adding a tab.
    fn is_pristine(&self, i: usize) -> bool {
        let d = &self.docs[i];
        d.path.is_none() && !self.doc_modified(i) && unsafe { GetWindowTextLengthW(d.edit) } == 0
    }

    /// false = the user cancelled the save prompt.
    fn close_tab(&mut self, i: usize) -> bool {
        if i >= self.docs.len() {
            return true;
        }
        if self.doc_modified(i) {
            self.switch_to(i);
            if !self.check_save() {
                return false;
            }
        }
        if self.docs.len() == 1 {
            // Closing the last tab closes the window, as in Windows 11.
            self.quit();
            return true;
        }
        let d = self.docs.remove(i);
        unsafe { DestroyWindow(d.edit) };
        let next = if self.cur > i { self.cur - 1 } else { self.cur.min(self.docs.len() - 1) };
        self.switch_to(next);
        true
    }

    /// Ask about every modified tab; false = cancelled.
    fn confirm_close_all(&mut self) -> bool {
        for i in 0..self.docs.len() {
            if self.doc_modified(i) {
                self.switch_to(i);
                if !self.check_save() {
                    return false;
                }
            }
        }
        true
    }

    fn quit(&mut self) {
        self.save_placement();
        self.settings.save();
        unsafe { DestroyWindow(self.hwnd) };
    }

    fn cycle_tab(&mut self, forward: bool) {
        let n = self.docs.len();
        self.switch_to(if forward { (self.cur + 1) % n } else { (self.cur + n - 1) % n });
    }

    // ------------------------------------------------------------ files

    fn open_from_command_line(&mut self, arg: &str, forced: Option<Encoding>) {
        let mut path = std::path::absolute(arg).unwrap_or_else(|_| arg.into());
        if !path.exists() && path.extension().is_none() {
            path.set_extension("txt");
        }
        let p = path.to_string_lossy().into_owned();
        if path.exists() {
            self.load(&p, forced);
            return;
        }
        let msg = format!("Cannot find the {p} file.\n\nDo you want to create a new file?");
        match dialogs::message_box(self.hwnd, APP_NAME, &msg, &[("&Yes", IDYES), ("&No", IDNO), ("Cancel", IDCANCEL)]) {
            IDYES => match std::fs::File::create(&path) {
                Ok(_) => {
                    self.load(&p, forced);
                }
                Err(e) => self.error(&format!("{p}\n{e}")),
            },
            IDCANCEL => unsafe {
                PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
            },
            _ => {}
        }
    }

    /// Open a file in its own tab: switch to it if already open, reuse an untouched Untitled tab.
    fn open_path(&mut self, path: &str, forced: Option<Encoding>) {
        let same = |d: &Doc| d.path.as_deref().is_some_and(|p| p.eq_ignore_ascii_case(path));
        if let Some(i) = self.docs.iter().position(same) {
            self.switch_to(i);
            if forced.is_some() && !self.modified() {
                self.load(path, forced);
            }
            return;
        }
        let reuse = self.is_pristine(self.cur);
        if !reuse {
            self.new_tab();
        }
        if !self.load(path, forced) && !reuse {
            self.close_tab(self.cur);
        }
    }

    /// Load a file into the active tab.
    fn load(&mut self, path: &str, forced: Option<Encoding>) -> bool {
        match std::fs::metadata(path) {
            Ok(m) if m.len() >= MAX_FILE => {
                self.error(&format!("The file {path} is too large for {APP_NAME}."));
                return false;
            }
            Err(e) => {
                self.error(&format!("{path}\n{e}"));
                return false;
            }
            _ => {}
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.error(&format!("{path}\n{e}"));
                return false;
            }
        };
        let enc = forced.unwrap_or_else(|| encoding::detect(&bytes));
        let content = encoding::decode(&bytes, enc);
        let e = self.edit();
        unsafe {
            SendMessageW(e, WM_SETREDRAW, 0, 0);
            set_edit_text(e, &content);
            SendMessageW(e, EM_SETENDOFLINE, EC_ENDOFLINE_DETECTFROMCONTENT, 0);
            SendMessageW(e, EM_EMPTYUNDOBUFFER, 0, 0);
            SendMessageW(e, EM_SETMODIFY, 0, 0);
            SendMessageW(e, EM_SETSEL, 0, 0);
            SendMessageW(e, EM_SCROLLCARET, 0, 0);
            SendMessageW(e, WM_SETREDRAW, 1, 0);
            InvalidateRect(e, std::ptr::null(), 1);
        }
        let d = self.doc_mut();
        d.path = Some(path.to_string());
        d.encoding = enc;
        if text::is_log_file(&content) {
            let end = content.len();
            unsafe {
                SendMessageW(e, EM_SETSEL, end, end as isize);
                SendMessageW(e, EM_SCROLLCARET, 0, 0);
            }
            self.replace_sel(&text::timestamp(true));
        }
        let wpath = wide(path);
        unsafe { SHAddToRecentDocs(SHARD_PATHW as u32, wpath.as_ptr().cast()) };
        self.update_title();
        self.invalidate_chrome();
        self.set_message("");
        self.update_status();
        self.fit_scrollbars();
        true
    }

    fn write(&mut self, path: &str, enc: Encoding) -> bool {
        let content = get_edit_text(self.edit());
        let bytes = match encoding::encode(&content, enc, false) {
            Ok(b) => b,
            Err(()) => {
                let msg = "This file contains characters in Unicode format which will be lost if you save this file as an ANSI encoded text file. To keep the Unicode information, click Cancel below and then select one of the Unicode options from the Encoding drop down list. Continue?";
                if dialogs::message_box(self.hwnd, APP_NAME, msg, &[("OK", IDOK), ("Cancel", IDCANCEL)]) != IDOK {
                    return false;
                }
                encoding::encode(&content, enc, true).unwrap_or_default()
            }
        };
        if let Err(e) = std::fs::write(path, bytes) {
            self.error(&format!("{path}\n{e}"));
            return false;
        }
        let d = self.doc_mut();
        d.path = Some(path.to_string());
        d.encoding = enc;
        unsafe { SendMessageW(self.edit(), EM_SETMODIFY, 0, 0) };
        self.update_title();
        self.invalidate_chrome();
        self.update_status();
        true
    }

    fn save(&mut self) -> bool {
        match self.doc().path.clone() {
            Some(p) => self.write(&p, self.doc().encoding),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) -> bool {
        match filedlg::save_as(self.hwnd, self.doc().path.as_deref(), self.doc().encoding) {
            Some(picked) => self.write(&picked.path, picked.encoding.unwrap_or(self.doc().encoding)),
            None => false,
        }
    }

    fn save_all(&mut self) {
        let back = self.cur;
        for i in 0..self.docs.len() {
            if self.doc_modified(i) {
                self.switch_to(i);
                if !self.save() {
                    return; // cancelled: stay on the tab that needs attention
                }
            }
        }
        self.switch_to(back);
    }

    /// true = go ahead (saved or discarded), false = user cancelled.
    fn check_save(&mut self) -> bool {
        if !self.modified() {
            return true;
        }
        let name = self.doc().path.clone().unwrap_or_else(|| "Untitled".into());
        let msg = format!("Do you want to save changes to {name}?");
        match dialogs::message_box(self.hwnd, APP_NAME, &msg, &[("&Save", IDYES), ("Do&n't save", IDNO), ("Cancel", IDCANCEL)]) {
            IDYES => self.save(),
            IDNO => true,
            _ => false,
        }
    }

    fn open(&mut self) {
        if let Some(picked) = filedlg::open(self.hwnd, self.doc().path.as_deref()) {
            self.open_path(&picked.path, picked.encoding);
        }
    }

    fn error(&self, msg: &str) {
        dialogs::message_box(self.hwnd, APP_NAME, msg, &[("OK", IDOK)]);
    }

    fn print(&mut self, ask: bool) {
        let content = get_edit_text(self.edit());
        let title = self.tab_title(self.cur);
        let s = &self.settings;
        let job = print::Job { text: &content, title: &title, face: &s.face, point_size: s.point_size, weight: s.weight, italic: s.italic, margins: s.margins };
        if let Err(e) = print::print(self.hwnd, &job, ask) {
            self.error(&e);
        }
    }

    // ------------------------------------------------------------ search

    fn search_text(&self) -> Vec<u16> {
        self.settings.search.encode_utf16().collect()
    }

    fn not_found(&self) {
        let owner = if self.find_dlg.is_null() { self.hwnd } else { self.find_dlg };
        dialogs::message_box(owner, APP_NAME, &format!("Cannot find \"{}\"", self.settings.search), &[("OK", IDOK)]);
    }

    /// F3 / Shift+F3 / Find Next button.
    fn find_next(&mut self, down: bool) -> bool {
        if self.settings.search.is_empty() {
            self.open_find(false);
            return false;
        }
        let content = get_edit_text(self.edit());
        let mut sel = self.sel();
        if sel == (0, content.len()) {
            sel = (0, 0); // everything selected: start from the top
        }
        let pat = self.search_text();
        match text::find(&content, &pat, sel, down, self.settings.match_case, self.settings.wrap_around) {
            Some(hit) => {
                unsafe {
                    SendMessageW(self.edit(), EM_SETSEL, hit.start, (hit.start + hit.len) as isize);
                    SendMessageW(self.edit(), EM_SCROLLCARET, 0, 0);
                }
                let msg = match (hit.wrapped, down) {
                    (true, true) => "Found next from the top",
                    (true, false) => "Found next from the bottom",
                    _ => "",
                };
                self.set_message(msg);
                true
            }
            None => {
                self.not_found();
                false
            }
        }
    }

    fn replace_one(&mut self) {
        let (s, e) = self.sel();
        let content = get_edit_text(self.edit());
        let selected = &content[s.min(content.len())..e.min(content.len())];
        if text::selection_matches(selected, &self.search_text(), self.settings.match_case) {
            let with: Vec<u16> = self.settings.replace.encode_utf16().collect();
            self.replace_sel(&with);
        }
        self.find_next(true);
    }

    fn replace_all(&mut self) {
        let content = get_edit_text(self.edit());
        let with: Vec<u16> = self.settings.replace.encode_utf16().collect();
        let (out, n) = text::replace_all(&content, &self.search_text(), &with, self.settings.match_case);
        if n == 0 {
            self.not_found();
            return;
        }
        let e = self.edit();
        unsafe {
            SendMessageW(e, WM_SETREDRAW, 0, 0);
            SendMessageW(e, EM_SETSEL, 0, -1);
            self.replace_sel(&out);
            SendMessageW(e, EM_SETSEL, 0, 0);
            SendMessageW(e, EM_SCROLLCARET, 0, 0);
            SendMessageW(e, WM_SETREDRAW, 1, 0);
            InvalidateRect(e, std::ptr::null(), 1);
        }
    }

    fn open_find(&mut self, replace: bool) {
        unsafe {
            // Seed with a short single-line selection, like Notepad.
            let (s, e) = self.sel();
            if e > s && e - s < 128 {
                let content = get_edit_text(self.edit());
                let picked = &content[s..e.min(content.len())];
                if !picked.contains(&10) && !picked.contains(&13) {
                    self.settings.search = String::from_utf16_lossy(picked);
                }
            }
            if !self.find_dlg.is_null() {
                if self.find_is_replace == replace {
                    let edit = GetDlgItem(self.find_dlg, IDC_FIND_TEXT as i32);
                    dialogs::set_text(edit, &self.settings.search);
                    SetFocus(edit);
                    SendMessageW(edit, EM_SETSEL, 0, -1);
                    return;
                }
                DestroyWindow(self.find_dlg);
            }
            self.find_is_replace = replace;
            let tpl = find_template(replace).build();
            self.find_dlg = CreateDialogIndirectParamW(dialogs::hinstance(), tpl.as_ptr().cast(), self.hwnd, Some(find_proc), 0);
            ShowWindow(self.find_dlg, SW_SHOW);
        }
    }

    fn goto(&mut self) {
        let (line, _) = self.caret();
        let e = self.edit();
        let max = unsafe { SendMessageW(e, EM_GETFILELINECOUNT, 0, 0) }.max(1) as u32;
        if let Some(n) = dialogs::goto_line(self.hwnd, line as u32, max) {
            unsafe {
                let idx = SendMessageW(e, EM_FILELINEINDEX, (n - 1) as usize, 0) as usize;
                SendMessageW(e, EM_SETSEL, idx, idx as isize);
                SendMessageW(e, EM_SCROLLCARET, 0, 0);
            }
        }
    }

    // ------------------------------------------------------------ view

    fn set_zoom(&mut self, pct: i32) {
        let z = pct.clamp(10, 500);
        self.doc_mut().zoom = z;
        unsafe { SendMessageW(self.edit(), EM_SETZOOM, z as usize, 100) };
        self.pad_edit(self.edit());
        self.fit_scrollbars();
        self.update_status();
    }

    /// The control zooms itself on Ctrl+wheel; read back what it chose.
    fn sync_zoom(&mut self) {
        let (mut num, mut den) = (0i32, 0i32);
        unsafe { SendMessageW(self.edit(), EM_GETZOOM, &mut num as *mut i32 as usize, &mut den as *mut i32 as isize) };
        self.doc_mut().zoom = if num > 0 && den > 0 { (num * 100 + den / 2) / den } else { 100 };
        self.pad_edit(self.edit());
        self.fit_scrollbars();
        self.update_status();
    }

    fn toggle_wrap(&mut self) {
        self.settings.wrap = !self.settings.wrap;
        for i in 0..self.docs.len() {
            let old = self.docs[i].edit;
            let content = get_edit_text(old);
            let (mut s, mut e) = (0u32, 0u32);
            unsafe { SendMessageW(old, EM_GETSEL, &mut s as *mut u32 as usize, &mut e as *mut u32 as isize) };
            let edit = self.make_edit(&content, (s as usize, e as usize), self.doc_modified(i), self.docs[i].zoom);
            self.docs[i].edit = edit;
            unsafe { DestroyWindow(old) };
        }
        self.switch_to(self.cur);
    }

    fn choose_font(&mut self) {
        unsafe {
            let mut lf: LOGFONTW = std::mem::zeroed();
            lf.lfHeight = -muldiv(self.settings.point_size, 96, 720);
            lf.lfWeight = self.settings.weight;
            lf.lfItalic = self.settings.italic as u8;
            lf.lfCharSet = self.settings.charset;
            for (d, s) in lf.lfFaceName.iter_mut().zip(self.settings.face.encode_utf16().take(31)) {
                *d = s;
            }
            let mut cf: CHOOSEFONTW = std::mem::zeroed();
            cf.lStructSize = std::mem::size_of::<CHOOSEFONTW>() as u32;
            cf.hwndOwner = self.hwnd;
            cf.lpLogFont = &mut lf;
            cf.Flags = CF_INITTOLOGFONTSTRUCT | CF_SCREENFONTS | CF_NOVERTFONTS | CF_ENABLEHOOK;
            cf.lpfnHook = Some(font_hook);
            // The font dialog reasons in 96-DPI points; keep it system-aware like Notepad does.
            let prev = windows_sys::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(-2isize as _);
            let ok = ChooseFontW(&mut cf) != 0;
            windows_sys::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(prev);
            if !ok {
                return;
            }
            let len = lf.lfFaceName.iter().position(|&c| c == 0).unwrap_or(32);
            self.settings.face = String::from_utf16_lossy(&lf.lfFaceName[..len]);
            self.settings.point_size = cf.iPointSize.max(10);
            self.settings.weight = lf.lfWeight;
            self.settings.italic = lf.lfItalic != 0;
            self.settings.charset = lf.lfCharSet;
            self.make_font();
        }
    }

    fn toggle_dark(&mut self) {
        self.settings.dark = !self.settings.dark;
        theme::set_app_dark(self.settings.dark);
        theme::apply_window(self.hwnd);
        for d in &self.docs {
            theme::apply_control(d.edit, "DarkMode_Explorer");
        }
        unsafe {
            if !self.find_dlg.is_null() {
                theme::apply_dialog(self.find_dlg);
                RedrawWindow(self.find_dlg, std::ptr::null(), std::ptr::null_mut(), RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_FRAME | RDW_ERASE);
            }
            RedrawWindow(self.hwnd, std::ptr::null(), std::ptr::null_mut(), RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_FRAME | RDW_ERASE);
        }
    }

    fn save_placement(&mut self) {
        unsafe {
            let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
            wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
            if GetWindowPlacement(self.hwnd, &mut wp) != 0 {
                let r = wp.rcNormalPosition;
                self.settings.window = Some((r.left, r.top, r.right - r.left, r.bottom - r.top));
                self.settings.maximized = wp.showCmd == SW_SHOWMAXIMIZED as u32;
            }
        }
    }

    // ------------------------------------------------------------ menus

    /// Drop a menu-bar popup under its button. Left/Right and hovering another
    /// button move along the bar, like a real menu bar.
    fn track_menu(&mut self, first: usize, from_keyboard: bool) {
        let mut i = first;
        let mut keyboard = from_keyboard;
        let mut picked;
        loop {
            self.chrome.open_menu = Some(i);
            self.chrome.show_accel = keyboard;
            self.chrome.hover = Hit::Nothing;
            self.invalidate_chrome();
            unsafe { UpdateWindow(self.hwnd) };
            let l = self.chrome_layout();
            let mut r = l.menus[i];
            unsafe { MapWindowPoints(self.hwnd, std::ptr::null_mut(), (&mut r as *mut RECT).cast(), 2) };
            NAV.with(|n| n.set(MenuNav { current: i, ..Default::default() }));
            unsafe {
                let hook = SetWindowsHookExW(WH_MSGFILTER, Some(menu_filter), std::ptr::null_mut(), GetCurrentThreadId());
                if keyboard {
                    PostMessageW(self.hwnd, WM_KEYDOWN, VK_DOWN as usize, 0); // highlight the first item
                }
                let tpm = TPMPARAMS { cbSize: std::mem::size_of::<TPMPARAMS>() as u32, rcExclude: r };
                let (align, x) = if i == SETTINGS_MENU { (TPM_RIGHTALIGN, r.right) } else { (TPM_LEFTALIGN, r.left) };
                picked = TrackPopupMenuEx(self.menus[i], align | TPM_TOPALIGN | TPM_VERTICAL | TPM_RETURNCMD, x, r.bottom, self.hwnd, &tpm);
                UnhookWindowsHookEx(hook);
            }
            let state = nav();
            match state.next {
                Some(n) if picked == 0 => {
                    i = n;
                    keyboard = state.from_keyboard;
                }
                _ => break,
            }
        }
        self.chrome.open_menu = None;
        self.chrome.show_accel = false;
        self.invalidate_chrome();
        if picked != 0 {
            self.command(picked as u16);
        }
    }

    fn init_menu(&self, menu: HMENU) {
        unsafe {
            let e = self.edit();
            let (s, end) = self.sel();
            let has_sel = end > s;
            let has_text = GetWindowTextLengthW(e) > 0;
            let en = |id: u16, on: bool| {
                EnableMenuItem(menu, id as u32, MF_BYCOMMAND | if on { MF_ENABLED } else { MF_GRAYED });
            };
            let ck = |id: u16, on: bool| {
                CheckMenuItem(menu, id as u32, MF_BYCOMMAND | if on { MF_CHECKED } else { MF_UNCHECKED });
            };
            en(IDM_UNDO, SendMessageW(e, EM_CANUNDO, 0, 0) != 0);
            en(IDM_CUT, has_sel);
            en(IDM_COPY, has_sel);
            en(IDM_DELETE, has_sel);
            en(IDM_PASTE, IsClipboardFormatAvailable(13 /* CF_UNICODETEXT */) != 0);
            en(IDM_FIND, has_text);
            en(IDM_FIND_NEXT, has_text);
            en(IDM_FIND_PREV, has_text);
            en(IDM_REPLACE, has_text);
            ck(IDM_WRAP, self.settings.wrap);
            ck(IDM_STATUS_BAR, self.settings.status_bar);
            ck(IDM_DARK, self.settings.dark);
            ck(IDM_AUTO_UPDATES, self.settings.check_updates);
        }
    }

    fn command(&mut self, id: u16) {
        let e = self.edit();
        unsafe {
            match id {
                IDM_NEW => self.new_tab(),
                IDM_NEW_WINDOW => {
                    if let Ok(exe) = std::env::current_exe() {
                        let _ = std::process::Command::new(exe).spawn();
                    }
                }
                IDM_OPEN => self.open(),
                IDM_SAVE => {
                    self.save();
                }
                IDM_SAVE_AS => {
                    self.save_as();
                }
                IDM_SAVE_ALL => self.save_all(),
                IDM_PAGE_SETUP => {
                    if let Some(m) = print::page_setup(self.hwnd, self.settings.margins) {
                        self.settings.margins = m;
                    }
                }
                IDM_PRINT => self.print(true),
                IDM_CLOSE_TAB => {
                    self.close_tab(self.cur);
                }
                IDM_CLOSE_WINDOW | IDM_EXIT => {
                    PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
                }
                IDM_NEXT_TAB => self.cycle_tab(true),
                IDM_PREV_TAB => self.cycle_tab(false),
                IDM_UNDO => {
                    SendMessageW(e, WM_UNDO, 0, 0);
                }
                IDM_CUT => {
                    SendMessageW(e, WM_CUT, 0, 0);
                }
                IDM_COPY => {
                    SendMessageW(e, WM_COPY, 0, 0);
                }
                IDM_PASTE => {
                    SendMessageW(e, WM_PASTE, 0, 0);
                }
                IDM_DELETE => {
                    SendMessageW(e, WM_CLEAR, 0, 0);
                }
                IDM_FIND => self.open_find(false),
                IDM_REPLACE => self.open_find(true),
                IDM_FIND_NEXT => {
                    self.find_next(true);
                }
                IDM_FIND_PREV => {
                    self.find_next(false);
                }
                IDM_GOTO => self.goto(),
                IDM_SELECT_ALL => {
                    SendMessageW(e, EM_SETSEL, 0, -1);
                }
                IDM_TIME_DATE => self.replace_sel(&text::timestamp(false)),
                IDM_WRAP => self.toggle_wrap(),
                IDM_FONT => self.choose_font(),
                IDM_ZOOM_IN => self.set_zoom(self.doc().zoom + 10),
                IDM_ZOOM_OUT => self.set_zoom(self.doc().zoom - 10),
                IDM_ZOOM_RESET => self.set_zoom(100),
                IDM_STATUS_BAR => {
                    self.settings.status_bar = !self.settings.status_bar;
                    self.layout();
                    self.update_status();
                }
                IDM_DARK => self.toggle_dark(),
                IDM_ABOUT => dialogs::about(self.hwnd),
                IDM_CHECK_UPDATES | status::MESSAGE_CLICKED => {
                    self.set_message("Checking for updates…");
                    update::check_async(self.hwnd, true);
                }
                IDM_AUTO_UPDATES => self.settings.check_updates = !self.settings.check_updates,
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------ mouse on the chrome

    fn track_leave(&mut self, nonclient: bool) {
        let mut t = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE | if nonclient { TME_NONCLIENT } else { 0 },
            hwndTrack: self.hwnd,
            dwHoverTime: 0,
        };
        unsafe { TrackMouseEvent(&mut t) };
    }

    fn caption_action(&self, h: Hit) {
        let cmd = match h {
            Hit::Min => SC_MINIMIZE,
            Hit::Max if unsafe { IsZoomed(self.hwnd) } != 0 => SC_RESTORE,
            Hit::Max => SC_MAXIMIZE,
            Hit::Close => SC_CLOSE,
            _ => return,
        };
        unsafe { PostMessageW(self.hwnd, WM_SYSCOMMAND, cmd as usize, 0) };
    }
}

/// 1234567 -> "1,234,567".
fn group(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn get_edit_text(edit: HWND) -> Vec<u16> {
    unsafe {
        let n = GetWindowTextLengthW(edit);
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(edit, buf.as_mut_ptr(), n + 1);
        buf.truncate(got.max(0) as usize);
        buf
    }
}

fn set_edit_text(edit: HWND, text: &[u16]) {
    let mut t = text.to_vec();
    t.push(0);
    unsafe { SetWindowTextW(edit, t.as_ptr()) };
}

fn point(lparam: LPARAM) -> (i32, i32) {
    ((lparam & 0xFFFF) as i16 as i32, ((lparam >> 16) & 0xFFFF) as i16 as i32)
}

fn hit_from_ht(ht: u32) -> Hit {
    match ht {
        HTMINBUTTON => Hit::Min,
        HTMAXBUTTON => Hit::Max,
        HTCLOSE => Hit::Close,
        _ => Hit::Nothing,
    }
}

/// Resize border plus padding: how far a maximized window hangs off the screen.
unsafe fn frame_thickness(hwnd: HWND) -> i32 {
    let dpi = GetDpiForWindow(hwnd);
    GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
}

/// Drop the caption: the client area starts at the window's top edge, where
/// chrome.rs paints the tab strip. Side and bottom borders stay standard.
unsafe fn frame_calcsize(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let p = &mut *(lparam as *mut NCCALCSIZE_PARAMS);
    let top = p.rgrc[0].top;
    let r = DefWindowProcW(hwnd, WM_NCCALCSIZE, wparam, lparam);
    p.rgrc[0].top = top + if IsZoomed(hwnd) != 0 { frame_thickness(hwnd) } else { 0 };
    r
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCCALCSIZE && wparam != 0 {
        return frame_calcsize(hwnd, wparam, lparam);
    }
    let Some(a) = try_app().filter(|a| a.hwnd == hwnd) else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };
    match msg {
        WM_NCHITTEST => {
            let r = DefWindowProcW(hwnd, msg, wparam, lparam);
            if r != HTCLIENT as isize {
                return r;
            }
            let (x, y) = point(lparam);
            let mut pt = POINT { x, y };
            ScreenToClient(hwnd, &mut pt);
            if IsZoomed(hwnd) == 0 && pt.y < GetSystemMetricsForDpi(SM_CYSIZEFRAME, a.dpi) {
                return HTTOP as isize;
            }
            (match a.hit(pt.x, pt.y) {
                Hit::Min => HTMINBUTTON,
                Hit::Max => HTMAXBUTTON, // also what makes Windows 11 offer Snap Layouts
                Hit::Close => HTCLOSE,
                Hit::Caption => HTCAPTION,
                _ => HTCLIENT,
            }) as isize
        }
        // Caption buttons are ours: keep DefWindowProc from drawing classic ones.
        WM_NCMOUSEMOVE => {
            a.set_hover(hit_from_ht(wparam as u32));
            a.track_leave(true);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_NCMOUSELEAVE => {
            if a.chrome.hover.is_caption_button() || a.chrome.pressed.is_caption_button() {
                a.chrome.pressed = Hit::Nothing;
                a.set_hover(Hit::Nothing);
                a.invalidate_chrome();
            }
            0
        }
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if hit_from_ht(wparam as u32).is_caption_button() => {
            a.chrome.pressed = hit_from_ht(wparam as u32);
            a.invalidate_chrome();
            0
        }
        WM_NCLBUTTONUP if hit_from_ht(wparam as u32).is_caption_button() => {
            let h = hit_from_ht(wparam as u32);
            if a.chrome.pressed == h {
                a.caption_action(h);
            }
            a.chrome.pressed = Hit::Nothing;
            a.invalidate_chrome();
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = point(lparam);
            let h = a.hit(x, y);
            a.set_hover(if matches!(h, Hit::Tab(_) | Hit::TabClose(_) | Hit::NewTab | Hit::Menu(_)) { h } else { Hit::Nothing });
            if !a.tracking_mouse {
                a.tracking_mouse = true;
                a.track_leave(false);
            }
            0
        }
        WM_MOUSELEAVE => {
            a.tracking_mouse = false;
            a.set_hover(Hit::Nothing);
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = point(lparam);
            match a.hit(x, y) {
                Hit::Tab(i) => a.switch_to(i),
                h @ (Hit::TabClose(_) | Hit::NewTab) => {
                    a.chrome.pressed = h;
                    SetCapture(hwnd);
                }
                Hit::Menu(i) => a.track_menu(i, false),
                _ => {}
            }
            0
        }
        WM_LBUTTONUP => {
            if GetCapture() == hwnd {
                ReleaseCapture();
            }
            let (x, y) = point(lparam);
            let pressed = std::mem::replace(&mut a.chrome.pressed, Hit::Nothing);
            if a.hit(x, y) == pressed {
                match pressed {
                    Hit::TabClose(i) => {
                        a.close_tab(i);
                    }
                    Hit::NewTab => a.new_tab(),
                    _ => {}
                }
            }
            0
        }
        WM_MBUTTONUP => {
            let (x, y) = point(lparam);
            if let Hit::Tab(i) | Hit::TabClose(i) = a.hit(x, y) {
                a.close_tab(i);
            }
            0
        }
        WM_SYSCOMMAND if (wparam & 0xFFF0) as u32 == SC_KEYMENU => {
            // Alt+letter / F10 with no real menu bar: open ours. Alt+Space stays the system menu.
            match char::from_u32(lparam as u32).map(|c| c.to_ascii_lowercase()) {
                Some(' ') => DefWindowProcW(hwnd, msg, wparam, lparam),
                Some('f') | Some('\0') => {
                    a.track_menu(0, true);
                    0
                }
                Some('e') => {
                    a.track_menu(1, true);
                    0
                }
                Some('v') => {
                    a.track_menu(2, true);
                    0
                }
                _ => 0,
            }
        }
        WM_MENUSELECT => {
            let flags = (wparam >> 16) as u32 & 0xFFFF;
            let top = a.menus[nav().current];
            update_nav(|n| {
                n.on_popup_item = flags & MF_POPUP != 0;
                n.in_submenu = lparam as HMENU != top;
            });
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            a.paint_chrome(hdc);
            EndPaint(hwnd, &ps);
            0
        }
        WM_ERASEBKGND => 1,
        WM_CTLCOLOREDIT => theme::edit_color(wparam as HDC).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
        WM_SIZE => {
            a.chrome.hover = Hit::Nothing;
            a.layout();
            a.fit_scrollbars();
            a.invalidate_chrome();
            0
        }
        WM_ACTIVATE => {
            a.window_active = (wparam & 0xFFFF) != WA_INACTIVE as usize;
            a.invalidate_chrome();
            if a.window_active {
                SetFocus(a.edit());
            }
            0
        }
        WM_SETFOCUS => {
            SetFocus(a.edit());
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as u16;
            let code = (wparam >> 16) as u32;
            if a.docs.iter().any(|d| d.edit == lparam as HWND) {
                if lparam as HWND == a.edit() {
                    if code == EN_CHANGE {
                        a.update_title();
                        a.update_status();
                    }
                    if matches!(code, EN_CHANGE | EN_HSCROLL | EN_VSCROLL) {
                        a.fit_scrollbars();
                    }
                }
                return 0;
            }
            a.command(id);
            0
        }
        WM_INITMENUPOPUP => {
            a.init_menu(wparam as HMENU);
            0
        }
        update::WM_UPDATE_RESULT => {
            let outcome = *Box::from_raw(lparam as *mut update::Outcome);
            a.update_result(outcome, wparam != 0);
            0
        }
        WM_DROPFILES => {
            let drop = wparam as HDROP;
            let count = DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0);
            let mut paths = Vec::new();
            let mut buf = [0u16; 32768];
            for i in 0..count {
                let n = DragQueryFileW(drop, i, buf.as_mut_ptr(), buf.len() as u32) as usize;
                if n > 0 {
                    paths.push(String::from_utf16_lossy(&buf[..n]));
                }
            }
            DragFinish(drop);
            for p in paths {
                a.open_path(&p, None);
            }
            0
        }
        WM_DPICHANGED => {
            a.dpi = ((wparam >> 16) & 0xFFFF) as u32;
            let r = &*(lparam as *const RECT);
            SetWindowPos(hwnd, std::ptr::null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            a.chrome.set_dpi(a.dpi);
            a.status.set_dpi(a.dpi);
            a.make_font();
            a.invalidate_chrome();
            0
        }
        WM_CLOSE => {
            if a.confirm_close_all() {
                a.quit();
            }
            0
        }
        WM_QUERYENDSESSION => a.confirm_close_all() as isize,
        WM_ENDSESSION => {
            if wparam != 0 {
                a.save_placement();
                a.settings.save();
            }
            0
        }
        WM_DESTROY => {
            if !a.hook.is_null() {
                UnhookWinEvent(a.hook);
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// While a menu-bar popup is open: Left/Right and hovering another menu-bar
/// button switch popups (the modal menu loop would otherwise swallow both).
unsafe extern "system" fn menu_filter(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == MSGF_MENU as i32 {
        if let Some(a) = try_app() {
            let msg = &*(lparam as *const MSG);
            let n = MENU_COUNT;
            let state = nav();
            let target = match msg.message {
                WM_KEYDOWN if msg.wParam as u16 == VK_LEFT && !state.in_submenu => Some(((state.current + n - 1) % n, true)),
                WM_KEYDOWN if msg.wParam as u16 == VK_RIGHT && !state.on_popup_item => Some(((state.current + 1) % n, true)),
                WM_MOUSEMOVE => {
                    let mut pt = msg.pt;
                    ScreenToClient(a.hwnd, &mut pt);
                    match a.hit(pt.x, pt.y) {
                        Hit::Menu(j) if j != state.current => Some((j, false)),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some((next, keyboard)) = target {
                update_nav(|v| {
                    v.next = Some(next);
                    v.from_keyboard = keyboard;
                });
                EndMenu();
                return 1;
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// The edit control has no caret-moved notification; Notepad watches the
/// caret's accessibility location events instead.
unsafe extern "system" fn caret_event(_: HWINEVENTHOOK, _: u32, hwnd: HWND, id_object: i32, _: i32, _: u32, _: u32) {
    if id_object == OBJID_CARET {
        if let Some(a) = try_app() {
            if hwnd == a.edit() {
                a.update_status();
                a.fit_scrollbars(); // typing past the edge scrolls sideways
            }
        }
    }
}

unsafe extern "system" fn edit_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    let r = DefSubclassProc(hwnd, msg, wparam, lparam);
    if msg == WM_MOUSEWHEEL && (wparam & 0x0008) != 0 {
        // MK_CONTROL: the control just zoomed itself.
        if let Some(a) = try_app() {
            a.sync_zoom();
        }
    }
    r
}

unsafe extern "system" fn font_hook(dlg: HWND, msg: u32, wparam: WPARAM, _: LPARAM) -> usize {
    if msg == WM_INITDIALOG {
        theme::apply_dialog(dlg);
        return 1;
    }
    theme::dialog_color_msg(msg, wparam).map_or(0, |b| b as usize)
}

// ---------------------------------------------------------------- find/replace

fn find_template(replace: bool) -> Template {
    let mut t = Template::new(if replace { "Replace" } else { "Find" }, 0, 246, if replace { 94 } else { 62 });
    t.label("Fi&nd what:", 6, 9, 48, 8);
    t.item(EDIT, "", IDC_FIND_TEXT, WS_BORDER | WS_TABSTOP | WS_GROUP | ES_AUTOHSCROLL as u32, 56, 7, 122, 12);
    if replace {
        t.label("Re&place with:", 6, 27, 48, 8);
        t.item(EDIT, "", IDC_REPLACE_TEXT, WS_BORDER | WS_TABSTOP | WS_GROUP | ES_AUTOHSCROLL as u32, 56, 25, 122, 12);
        t.check("Match &case", IDC_MATCH_CASE, false, true, 6, 62, 90);
        t.check("W&rap around", IDC_WRAP_AROUND, false, true, 6, 76, 90);
        t.button("&Find Next", IDOK as u16, true, 184, 6);
        t.button("&Replace", IDC_REPLACE_ONE, false, 184, 24);
        t.button("Replace &All", IDC_REPLACE_ALL, false, 184, 42);
        t.button("Cancel", IDCANCEL as u16, false, 184, 60);
    } else {
        t.check("Match &case", IDC_MATCH_CASE, false, true, 6, 30, 90);
        t.check("W&rap around", IDC_WRAP_AROUND, false, true, 6, 44, 90);
        t.label("Direction", 104, 30, 60, 8);
        t.check("&Up", IDC_UP, true, true, 104, 44, 34);
        t.check("&Down", IDC_DOWN, true, false, 140, 44, 40);
        t.button("&Find Next", IDOK as u16, true, 184, 6);
        t.button("Cancel", IDCANCEL as u16, false, 184, 24);
    }
    t
}

unsafe fn read_find_dialog(dlg: HWND) {
    let a = app();
    let checked = |id: u16| IsDlgButtonChecked(dlg, id as i32) == BST_CHECKED;
    a.settings.search = dialogs::get_text(GetDlgItem(dlg, IDC_FIND_TEXT as i32));
    let rep = GetDlgItem(dlg, IDC_REPLACE_TEXT as i32);
    if !rep.is_null() {
        a.settings.replace = dialogs::get_text(rep);
    } else {
        a.settings.search_up = checked(IDC_UP);
    }
    a.settings.match_case = checked(IDC_MATCH_CASE);
    a.settings.wrap_around = checked(IDC_WRAP_AROUND);
}

unsafe fn enable_find_buttons(dlg: HWND) {
    let has = GetWindowTextLengthW(GetDlgItem(dlg, IDC_FIND_TEXT as i32)) > 0;
    for id in [IDOK as u16, IDC_REPLACE_ONE, IDC_REPLACE_ALL] {
        let b = GetDlgItem(dlg, id as i32);
        if !b.is_null() {
            EnableWindow(b, has as i32);
        }
    }
}

unsafe extern "system" fn find_proc(dlg: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    let _ = (BUTTON, LABEL_OFFSET, lparam);
    match msg {
        WM_INITDIALOG => {
            let a = app();
            let s = &a.settings;
            dialogs::set_text(GetDlgItem(dlg, IDC_FIND_TEXT as i32), &s.search);
            let rep = GetDlgItem(dlg, IDC_REPLACE_TEXT as i32);
            if !rep.is_null() {
                dialogs::set_text(rep, &s.replace);
            } else {
                CheckRadioButton(dlg, IDC_UP as i32, IDC_DOWN as i32, if s.search_up { IDC_UP } else { IDC_DOWN } as i32);
            }
            CheckDlgButton(dlg, IDC_MATCH_CASE as i32, if s.match_case { BST_CHECKED } else { BST_UNCHECKED });
            CheckDlgButton(dlg, IDC_WRAP_AROUND as i32, if s.wrap_around { BST_CHECKED } else { BST_UNCHECKED });
            enable_find_buttons(dlg);
            theme::apply_dialog(dlg);
            dialogs::center_on_owner(dlg);
            let edit = GetDlgItem(dlg, IDC_FIND_TEXT as i32);
            SetFocus(edit);
            SendMessageW(edit, EM_SETSEL, 0, -1);
            0 // focus set by hand
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as u16;
            let code = (wparam >> 16) as u32;
            if dialogs::forward_label_click(dlg, id, code) {
                return 1;
            }
            match id {
                IDC_FIND_TEXT if code == EN_CHANGE => enable_find_buttons(dlg),
                x if x == IDOK as u16 => {
                    read_find_dialog(dlg);
                    let a = app();
                    let down = !a.find_is_replace && !a.settings.search_up || a.find_is_replace;
                    a.find_next(down);
                }
                IDC_REPLACE_ONE => {
                    read_find_dialog(dlg);
                    app().replace_one();
                }
                IDC_REPLACE_ALL => {
                    read_find_dialog(dlg);
                    app().replace_all();
                }
                x if x == IDCANCEL as u16 => {
                    read_find_dialog(dlg);
                    DestroyWindow(dlg);
                }
                _ => {}
            }
            1
        }
        WM_CLOSE => {
            DestroyWindow(dlg);
            1
        }
        WM_DESTROY => {
            app().find_dlg = std::ptr::null_mut();
            0
        }
        _ => theme::dialog_color_msg(msg, wparam).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn group_inserts_thousands_separators() {
        assert_eq!(super::group(0), "0");
        assert_eq!(super::group(999), "999");
        assert_eq!(super::group(1000), "1,000");
        assert_eq!(super::group(1234567), "1,234,567");
    }
}

fn open_url(hwnd: HWND, url: &str) {
    let (verb, target) = (wide("open"), wide(url));
    unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(hwnd, verb.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}
