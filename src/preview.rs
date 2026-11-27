//! The Markdown preview control: a read-only RichEdit (Msftedit.dll, part of
//! every Windows since XP SP1) that shows `markdown::to_rtf`. app.rs owns one per
//! tab while that tab previews. windows-sys 0.61 has no RichEdit module, so the
//! few messages and structs it needs are declared here.

use crate::markdown::{self, Colors};
use crate::{dialogs, theme, wide};
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{GetSysColor, COLOR_WINDOW, COLOR_WINDOWTEXT};
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
/// Without it, table cell edges that have no border show as gray gridlines.
const SES_HIDEGRIDLINES: usize = 0x0002_0000;
const ENM_LINK: isize = 0x0400_0000;
const EN_LINK: u32 = 0x070B;

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

/// The preview colors for the current theme: (background, text colors).
fn colors() -> (u32, Colors) {
    if theme::is_dark() {
        let bg = theme::EDIT_BG;
        let c = Colors {
            text: theme::EDIT_FG,
            muted: theme::mix(bg, theme::EDIT_FG, 0.62),
            link: theme::rgb(76, 194, 255),
            code_bg: theme::rgb(54, 54, 54),
            rule: theme::rgb(72, 72, 72),
        };
        (bg, c)
    } else {
        let (bg, fg) = unsafe { (GetSysColor(COLOR_WINDOW), GetSysColor(COLOR_WINDOWTEXT)) };
        let c = Colors {
            text: fg,
            muted: theme::mix(bg, fg, 0.62),
            link: theme::rgb(0, 95, 184),
            code_bg: theme::mix(bg, fg, 0.06),
            rule: theme::mix(bg, fg, 0.2),
        };
        (bg, c)
    }
}

/// Show `md` in the theme's colors, keeping the scroll position (theme changes re-render).
pub fn render(h: HWND, md: &str) {
    let (bg, c) = colors();
    let mut rtf = markdown::to_rtf(md, &c).into_bytes();
    rtf.push(0);
    unsafe {
        let mut pos = [0i32; 2];
        SendMessageW(h, EM_GETSCROLLPOS, 0, pos.as_mut_ptr() as isize);
        SendMessageW(h, WM_SETREDRAW, 0, 0);
        SendMessageW(h, EM_SETBKGNDCOLOR, 0, bg as isize);
        // The RTF is pure ASCII, so the code page does not matter.
        let st = SetTextEx { flags: 0, codepage: 0 };
        SendMessageW(h, EM_SETTEXTEX, &st as *const SetTextEx as usize, rtf.as_ptr() as isize);
        SendMessageW(h, EM_SETSCROLLPOS, 0, pos.as_ptr() as isize);
        SendMessageW(h, WM_SETREDRAW, 1, 0);
        windows_sys::Win32::Graphics::Gdi::InvalidateRect(h, std::ptr::null(), 1);
    }
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
        let class = wide("STATIC");
        let null = std::ptr::null_mut();
        let parent = unsafe { CreateWindowExW(0, class.as_ptr(), std::ptr::null(), WS_POPUP, 0, 0, 600, 400, null, null, null, std::ptr::null()) };
        let h = create(parent, 1);
        assert!(!h.is_null());
        render(h, "# Title\n\n- [x] done\n- item with [a link](docs/a.md)\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n`code` and é");
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

    /// A mouse click on a link's text reaches the parent as that link's URL.
    #[test]
    fn clicking_a_link_reports_its_url() {
        unsafe {
            let class = wide("RENotepadPreviewTest");
            let wc = WNDCLASSW { lpfnWndProc: Some(record_links), lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
            RegisterClassW(&wc);
            let null = std::ptr::null_mut();
            let parent = CreateWindowExW(0, class.as_ptr(), std::ptr::null(), WS_POPUP, 0, 0, 600, 400, null, null, null, std::ptr::null());
            let h = create(parent, 1);
            SetWindowPos(h, null, 0, 0, 600, 400, SWP_NOZORDER);
            render(h, "[other file](docs/other.md)\n\nmore");
            let at = (12 << 16) | 20; // x 20, y 12: inside the first line's link text
            SendMessageW(h, WM_LBUTTONDOWN, 1, at);
            SendMessageW(h, WM_LBUTTONUP, 0, at);
            let got = CLICKED.with(|c| c.borrow().clone());
            DestroyWindow(parent);
            assert_eq!(got, vec!["docs/other.md".to_string()]);
        }
    }
}
