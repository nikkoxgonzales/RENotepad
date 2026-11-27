//! Main window: tabs, menus, edit controls, commands. Behavior follows
//! docs/WIN10-NOTEPAD-SPEC.md; the look (custom title bar with tabs, flat menu
//! bar, status bar) follows Windows 11 Notepad and is drawn by chrome.rs.

use crate::chrome::{Chrome, Hit, Layout, TabView, MENU_COUNT, SETTINGS_MENU};
use crate::dialogs::{self, Template, BUTTON, EDIT, LABEL_OFFSET};
use crate::encoding::{self, Encoding};
use crate::explorer::{self, Explorer};
use crate::settings::{self, Settings};
use crate::status::{self, Status};
use crate::{filedlg, jumplist, markdown, muldiv, preview, print, text, theme, update, wide};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_IGNORE_MERGE_ERRORS};
use windows_sys::Win32::System::DataExchange::IsClipboardFormatAvailable;
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::Controls::Dialogs::{ChooseFontW, CF_ENABLEHOOK, CF_INITTOLOGFONTSTRUCT, CF_NOVERTFONTS, CF_SCREENFONTS, CHOOSEFONTW};
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetCapture, GetFocus, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, VK_DOWN,
    VK_ESCAPE, VK_LEFT, VK_RIGHT,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, DragAcceptFiles, DragFinish, DragQueryFileW, PathCompactPathExW, SHAddToRecentDocs, SetWindowSubclass, HDROP, SHARD_PATHW};
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
const IDM_IMPORT_PINS: u16 = 51;
const IDM_PIN: u16 = 52;
const IDM_CLEAR_RECENT: u16 = 53;
const IDM_PREVIEW: u16 = 54;
const IDM_OPEN_FOLDER: u16 = 55;
/// View › Folder pane and the menu-bar button: show or hide.
const IDM_FOLDER_PANE: u16 = 56;
/// Ctrl+Shift+E: show the pane, then focus it, then hide it.
const IDM_FOLDER_PANE_KEY: u16 = 57;
/// File › Recent files: IDM_RECENT + index into settings::recent().
const IDM_RECENT: u16 = 100;

const EDIT_ID: usize = 15;
const PREVIEW_ID: usize = 14;
const TREE_ID: usize = 13;
/// Folder pane geometry at 96 DPI: the splitter, the narrowest pane, and the
/// window width under which the pane hides.
const SPLIT_W: i32 = 5;
const PANE_MIN: i32 = 160;
const PANE_HIDE_BELOW: i32 = 520;
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

/// A clicked preview link, posted to ourselves: LPARAM = Box<String> (the URL).
/// Opening a tab inside RichEdit's own click notification is not safe.
const WM_OPEN_LINK: u32 = WM_APP + 3;
/// A folder pane action, posted out of the tree's notification: LPARAM = Box<explorer::Action>.
const WM_FOLDER_ACTION: u32 = WM_APP + 6;

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
    /// The Markdown preview while this tab shows it, else null. Made from the
    /// editor text on each switch to it, and destroyed on the way back.
    preview: HWND,
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
    /// `fit_scrollbars` is running. Out of App for the same reason as NAV: it is
    /// read by a re-entrant call while the outer one holds `&mut self`.
    static FITTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// `confirm_close_all` is prompting. A second close request (taskbar, a queued
    /// Ctrl+Shift+W) reaches us through the prompt's message loop.
    static CLOSING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
    /// File › Recent files, filled each time it opens (init_menu).
    recent_menu: HMENU,
    window_active: bool,
    tracking_mouse: bool,
    /// Scrollbars the active editor needs (vertical, horizontal); the rest are
    /// pushed under the window edge / status bar, as Windows 11 shows none at rest.
    bars: (bool, bool),
    shown_title: String,
    hook: HWINEVENTHOOK,
    pub accel: HACCEL,
    pub global_accel: HACCEL,
    explorer: Explorer,
    /// Dragging the pane splitter: pointer x minus the pane width at the press.
    split_drag: Option<i32>,
    /// Dragging a file out of the folder pane.
    drag_file: Option<std::path::PathBuf>,
    /// The tab path the pane last showed in bold (None = show it again).
    revealed: Option<Option<String>>,
    /// The focus was in the folder pane when the window was deactivated.
    tree_had_focus: bool,
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

        let (menus, recent_menu) = build_menus();
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
            menus,
            recent_menu,
            window_active: true,
            tracking_mouse: false,
            bars: (true, true),
            shown_title: String::new(),
            hook: std::ptr::null_mut(),
            accel: accelerators(false),
            global_accel: accelerators(true),
            explorer: Explorer::empty(),
            split_drag: None,
            drag_file: None,
            revealed: None,
            tree_had_focus: false,
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
        a.explorer = Explorer::create(hwnd, TREE_ID, a.dpi, a.settings.show_hidden);
        if a.settings.folder_pane {
            // The pane comes back only with a root that still exists.
            match a.settings.folder_root.clone().map(std::path::PathBuf::from).filter(|r| r.is_dir()) {
                Some(root) => a.explorer.set_root(Some(root)),
                None => a.settings.folder_pane = false,
            }
        }
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
        // Once: Notepad's recent files seed File › Recent files. The flag is set
        // before the run, so a child that fails or crashes is not run again.
        if !settings::recent_imported() {
            settings::set_recent_imported();
            jumplist::run_async(hwnd, jumplist::IMPORT_RECENT, vec!["/ImportNotepadRecent".into()]);
        }
        SetFocus(a.view());
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
            (cs, b'M' as u16, IDM_PREVIEW),
            (cs, b'E' as u16, IDM_FOLDER_PANE_KEY),
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
fn build_menus() -> ([HMENU; MENU_COUNT], HMENU) {
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
        add(file, IDM_OPEN_FOLDER, "Op&en folder...");
        let recent = CreatePopupMenu();
        let w = wide("Recent &files");
        AppendMenuW(file, MF_POPUP, recent as usize, w.as_ptr());
        add(file, IDM_SAVE, "&Save\tCtrl+S");
        add(file, IDM_SAVE_AS, "Save &as...\tCtrl+Shift+S");
        add(file, IDM_SAVE_ALL, "Save a&ll");
        sep(file);
        add(file, IDM_PIN, "Pin to &jump list");
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
        sep(view);
        add(view, IDM_FOLDER_PANE, "&Folder pane\tCtrl+Shift+E");
        add(view, IDM_PREVIEW, "&Markdown preview	Ctrl+Shift+M");

        let settings = CreatePopupMenu();
        add(settings, IDM_DARK, "&Dark mode");
        sep(settings);
        add(settings, IDM_CHECK_UPDATES, "Check for &updates");
        add(settings, IDM_AUTO_UPDATES, "Check for updates at &startup");
        sep(settings);
        add(settings, IDM_IMPORT_PINS, "&Import pinned files from Notepad");
        sep(settings);
        add(settings, IDM_ABOUT, &format!("&About {APP_NAME}"));
        ([file, edit, view, settings], recent)
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

    /// The active tab's Markdown preview, or null while it shows the editor.
    fn preview(&self) -> HWND {
        self.docs.get(self.cur).map_or(std::ptr::null_mut(), |d| d.preview)
    }

    /// What the active tab shows: its preview or its editor. Takes the focus.
    fn view(&self) -> HWND {
        let p = self.preview();
        if p.is_null() { self.edit() } else { p }
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
            let height = (rc.bottom - sh - top).max(0);
            let pane = self.pane_width();
            let left = self.editor_left();
            let tree = self.explorer.hwnd;
            if pane > 0 {
                MoveWindow(tree, 0, top, pane, height, 1);
                ShowWindow(tree, SW_SHOWNA);
                InvalidateRect(self.hwnd, &self.splitter_rect(), 0);
            } else if GetWindowLongW(tree, GWL_STYLE) as u32 & WS_VISIBLE != 0 {
                // The style, not IsWindowVisible: that is false while the main window is hidden.
                let focus = GetFocus();
                ShowWindow(tree, SW_HIDE);
                if focus == tree || IsChild(tree, focus) != 0 {
                    SetFocus(self.view());
                }
            }
            let edit = self.edit();
            if !edit.is_null() {
                // An unneeded scrollbar is pushed past the window edge / under the status bar.
                let (v, h) = self.bars;
                let extra_w = if v { 0 } else { GetSystemMetricsForDpi(SM_CXVSCROLL, self.dpi) };
                let extra_h = if h || self.settings.wrap { 0 } else { GetSystemMetricsForDpi(SM_CYHSCROLL, self.dpi) };
                MoveWindow(edit, left, top, (rc.right - left).max(0) + extra_w, height + extra_h, 1);
                self.pad_edit(edit);
            }
            let p = self.preview();
            if !p.is_null() {
                // RichEdit re-wraps on a resize but repaints only the new strip;
                // without NOCOPYBITS the old wrap stays on screen while the user drags.
                SetWindowPos(p, std::ptr::null_mut(), left, top, (rc.right - left).max(0), height, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS);
                self.pad_edit(p);
            }
            MoveWindow(self.status.hwnd, 0, rc.bottom - sh, rc.right, sh, 1);
            ShowWindow(self.status.hwnd, if self.settings.status_bar { SW_SHOWNA } else { SW_HIDE });
        }
    }

    /// The pane shows: turned on, with a root, in a window wide enough.
    fn pane_shown(&self) -> bool {
        self.settings.folder_pane && self.explorer.root().is_some() && self.client_width() >= self.s(PANE_HIDE_BELOW)
    }

    /// Width of the folder pane in pixels, 0 when it is hidden.
    fn pane_width(&self) -> i32 {
        if !self.pane_shown() {
            return 0;
        }
        let max = (self.client_width() * 2 / 5).max(self.s(PANE_MIN));
        self.s(self.settings.folder_width).clamp(self.s(PANE_MIN), max)
    }

    /// Where the editor starts: right of the pane and its splitter.
    fn editor_left(&self) -> i32 {
        let w = self.pane_width();
        if w > 0 { w + self.s(SPLIT_W) } else { 0 }
    }

    fn splitter_rect(&self) -> RECT {
        let mut rc: RECT = unsafe { std::mem::zeroed() };
        unsafe { GetClientRect(self.hwnd, &mut rc) };
        let sh = if self.settings.status_bar { self.status.height() } else { 0 };
        let w = self.pane_width();
        RECT { left: w, top: self.chrome.top_height(), right: if w > 0 { w + self.s(SPLIT_W) } else { 0 }, bottom: rc.bottom - sh }
    }

    fn on_splitter(&self, x: i32, y: i32) -> bool {
        let r = self.splitter_rect();
        r.right > r.left && x >= r.left && x < r.right && y >= r.top && y < r.bottom
    }

    fn paint_splitter(&self, hdc: HDC) {
        let r = self.splitter_rect();
        if r.right > r.left {
            let p = theme::palette();
            theme::fill(hdc, &r, p.surface);
            theme::fill(hdc, &RECT { left: r.right - 1, ..r }, p.line);
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
        // Resizing the editor makes it send EN_VSCROLL / EN_HSCROLL back before the
        // new layout settles. Refitting from that half-done state flipped the bar
        // back every time and recursed until the stack overflowed; the loop below
        // re-checks once the layout has settled, so the nested call has nothing to do.
        if FITTING.with(|f| f.replace(true)) {
            return;
        }
        for _ in 0..3 {
            let need = self.scrollbar_need();
            if need == self.bars {
                break;
            }
            self.bars = need;
            self.layout();
            // Resizing re-insets the text without re-scrolling; keep the caret in view.
            unsafe { SendMessageW(self.edit(), EM_SCROLLCARET, 0, 0) };
        }
        FITTING.with(|f| f.set(false));
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
        let md = self.docs.get(self.cur).and_then(|d| d.path.as_deref()).is_some_and(markdown::is_markdown);
        self.chrome.preview = md.then(|| !self.preview().is_null());
        // The setting, not the width: in a narrow window the pane is on but hidden,
        // and the button and the menu check must still offer to turn it off.
        self.chrome.pane_shown = self.pane_on();
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
        self.follow_tab();
    }

    /// The folder pane shows the active tab's file in bold (only when it changed).
    fn follow_tab(&mut self) {
        let path = self.docs.get(self.cur).and_then(|d| d.path.clone());
        if self.revealed.as_ref() != Some(&path) {
            self.revealed = Some(path.clone());
            self.explorer.reveal(path.as_deref().map(std::path::Path::new));
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
        let pos = if self.preview().is_null() { format!("Ln {ln}, Col {col}") } else { "Markdown preview".into() };
        self.status.set(status::POS, &pos);
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
    fn jumplist_result(&mut self, what: usize, result: Option<jumplist::Tally>) {
        if what == jumplist::IMPORT_RECENT {
            return; // silent: File › Recent files reads the registry when it opens
        }
        if what == jumplist::PIN {
            self.set_message(match result {
                None => "Couldn't pin it to the jump list",
                Some(t) if t.missing > 0 => "Save the file first, then pin it",
                Some(t) if t.already > 0 => "Already pinned to the jump list",
                Some(_) => "Pinned to the taskbar jump list",
            });
            return;
        }
        self.set_message("");
        let files = |n: u32| if n == 1 { "1 file".to_string() } else { format!("{n} files") };
        let text = match result {
            None => "Couldn't import Notepad's pinned files.".to_string(),
            Some(t) => {
                let mut lines = Vec::new();
                if t.pinned > 0 {
                    lines.push(format!("Pinned {} from Notepad. Right-click {APP_NAME} on the taskbar to see them.", files(t.pinned)));
                }
                if t.already > 0 {
                    lines.push(format!("{} already pinned.", files(t.already)));
                }
                if t.missing > 0 {
                    lines.push(format!("Skipped {} that no longer exist where Notepad last saw them (moved, deleted, or on a drive that isn't connected).", files(t.missing)));
                }
                if lines.is_empty() {
                    lines.push("Notepad has no pinned files.".into());
                }
                lines.join("\n\n")
            }
        };
        dialogs::message_box(self.hwnd, "Import pinned files from Notepad", &text, &[("OK", IDOK)]);
    }

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
        self.docs.push(Doc { edit, preview: std::ptr::null_mut(), encoding: Encoding::Utf8, path: None, zoom: 100 });
        self.switch_to(self.docs.len() - 1);
    }

    fn switch_to(&mut self, i: usize) {
        if i >= self.docs.len() {
            return;
        }
        self.cur = i;
        unsafe {
            for (k, d) in self.docs.iter().enumerate() {
                if k != i || !d.preview.is_null() {
                    ShowWindow(d.edit, SW_HIDE);
                }
                if k != i && !d.preview.is_null() {
                    ShowWindow(d.preview, SW_HIDE);
                }
            }
            self.layout();
            ShowWindow(self.view(), SW_SHOW);
            SendMessageW(self.edit(), EM_SCROLLCARET, 0, 0);
            SetFocus(self.view());
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
        unsafe {
            DestroyWindow(d.edit);
            if !d.preview.is_null() {
                DestroyWindow(d.preview);
            }
        }
        let next = if self.cur > i { self.cur - 1 } else { self.cur.min(self.docs.len() - 1) };
        self.switch_to(next);
        true
    }

    /// Ask about every modified tab; false = cancelled, or already asking.
    fn confirm_close_all(&mut self) -> bool {
        if CLOSING.with(|c| c.replace(true)) {
            return false;
        }
        let mut ok = true;
        for i in 0..self.docs.len() {
            if self.doc_modified(i) {
                self.switch_to(i);
                if !self.check_save() {
                    ok = false;
                    break;
                }
            }
        }
        CLOSING.with(|c| c.set(false));
        ok
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

    /// Show the active tab as formatted Markdown (on) or as its text (off).
    fn set_preview(&mut self, on: bool) {
        if on == !self.preview().is_null() {
            return;
        }
        unsafe {
            if on {
                // Find and Replace act on the hidden text: close them.
                if !self.find_dlg.is_null() {
                    DestroyWindow(self.find_dlg);
                }
                let p = preview::create(self.hwnd, PREVIEW_ID);
                SetWindowSubclass(p, Some(preview_subclass), 1, 0);
                theme::apply_control(p, "DarkMode_Explorer");
                let zoom = self.doc().zoom;
                if zoom != 100 {
                    SendMessageW(p, EM_SETZOOM, zoom as usize, 100);
                }
                preview::render(p, &String::from_utf16_lossy(&get_edit_text(self.edit())));
                SetWindowPos(self.status.hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                self.doc_mut().preview = p;
            } else {
                // Cleared first: destroying the focused preview sends WM_SETFOCUS, which focuses view().
                let p = std::mem::replace(&mut self.doc_mut().preview, std::ptr::null_mut());
                DestroyWindow(p);
            }
        }
        self.switch_to(self.cur);
    }

    /// A link clicked in the preview: web links go to the browser, Markdown files open in a tab.
    fn open_link(&mut self, url: &str) {
        let dir = self.doc().path.as_deref().and_then(|p| std::path::Path::new(p).parent()).map(|d| d.to_path_buf());
        match markdown::resolve(url, dir.as_deref()) {
            markdown::Target::Web(u) => open_url(self.hwnd, &u),
            markdown::Target::File(p) => {
                // Normalized, so the already-open check in open_path matches.
                let p = std::path::absolute(&p).unwrap_or(p).to_string_lossy().into_owned();
                if std::path::Path::new(&p).is_file() {
                    self.open_path(&p, None);
                } else {
                    self.error(&format!("Cannot find the {p} file."));
                }
            }
            markdown::Target::Nothing => {}
        }
    }

    // ------------------------------------------------------------ folder pane

    /// Lay out again after the pane showed, hid or changed width.
    fn relayout(&mut self) {
        self.layout();
        self.fit_scrollbars();
        unsafe { InvalidateRect(self.hwnd, std::ptr::null(), 0) };
    }

    /// A new root for the pane (None = no folder).
    fn set_root(&mut self, root: Option<std::path::PathBuf>) {
        self.settings.folder_root = root.as_ref().map(|r| r.to_string_lossy().into_owned());
        self.explorer.set_root(root);
        self.revealed = None;
        self.follow_tab();
    }

    /// Open a folder in the pane and show it, or close the folder and hide the pane.
    fn set_folder(&mut self, root: Option<std::path::PathBuf>) {
        self.settings.folder_pane = root.is_some();
        self.set_root(root);
        self.relayout();
    }

    /// File › Open folder...
    fn open_folder(&mut self) {
        let start = self.explorer.root().map(|r| r.to_path_buf()).or_else(|| self.doc().path.as_deref().and_then(|p| std::path::Path::new(p).parent()).map(|d| d.to_path_buf()));
        if let Some(dir) = filedlg::pick_folder(self.hwnd, start.as_deref()) {
            self.set_folder(Some(dir.into()));
        }
    }

    /// Show or hide the pane. Showing it with no root roots it at the active
    /// file's folder, else asks for a folder. false = the user cancelled.
    fn show_pane(&mut self, on: bool) -> bool {
        if on && self.explorer.root().is_none() {
            let dir = self.doc().path.as_deref().and_then(|p| std::path::Path::new(p).parent()).filter(|d| d.is_dir()).map(|d| d.to_path_buf());
            let Some(root) = dir.or_else(|| filedlg::pick_folder(self.hwnd, None).map(Into::into)) else { return false };
            self.set_root(Some(root));
        }
        self.settings.folder_pane = on;
        self.relayout();
        true
    }

    fn pane_on(&self) -> bool {
        self.settings.folder_pane && self.explorer.root().is_some()
    }

    fn focus_in_tree(&self) -> bool {
        let f = unsafe { GetFocus() };
        !f.is_null() && (f == self.explorer.hwnd || unsafe { IsChild(self.explorer.hwnd, f) } != 0)
    }

    /// Ctrl+Shift+E: hidden → show and focus; shown → focus; focused → hide.
    fn folder_pane_key(&mut self) {
        if !self.pane_on() {
            if self.show_pane(true) && self.pane_shown() {
                unsafe { SetFocus(self.explorer.hwnd) };
            }
        } else if self.focus_in_tree() {
            self.show_pane(false);
            unsafe { SetFocus(self.view()) };
        } else if self.pane_shown() {
            unsafe { SetFocus(self.explorer.hwnd) };
        }
    }

    /// A command on the folder pane's selected item (or its root).
    fn tree_command(&mut self, id: u16) {
        use explorer::*;
        if id == CMD_OPEN_FOLDER {
            return self.open_folder();
        }
        let Some(t) = self.explorer.target() else { return };
        let folder = if t.kind == Kind::File { t.path.parent().map(|p| p.to_path_buf()) } else { Some(t.path.clone()) };
        match id {
            CMD_OPEN if t.kind == Kind::File => self.open_path(&t.path.to_string_lossy(), None),
            CMD_OPEN | CMD_TOGGLE => self.explorer.toggle(),
            CMD_NEW_WINDOW => {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe).arg(&t.path).spawn();
                }
            }
            CMD_REVEAL => reveal_in_explorer(&t.path),
            CMD_COPY_PATH => copy_text(self.hwnd, &t.path.to_string_lossy()),
            CMD_NEW_FILE | CMD_NEW_FOLDER => {
                if let Some(dir) = folder {
                    if let Err(e) = self.explorer.create_new(&dir, id == CMD_NEW_FOLDER) {
                        self.error(&e);
                    }
                }
            }
            CMD_RENAME => self.explorer.start_rename(),
            CMD_DELETE if t.kind != Kind::Root => self.delete_path(&t.path),
            CMD_SET_ROOT if t.kind != Kind::File => self.set_root(Some(t.path)),
            CMD_REFRESH => self.explorer.refresh_all(),
            CMD_COLLAPSE_ALL => self.explorer.collapse_all(),
            CMD_SHOW_HIDDEN => {
                self.settings.show_hidden = !self.settings.show_hidden;
                self.explorer.show_hidden = self.settings.show_hidden;
                self.explorer.refresh_all();
            }
            CMD_CLOSE_FOLDER => self.set_folder(None),
            _ => {}
        }
    }

    /// Delete from the pane: to the Recycle Bin. A tab of a deleted file stays
    /// open, marked modified, so its text can still be saved.
    fn delete_path(&mut self, path: &std::path::Path) {
        let inside = |d: &Doc| d.path.as_deref().is_some_and(|p| explorer::rel_components(path, std::path::Path::new(p)).is_some());
        let unsaved = (0..self.docs.len()).any(|i| inside(&self.docs[i]) && self.doc_modified(i));
        if unsaved {
            let name = path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
            let msg = format!("{name} is open with unsaved changes.\n\nMove it to the Recycle Bin anyway? Its tab stays open, so you can still save the text.");
            if dialogs::message_box(self.hwnd, APP_NAME, &msg, &[("&Delete", IDYES), ("Cancel", IDCANCEL)]) != IDYES {
                return;
            }
        }
        if !explorer::recycle(self.hwnd, path) {
            return;
        }
        self.explorer.forget(path);
        for d in self.docs.iter().filter(|d| inside(d)) {
            unsafe { SendMessageW(d.edit, EM_SETMODIFY, 1, 0) };
        }
        self.update_title();
        self.invalidate_chrome();
    }

    /// Run a folder pane action that came out of a tree notification.
    fn folder_action(&mut self, act: explorer::Action) {
        use explorer::Action;
        match act {
            Action::Open(p) => self.open_path(&p.to_string_lossy(), None),
            Action::Command(c) => self.tree_command(c),
            Action::FocusEditor => unsafe {
                SetFocus(self.view());
            },
            Action::Drag(p) => unsafe {
                use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_LBUTTON};
                if GetKeyState(VK_LBUTTON as i32) < 0 {
                    self.drag_file = Some(p);
                    SetCapture(self.hwnd);
                }
            },
            Action::Rename { id, name } => match self.explorer.rename(id, &name) {
                Ok(Some((old, new))) => {
                    let (old, new) = (old.to_string_lossy().into_owned(), new.to_string_lossy().into_owned());
                    for d in &mut self.docs {
                        if let Some(p) = d.path.as_mut() {
                            if explorer::rel_components(std::path::Path::new(&old), std::path::Path::new(p.as_str())).is_some() {
                                *p = format!("{new}{}", &p[old.len()..]);
                            }
                        }
                    }
                    self.revealed = None;
                    self.update_title();
                    self.invalidate_chrome();
                }
                Ok(None) => {}
                Err(e) => self.error(&e),
            },
        }
    }

    /// Drop of a file dragged out of the pane: on the editor, or on the title bar
    /// and the menu bar, opens it. The status bar and the pane do not.
    fn end_file_drag(&mut self, x: i32, y: i32) {
        let Some(path) = self.drag_file.take() else { return };
        unsafe { ReleaseCapture() };
        let mut rc: RECT = unsafe { std::mem::zeroed() };
        unsafe { GetClientRect(self.hwnd, &mut rc) };
        let sh = if self.settings.status_bar { self.status.height() } else { 0 };
        let on_target = x >= 0 && x < rc.right && y >= 0 && y < rc.bottom - sh && (y < self.chrome.top_height() || x >= self.editor_left());
        if on_target {
            self.open_path(&path.to_string_lossy(), None);
        }
    }

    // ------------------------------------------------------------ files

    fn open_from_command_line(&mut self, arg: &str, forced: Option<Encoding>) {
        let mut path = std::path::absolute(arg).unwrap_or_else(|_| arg.into());
        if path.is_dir() {
            // A folder (Notepad fails on one) opens in the folder pane.
            self.set_folder(Some(path));
            return;
        }
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
            // Opened again, so it moves to the top of the recent files like a first open.
            settings::add_recent(path);
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

    /// Load a file into the active tab. A file always opens in the editor.
    fn load(&mut self, path: &str, forced: Option<Encoding>) -> bool {
        self.set_preview(false);
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
        settings::add_recent(path);
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
        if let Err(e) = write_file(path, &bytes) {
            self.error(&format!("{path}\n{e}"));
            return false;
        }
        let d = self.doc_mut();
        d.path = Some(path.to_string());
        d.encoding = enc;
        settings::add_recent(path);
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
                let picked = &content[s.min(content.len())..e.min(content.len())];
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
        let p = self.preview();
        if !p.is_null() {
            unsafe { SendMessageW(p, EM_SETZOOM, z as usize, 100) };
            self.pad_edit(p);
        }
        self.fit_scrollbars();
        self.update_status();
    }

    /// The control zooms itself on Ctrl+wheel; read back what it chose, and
    /// give the tab's other control (editor or preview) the same zoom.
    fn sync_zoom(&mut self) {
        let (mut num, mut den) = (0i32, 0i32);
        unsafe { SendMessageW(self.view(), EM_GETZOOM, &mut num as *mut i32 as usize, &mut den as *mut i32 as isize) };
        self.set_zoom(if num > 0 && den > 0 { (num * 100 + den / 2) / den } else { 100 });
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
        self.explorer.apply_theme();
        for d in &self.docs {
            theme::apply_control(d.edit, "DarkMode_Explorer");
            if !d.preview.is_null() {
                theme::apply_control(d.preview, "DarkMode_Explorer");
                preview::render(d.preview, &String::from_utf16_lossy(&get_edit_text(d.edit)));
            }
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

    /// Fill File › Recent files from the registry; another window may have changed it.
    fn fill_recent_menu(&self) {
        unsafe {
            let m = self.recent_menu;
            while DeleteMenu(m, 0, MF_BYPOSITION) != 0 {}
            let list = settings::recent();
            if list.is_empty() {
                let w = wide("No recent files");
                AppendMenuW(m, MF_STRING | MF_GRAYED, 0, w.as_ptr());
                return;
            }
            for (i, path) in list.iter().enumerate() {
                // "&1" .. "&9", then "1&0"; a long path keeps its drive and file name.
                let mut short = [0u16; 64];
                let wpath = wide(path);
                let shown = if PathCompactPathExW(short.as_mut_ptr(), wpath.as_ptr(), short.len() as u32, 0) != 0 {
                    String::from_utf16_lossy(&short[..short.iter().position(|&c| c == 0).unwrap_or(0)])
                } else {
                    path.clone()
                };
                let n = i + 1;
                let key = if n < 10 { format!("&{n}") } else { format!("{}&{}", n / 10, n % 10) };
                let w = wide(&format!("{key}  {}", shown.replace('&', "&&")));
                AppendMenuW(m, MF_STRING, (IDM_RECENT as usize) + i, w.as_ptr());
            }
            AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
            let w = wide("&Clear recent files");
            AppendMenuW(m, MF_STRING, IDM_CLEAR_RECENT as usize, w.as_ptr());
        }
    }

    fn open_recent(&mut self, i: usize) {
        let Some(path) = settings::recent().get(i).cloned() else { return };
        let p = std::path::Path::new(&path);
        if !p.is_file() {
            // An unplugged drive or an offline share keeps its entry. Only a file
            // missing from a folder that is there is removed.
            if p.parent().is_some_and(|d| d.is_dir()) {
                settings::remove_recent(&path);
                self.error(&format!("Cannot find the {path} file.\n\nIt was removed from the recent files."));
            } else {
                self.error(&format!("Cannot find the {path} file."));
            }
            return;
        }
        self.open_path(&path, None);
    }

    fn init_menu(&self, menu: HMENU) {
        if menu == self.recent_menu {
            self.fill_recent_menu();
            return;
        }
        unsafe {
            let e = self.edit();
            // The preview is read-only: only Copy and Select all act on it.
            let editing = self.preview().is_null();
            let (mut s, mut end) = (0u32, 0u32);
            SendMessageW(self.view(), EM_GETSEL, &mut s as *mut u32 as usize, &mut end as *mut u32 as isize);
            let has_sel = end > s;
            let has_text = editing && GetWindowTextLengthW(e) > 0;
            let en = |id: u16, on: bool| {
                EnableMenuItem(menu, id as u32, MF_BYCOMMAND | if on { MF_ENABLED } else { MF_GRAYED });
            };
            let ck = |id: u16, on: bool| {
                CheckMenuItem(menu, id as u32, MF_BYCOMMAND | if on { MF_CHECKED } else { MF_UNCHECKED });
            };
            en(IDM_UNDO, editing && SendMessageW(e, EM_CANUNDO, 0, 0) != 0);
            en(IDM_CUT, editing && has_sel);
            en(IDM_COPY, has_sel);
            en(IDM_DELETE, editing && has_sel);
            en(IDM_PASTE, editing && IsClipboardFormatAvailable(13 /* CF_UNICODETEXT */) != 0);
            en(IDM_GOTO, editing);
            en(IDM_TIME_DATE, editing);
            ck(IDM_PREVIEW, !editing);
            en(IDM_FIND, has_text);
            en(IDM_FIND_NEXT, has_text);
            en(IDM_FIND_PREV, has_text);
            en(IDM_REPLACE, has_text);
            en(IDM_PIN, self.doc().path.is_some());
            ck(IDM_WRAP, self.settings.wrap);
            ck(IDM_STATUS_BAR, self.settings.status_bar);
            ck(IDM_DARK, self.settings.dark);
            ck(IDM_AUTO_UPDATES, self.settings.check_updates);
            ck(IDM_FOLDER_PANE, self.pane_on());
        }
    }

    fn command(&mut self, id: u16) {
        if self.focus_in_tree() {
            // Accelerators typed in the folder pane: F5 refreshes it, Ctrl+A selects
            // the name being renamed. Neither reaches the hidden editor.
            match id {
                IDM_TIME_DATE => return self.explorer.refresh_all(),
                IDM_SELECT_ALL => {
                    let f = unsafe { GetFocus() };
                    if f != self.explorer.hwnd {
                        unsafe { SendMessageW(f, EM_SETSEL, 0, -1) };
                    }
                    return;
                }
                _ => {}
            }
        }
        if (explorer::CMD_FIRST..=explorer::CMD_LAST).contains(&id) {
            return self.tree_command(id);
        }
        const EDITING_ONLY: [u16; 10] = [IDM_UNDO, IDM_CUT, IDM_PASTE, IDM_DELETE, IDM_FIND, IDM_FIND_NEXT, IDM_FIND_PREV, IDM_REPLACE, IDM_GOTO, IDM_TIME_DATE];
        if !self.preview().is_null() && EDITING_ONLY.contains(&id) {
            return; // an accelerator: the menu items are greyed out
        }
        let e = self.view();
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
                IDM_IMPORT_PINS => {
                    self.set_message("Importing Notepad's pinned files…");
                    jumplist::run_async(self.hwnd, jumplist::IMPORT, vec!["/ImportNotepadPins".into()]);
                }
                IDM_PIN => {
                    if let Some(path) = self.doc().path.clone() {
                        jumplist::run_async(self.hwnd, jumplist::PIN, vec!["/PinToJumpList".into(), path]);
                    }
                }
                IDM_CLEAR_RECENT => settings::set_recent(&[]),
                // By design (DECISIONS 2026-11-27): it works on any tab, not only .md, and
                // Save As to a .txt name keeps the preview on. Print prints the text, as Notepad does.
                IDM_PREVIEW => self.set_preview(self.preview().is_null()),
                IDM_OPEN_FOLDER => self.open_folder(),
                IDM_FOLDER_PANE => {
                    self.show_pane(!self.pane_on());
                }
                IDM_FOLDER_PANE_KEY => self.folder_pane_key(),
                _ if (IDM_RECENT..IDM_RECENT + settings::RECENT_MAX as u16).contains(&id) => self.open_recent((id - IDM_RECENT) as usize),
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
pub fn group(n: usize) -> String {
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

/// Save without ever leaving a half-written file: write the new bytes beside the
/// old file, flush them to disk, then swap them in. ReplaceFileW keeps the old
/// file's attributes, ACL and creation time. Falls back to writing in place, like
/// Notepad, when the folder won't take the temp file or the swap fails (some
/// network shares). Ceiling: the swap turns a hard-linked file into a separate copy.
fn write_file(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if !std::path::Path::new(path).exists() {
        return std::fs::write(path, bytes);
    }
    let tmp = format!("{path}.{}.tmp", std::process::id());
    let staged = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    if staged.is_ok() {
        let (target, staged) = (wide(path), wide(&tmp));
        let null = std::ptr::null();
        if unsafe { ReplaceFileW(target.as_ptr(), staged.as_ptr(), std::ptr::null(), REPLACEFILE_IGNORE_MERGE_ERRORS, null, null) } != 0 {
            return Ok(());
        }
    }
    let _ = std::fs::remove_file(&tmp);
    std::fs::write(path, bytes)
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
            if let Some(off) = a.split_drag {
                let max = (a.client_width() * 2 / 5).max(a.s(PANE_MIN));
                let w = (x - off).clamp(a.s(PANE_MIN), max);
                a.settings.folder_width = muldiv(w, 96, a.dpi as i32);
                a.layout();
                a.fit_scrollbars();
                return 0;
            }
            if a.drag_file.is_some() {
                let mut rc: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rc);
                let sh = if a.settings.status_bar { a.status.height() } else { 0 };
                let over_editor = y < rc.bottom - sh && (y < a.chrome.top_height() || x >= a.editor_left());
                SetCursor(LoadCursorW(std::ptr::null_mut(), if over_editor { IDC_HAND } else { IDC_NO }));
                return 0;
            }
            let h = a.hit(x, y);
            a.set_hover(if matches!(h, Hit::Tab(_) | Hit::TabClose(_) | Hit::NewTab | Hit::Menu(_) | Hit::Preview | Hit::Pane) { h } else { Hit::Nothing });
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
            if a.on_splitter(x, y) {
                a.split_drag = Some(x - a.pane_width());
                SetCapture(hwnd);
                return 0;
            }
            match a.hit(x, y) {
                Hit::Tab(i) => a.switch_to(i),
                h @ (Hit::TabClose(_) | Hit::NewTab | Hit::Preview | Hit::Pane) => {
                    a.chrome.pressed = h;
                    a.invalidate_chrome();
                    SetCapture(hwnd);
                }
                Hit::Menu(i) => a.track_menu(i, false),
                _ => {}
            }
            0
        }
        WM_LBUTTONUP => {
            let (x, y) = point(lparam);
            if a.split_drag.take().is_some() {
                ReleaseCapture();
                return 0;
            }
            if a.drag_file.is_some() {
                a.end_file_drag(x, y);
                return 0;
            }
            if GetCapture() == hwnd {
                ReleaseCapture();
            }
            let pressed = std::mem::replace(&mut a.chrome.pressed, Hit::Nothing);
            if a.hit(x, y) == pressed {
                match pressed {
                    Hit::TabClose(i) => {
                        a.close_tab(i);
                    }
                    Hit::NewTab => a.new_tab(),
                    Hit::Preview => a.command(IDM_PREVIEW),
                    Hit::Pane => a.command(IDM_FOLDER_PANE),
                    _ => {}
                }
            }
            a.invalidate_chrome();
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
        WM_CAPTURECHANGED if lparam as HWND != hwnd => {
            a.split_drag = None;
            a.drag_file = None;
            0
        }
        WM_SETCURSOR if wparam as HWND == hwnd && (lparam & 0xFFFF) as u32 == HTCLIENT => {
            let mut pt: POINT = std::mem::zeroed();
            GetCursorPos(&mut pt);
            ScreenToClient(hwnd, &mut pt);
            if a.on_splitter(pt.x, pt.y) {
                SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_SIZEWE));
                return 1;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CONTEXTMENU if wparam as HWND == a.explorer.hwnd => {
            let c = a.explorer.context_menu(lparam, a.settings.show_hidden);
            if c != 0 {
                a.tree_command(c);
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            a.paint_splitter(hdc);
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
                SetFocus(if a.tree_had_focus && a.pane_shown() { a.explorer.hwnd } else { a.view() });
            } else {
                a.tree_had_focus = a.focus_in_tree();
            }
            0
        }
        WM_SETFOCUS => {
            SetFocus(a.view());
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as u16;
            let code = (wparam >> 16) as u32;
            let from = lparam as HWND;
            // Null = a menu or accelerator command (a tab with no preview has a null one too).
            if !from.is_null() && a.docs.iter().any(|d| d.edit == from || d.preview == from) {
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
            // A control's notification (the folder pane's rename box sends EN_CHANGE
            // with id 1), not a command: id 1 would be New tab.
            if !from.is_null() {
                return 0;
            }
            a.command(id);
            0
        }
        WM_NOTIFY => {
            let hdr = &*(lparam as *const NMHDR);
            if !hdr.hwndFrom.is_null() && hdr.hwndFrom == a.explorer.hwnd {
                let (r, act) = a.explorer.notify(lparam);
                if let Some(act) = act {
                    let p = Box::into_raw(Box::new(act));
                    if PostMessageW(hwnd, WM_FOLDER_ACTION, 0, p as isize) == 0 {
                        drop(Box::from_raw(p));
                    }
                }
                return r;
            }
            if let Some(url) = preview::clicked_link(lparam) {
                PostMessageW(hwnd, WM_OPEN_LINK, 0, Box::into_raw(Box::new(url)) as isize);
            }
            0
        }
        WM_FOLDER_ACTION => {
            let act = *Box::from_raw(lparam as *mut explorer::Action);
            a.folder_action(act);
            0
        }
        explorer::WM_FOLDER_LISTED => {
            a.explorer.listed(*Box::from_raw(lparam as *mut explorer::Listing));
            0
        }
        explorer::WM_FOLDER_CHANGED => {
            a.explorer.changed(*Box::from_raw(lparam as *mut explorer::Changes));
            0
        }
        WM_OPEN_LINK => {
            let url = *Box::from_raw(lparam as *mut String);
            a.open_link(&url);
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
        jumplist::WM_JUMPLIST_RESULT => {
            a.jumplist_result(wparam, jumplist::Tally::from_code(lparam as u32));
            0
        }
        WM_DROPFILES => {
            let drop = wparam as HDROP;
            let count = DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0);
            let mut paths = Vec::new();
            for i in 0..count {
                let mut buf = vec![0u16; DragQueryFileW(drop, i, std::ptr::null_mut(), 0) as usize + 1];
                let n = DragQueryFileW(drop, i, buf.as_mut_ptr(), buf.len() as u32) as usize;
                if n > 0 {
                    paths.push(String::from_utf16_lossy(&buf[..n]));
                }
            }
            DragFinish(drop);
            for p in paths {
                if std::path::Path::new(&p).is_dir() {
                    a.set_folder(Some(p.into()));
                } else {
                    a.open_path(&p, None);
                }
            }
            0
        }
        WM_DPICHANGED => {
            a.dpi = ((wparam >> 16) & 0xFFFF) as u32;
            let r = &*(lparam as *const RECT);
            SetWindowPos(hwnd, std::ptr::null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            a.chrome.set_dpi(a.dpi);
            a.status.set_dpi(a.dpi);
            a.explorer.set_dpi(a.dpi);
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

/// The preview: Esc goes back to the editor, and Ctrl+wheel zooms the tab.
unsafe extern "system" fn preview_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    if msg == WM_KEYDOWN && wparam as u16 == VK_ESCAPE {
        PostMessageW(GetParent(hwnd), WM_COMMAND, IDM_PREVIEW as usize, 0);
        return 0;
    }
    edit_subclass(hwnd, msg, wparam, lparam, 0, 0)
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

fn open_url(hwnd: HWND, url: &str) {
    let (verb, target) = (wide("open"), wide(url));
    unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(hwnd, verb.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
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

    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("renotepad-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name).to_string_lossy().into_owned()
    }

    #[test]
    fn save_replaces_an_existing_file_and_leaves_no_temp() {
        let path = scratch("replace.txt");
        std::fs::write(&path, b"old content that is longer").unwrap();
        super::write_file(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert!(!std::path::Path::new(&format!("{path}.{}.tmp", std::process::id())).exists());
    }

    #[test]
    fn save_keeps_the_hidden_attribute() {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        let path = scratch("hidden.txt");
        std::fs::write(&path, b"old").unwrap();
        let mut perms_cmd = std::process::Command::new("attrib");
        assert!(perms_cmd.args(["+h", &path]).status().unwrap().success());
        super::write_file(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_ne!(std::fs::metadata(&path).unwrap().file_attributes() & HIDDEN, 0);
    }

    #[test]
    fn save_creates_a_new_file() {
        let path = scratch("fresh.txt");
        let _ = std::fs::remove_file(&path);
        super::write_file(&path, b"hello").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
    }
}
