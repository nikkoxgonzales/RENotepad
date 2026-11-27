//! The Markdown preview control: a read-only RichEdit (Msftedit.dll, part of
//! every Windows since XP SP1) that shows `markdown::to_rtf`. app.rs owns one per
//! tab while that tab previews. windows-sys 0.61 has no RichEdit module, so the
//! few messages and structs it needs are declared here.

use crate::markdown::{self, Colors};
use crate::{dialogs, theme, wide};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::System::LibraryLoader::{LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
use windows_sys::Win32::UI::Controls::NMHDR;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const EM_GETTEXTRANGE: u32 = WM_USER + 75;
const EM_SETBKGNDCOLOR: u32 = WM_USER + 67;
const EM_SETEVENTMASK: u32 = WM_USER + 69;
const EM_SETTEXTEX: u32 = WM_USER + 97;
const EM_GETSCROLLPOS: u32 = WM_USER + 221;
const EM_SETSCROLLPOS: u32 = WM_USER + 222;
const EM_SETEDITSTYLE: u32 = WM_USER + 204;
const EM_GETZOOM: u32 = WM_USER + 224;
const EM_GETRECT: u32 = 0x00B2;
/// Without it, table cell edges that have no border show as gray gridlines.
const SES_HIDEGRIDLINES: usize = 0x0002_0000;
const ENM_LINK: isize = 0x0400_0000;
const EN_LINK: u32 = 0x070B;
const EM_SETREADONLY: u32 = 0x00CF;

#[repr(C)]
struct SetTextEx {
    flags: u32,
    codepage: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CharRange {
    min: i32,
    max: i32,
}

#[repr(C)]
struct TextRange {
    chrg: CharRange,
    text: *mut u16,
}

/// richedit.h packs its structs to 4 bytes: on x64, wparam starts at offset 28, not 32.
#[repr(C, packed(4))]
struct EnLink {
    hdr: NMHDR,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    chrg: CharRange,
}

/// A hidden, read-only preview control.
pub fn create(parent: HWND, id: usize) -> HWND {
    unsafe {
        // Loaded once and kept: the class lives as long as the DLL.
        static LOADED: std::sync::Once = std::sync::Once::new();
        LOADED.call_once(|| {
            LoadLibraryExW(wide("Msftedit.dll").as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        });
        let class = wide("RICHEDIT50W");
        let style = WS_CHILD | WS_CLIPSIBLINGS | WS_VSCROLL | ES_MULTILINE as u32 | ES_READONLY as u32;
        let h = CreateWindowExW(0, class.as_ptr(), std::ptr::null(), style, 0, 0, 0, 0, parent, id as HMENU, dialogs::hinstance(), std::ptr::null());
        SendMessageW(h, EM_SETEVENTMASK, 0, ENM_LINK);
        SendMessageW(h, EM_SETEDITSTYLE, SES_HIDEGRIDLINES, SES_HIDEGRIDLINES as isize);
        h
    }
}

/// GitHub's Markdown colors (github-markdown-css), for the app's theme:
/// (background, colors). Translucent GitHub colors are blended over the background.
fn colors() -> (u32, Colors) {
    let rgb = theme::rgb;
    if theme::is_dark() {
        let bg = rgb(0x0d, 0x11, 0x17);
        let c = Colors {
            text: rgb(0xf0, 0xf6, 0xfc),
            muted: rgb(0x91, 0x98, 0xa1),
            link: rgb(0x44, 0x93, 0xf8),
            code_bg: theme::mix(bg, rgb(0x65, 0x6c, 0x76), 0.2),
            pre_bg: rgb(0x15, 0x1b, 0x23),
            border: rgb(0x3d, 0x44, 0x4d),
            border_muted: theme::mix(bg, rgb(0x3d, 0x44, 0x4d), 0.7),
            mark: theme::mix(bg, rgb(0xbb, 0x80, 0x09), 0.15),
            alerts: [
                (rgb(0x1f, 0x6f, 0xeb), rgb(0x44, 0x93, 0xf8)),
                (rgb(0x23, 0x86, 0x36), rgb(0x3f, 0xb9, 0x50)),
                (rgb(0x89, 0x57, 0xe5), rgb(0xab, 0x7d, 0xf8)),
                (rgb(0x9e, 0x6a, 0x03), rgb(0xd2, 0x99, 0x22)),
                (rgb(0xda, 0x36, 0x33), rgb(0xf8, 0x51, 0x49)),
            ],
            syntax: [
                rgb(0x91, 0x98, 0xa1),
                rgb(0xff, 0x7b, 0x72),
                rgb(0xa5, 0xd6, 0xff),
                rgb(0x79, 0xc0, 0xff),
                rgb(0xd2, 0xa8, 0xff),
                rgb(0xff, 0xa6, 0x57),
                rgb(0x7e, 0xe7, 0x87),
                rgb(0xaf, 0xf5, 0xb4),
                rgb(0xff, 0xdc, 0xd7),
            ],
        };
        (bg, c)
    } else {
        let bg = rgb(0xff, 0xff, 0xff);
        let c = Colors {
            text: rgb(0x1f, 0x23, 0x28),
            muted: rgb(0x59, 0x63, 0x6e),
            link: rgb(0x09, 0x69, 0xda),
            code_bg: theme::mix(bg, rgb(0x81, 0x8b, 0x98), 0.2),
            pre_bg: rgb(0xf6, 0xf8, 0xfa),
            border: rgb(0xd1, 0xd9, 0xe0),
            border_muted: theme::mix(bg, rgb(0xd1, 0xd9, 0xe0), 0.7),
            mark: rgb(0xff, 0xf8, 0xc5),
            alerts: [
                (rgb(0x09, 0x69, 0xda), rgb(0x09, 0x69, 0xda)),
                (rgb(0x1a, 0x7f, 0x37), rgb(0x1a, 0x7f, 0x37)),
                (rgb(0x82, 0x50, 0xdf), rgb(0x82, 0x50, 0xdf)),
                (rgb(0x9a, 0x67, 0x00), rgb(0x9a, 0x67, 0x00)),
                (rgb(0xd1, 0x24, 0x2f), rgb(0xd1, 0x24, 0x2f)),
            ],
            syntax: [
                rgb(0x59, 0x63, 0x6e),
                rgb(0xcf, 0x22, 0x2e),
                rgb(0x0a, 0x30, 0x69),
                rgb(0x05, 0x50, 0xae),
                rgb(0x66, 0x39, 0xba),
                rgb(0x95, 0x38, 0x00),
                rgb(0x11, 0x63, 0x29),
                rgb(0x11, 0x63, 0x29),
                rgb(0x82, 0x07, 0x1e),
            ],
        };
        (bg, c)
    }
}

/// The width in twips that the text has at the control's size, DPI and zoom.
fn text_width(h: HWND) -> i32 {
    unsafe {
        let mut rc: RECT = std::mem::zeroed();
        SendMessageW(h, EM_GETRECT, 0, &mut rc as *mut RECT as isize);
        let dpi = GetDpiForWindow(h).max(96) as i32;
        let (mut num, mut den) = (0i32, 0i32);
        SendMessageW(h, EM_GETZOOM, &mut num as *mut i32 as usize, &mut den as *mut i32 as isize);
        let (num, den) = if num > 0 && den > 0 { (num, den) } else { (1, 1) };
        // A small margin: a row that reaches the edge exactly can wrap.
        ((rc.right - rc.left) * 1440 / dpi * den / num - 30).max(1500)
    }
}

/// The width in twips of a 10pt Consolas character as the control draws it:
/// GDI rounds glyph widths to whole pixels at the control's DPI and zoom.
fn code_char(h: HWND) -> f32 {
    use windows_sys::Win32::Graphics::Gdi::*;
    unsafe {
        let dpi = GetDpiForWindow(h).max(96) as i32;
        let (mut num, mut den) = (0i32, 0i32);
        SendMessageW(h, EM_GETZOOM, &mut num as *mut i32 as usize, &mut den as *mut i32 as isize);
        let (num, den) = if num > 0 && den > 0 { (num, den) } else { (1, 1) };
        let px = -((10 * dpi * num) as f32 / (72 * den) as f32).round() as i32;
        let face = wide("Consolas");
        let font = CreateFontW(px, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, face.as_ptr());
        let dc = GetDC(h);
        let old = SelectObject(dc, font);
        let sample = wide(&"M".repeat(64));
        let mut size = std::mem::zeroed();
        GetTextExtentPoint32W(dc, sample.as_ptr(), 64, &mut size);
        SelectObject(dc, old);
        ReleaseDC(h, dc);
        DeleteObject(font);
        if size.cx <= 0 {
            return 110.0;
        }
        size.cx as f32 * 1440.0 * den as f32 / (64.0 * dpi as f32 * num as f32)
    }
}

/// The preview's width changed since it last rendered: its tables and boxes must fit again.
pub fn width_changed(h: HWND) -> bool {
    unsafe { GetWindowLongPtrW(h, GWLP_USERDATA) as i32 != text_width(h) }
}

/// Show `md` in the theme's colors, keeping the scroll position (theme changes
/// re-render). `dir` is the file's folder, for pictures; `open` are the
/// `<details>` blocks that the user opened.
pub fn render(h: HWND, md: &str, dir: Option<&Path>, open: &[u32]) {
    let (bg, c) = colors();
    let width = text_width(h);
    let picture = |url: &str| load_picture(url, dir);
    let opts = markdown::Options { width, open_details: open, picture: &picture, code_char: code_char(h) };
    let mut rtf = markdown::to_rtf(md, &c, &opts).into_bytes();
    rtf.push(0);
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, width as isize);
        let mut pos = [0i32; 2];
        SendMessageW(h, EM_GETSCROLLPOS, 0, pos.as_mut_ptr() as isize);
        SendMessageW(h, WM_SETREDRAW, 0, 0);
        SendMessageW(h, EM_SETBKGNDCOLOR, 0, bg as isize);
        // The RTF is pure ASCII, so the code page does not matter.
        let st = SetTextEx { flags: 0, codepage: 0 };
        // A read-only RichEdit drops the pictures of RTF that it is given.
        SendMessageW(h, EM_SETREADONLY, 0, 0);
        SendMessageW(h, EM_SETTEXTEX, &st as *const SetTextEx as usize, rtf.as_ptr() as isize);
        SendMessageW(h, EM_SETREADONLY, 1, 0);
        SendMessageW(h, EM_SETSCROLLPOS, 0, pos.as_ptr() as isize);
        SendMessageW(h, WM_SETREDRAW, 1, 0);
        windows_sys::Win32::Graphics::Gdi::InvalidateRect(h, std::ptr::null(), 1);
    }
}

/// Larger picture files show as their alt text.
const MAX_PICTURE: u64 = 8 << 20;

type Loaded = Option<(bool, u32, u32, std::rc::Rc<str>)>;
/// A picture file's size and change time, and what loading it gave.
type CacheEntry = ((u64, Option<std::time::SystemTime>), Loaded);

/// A local PNG or JPEG for an image URL: a path relative to the file's folder,
/// or an absolute one. A URL with a scheme (http:, data:) or a network path
/// loads nothing: the preview makes no connection.
fn load_picture(url: &str, dir: Option<&Path>) -> Option<markdown::Picture> {
    if let Some(colon) = url.find(':') {
        if colon > 1 && url[..colon].chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
    }
    let path = markdown::percent_decode(url.split(['#', '?']).next().unwrap_or(""));
    if path.is_empty() || markdown::is_unc(&path) {
        return None;
    }
    let p = Path::new(&path);
    let full = if p.is_absolute() { p.to_path_buf() } else { dir?.join(p) };
    let meta = std::fs::metadata(&full).ok()?;
    if !meta.is_file() || meta.len() > MAX_PICTURE {
        return None;
    }
    let stamp = (meta.len(), meta.modified().ok());
    // Ceiling: the cache keeps every picture shown in this run.
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashMap<PathBuf, CacheEntry>> = Default::default();
    }
    let hit = CACHE.with(|c| c.borrow().get(&full).filter(|(s, _)| *s == stamp).map(|(_, v)| v.clone()));
    let v = match hit {
        Some(v) => v,
        None => {
            let v = std::fs::read(&full).ok().and_then(|b| {
                let (png, w, h) = markdown::picture_info(&b)?;
                let mut hex = String::with_capacity(b.len() * 2);
                for x in &b {
                    hex.push_str(&format!("{x:02x}"));
                }
                Some((png, w, h, std::rc::Rc::from(hex)))
            });
            CACHE.with(|c| c.borrow_mut().insert(full, (stamp, v.clone())));
            v
        }
    };
    v.map(|(png, width, height, hex)| markdown::Picture { png, width, height, hex })
}

/// The URL of a link the user just clicked, from a WM_NOTIFY.
///
/// # Safety
/// `lparam` must be the LPARAM of a WM_NOTIFY.
pub unsafe fn clicked_link(lparam: LPARAM) -> Option<String> {
    let hdr = &*(lparam as *const NMHDR);
    if hdr.code != EN_LINK {
        return None;
    }
    let link = std::ptr::read_unaligned(lparam as *const EnLink);
    if link.msg != WM_LBUTTONUP {
        return None;
    }
    let chrg = link.chrg;
    let len = (chrg.max - chrg.min).clamp(0, 1 << 16) as usize;
    let mut buf = vec![0u16; len + 1];
    let mut tr = TextRange { chrg, text: buf.as_mut_ptr() };
    let n = SendMessageW(hdr.hwndFrom, EM_GETTEXTRANGE, 0, &mut tr as *mut TextRange as isize).max(0) as usize;
    let range = String::from_utf16_lossy(&buf[..n.min(len)]);
    markdown::link_target(&range).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Foundation::LRESULT;

    /// RichEdit controls made on 2 test threads at once lose link clicks and
    /// sometimes crash. The app makes them on its UI thread only. Each test
    /// that makes one holds this lock.
    static RICHEDIT: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn render_md(h: HWND, md: &str) {
        render(h, md, None, &[]);
    }

    fn plain_text(h: HWND) -> String {
        let n = unsafe { GetWindowTextLengthW(h) };
        let mut buf = vec![0u16; n as usize + 1];
        let got = unsafe { GetWindowTextW(h, buf.as_mut_ptr(), n + 1) };
        String::from_utf16_lossy(&buf[..got.max(0) as usize])
    }

    /// The RTF from to_rtf loads in the real RichEdit: text, symbols and the
    /// link's hidden URL all come through.
    #[test]
    fn rendered_markdown_loads_in_richedit() {
        let _one = RICHEDIT.lock().unwrap_or_else(|e| e.into_inner());
        let class = wide("STATIC");
        let null = std::ptr::null_mut();
        let parent = unsafe { CreateWindowExW(0, class.as_ptr(), std::ptr::null(), WS_POPUP, 0, 0, 600, 400, null, null, null, std::ptr::null()) };
        let h = create(parent, 1);
        assert!(!h.is_null());
        render_md(h, "# Title\n\n- [x] done\n- item with [a link](docs/a.md)\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n`code` and é");
        let t = plain_text(h);
        assert!(t.contains("Title"), "{t:?}");
        assert!(t.contains("\u{2611}\tdone"), "{t:?}");
        assert!(t.contains("\u{2022}\titem with "), "{t:?}");
        assert!(t.contains("HYPERLINK \"docs/a.md\"a link"), "{t:?}");
        assert!(t.contains("code and \u{e9}"), "{t:?}");
        unsafe { DestroyWindow(parent) };
    }

    thread_local! {
        static CLICKED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    unsafe extern "system" fn record_links(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if msg == WM_NOTIFY {
            if let Some(url) = clicked_link(l) {
                CLICKED.with(|c| c.borrow_mut().push(url));
            }
        }
        DefWindowProcW(h, msg, w, l)
    }

    /// A local picture loads into the read-only control as one character, not as its alt text.
    #[test]
    fn a_local_picture_loads_into_the_control() {
        let _one = RICHEDIT.lock().unwrap_or_else(|e| e.into_inner());
        let class = wide("STATIC");
        let null = std::ptr::null_mut();
        let parent = unsafe { CreateWindowExW(0, class.as_ptr(), std::ptr::null(), WS_POPUP, 0, 0, 600, 400, null, null, null, std::ptr::null()) };
        let h = create(parent, 1);
        let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
        render(h, "a ![Logo](icon.png) b", Some(&docs), &[]);
        let t = plain_text(h);
        unsafe { DestroyWindow(parent) };
        assert_eq!(t.chars().count(), 5, "a, space, the picture, space, b: {t:?}");
        assert!(!t.contains("Logo"), "{t:?}");
    }

    /// A mouse click on a link's text reaches the parent as that link's URL.
    #[test]
    fn clicking_a_link_reports_its_url() {
        let _one = RICHEDIT.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            let class = wide("RENotepadPreviewTest");
            let wc = WNDCLASSW { lpfnWndProc: Some(record_links), lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
            RegisterClassW(&wc);
            let null = std::ptr::null_mut();
            let parent = CreateWindowExW(0, class.as_ptr(), std::ptr::null(), WS_POPUP, 0, 0, 600, 400, null, null, null, std::ptr::null());
            let h = create(parent, 1);
            SetWindowPos(h, null, 0, 0, 600, 400, SWP_NOZORDER);
            render_md(h, "[other file](docs/other.md)\n\nmore");
            let at = (12 << 16) | 20; // x 20, y 12: inside the first line's link text
            SendMessageW(h, WM_LBUTTONDOWN, 1, at);
            SendMessageW(h, WM_LBUTTONUP, 0, at);
            let got = CLICKED.with(|c| c.borrow().clone());
            DestroyWindow(parent);
            assert_eq!(got, vec!["docs/other.md".to_string()]);
        }
    }
}
