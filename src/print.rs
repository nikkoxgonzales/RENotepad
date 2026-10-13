//! Page Setup and Print. Same page model as Windows 10 Notepad: margins in
//! thousandths of an inch, file name as header, "Page N" as footer, text
//! word-wrapped and paged with DrawTextEx.

use crate::wide;
use std::cell::Cell;
use windows_sys::Win32::Foundation::{HGLOBAL, HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Storage::Xps::{EndDoc, EndPage, StartDocW, StartPage, DOCINFOW};
use windows_sys::Win32::UI::Controls::Dialogs::*;

thread_local! {
    static DEVMODE: Cell<HGLOBAL> = const { Cell::new(std::ptr::null_mut()) };
    static DEVNAMES: Cell<HGLOBAL> = const { Cell::new(std::ptr::null_mut()) };
}

/// Returns new margins [left, top, right, bottom] if the user pressed OK.
pub fn page_setup(owner: HWND, margins: [i32; 4]) -> Option<[i32; 4]> {
    unsafe {
        let mut psd: PAGESETUPDLGW = std::mem::zeroed();
        psd.lStructSize = std::mem::size_of::<PAGESETUPDLGW>() as u32;
        psd.hwndOwner = owner;
        psd.hDevMode = DEVMODE.get();
        psd.hDevNames = DEVNAMES.get();
        psd.Flags = PSD_INTHOUSANDTHSOFINCHES | PSD_MARGINS;
        psd.rtMargin = RECT { left: margins[0], top: margins[1], right: margins[2], bottom: margins[3] };
        if PageSetupDlgW(&mut psd) == 0 {
            return None;
        }
        DEVMODE.set(psd.hDevMode);
        DEVNAMES.set(psd.hDevNames);
        let m = psd.rtMargin;
        Some([m.left, m.top, m.right, m.bottom])
    }
}

pub struct Job<'a> {
    pub text: &'a [u16],
    pub title: &'a str,
    pub face: &'a str,
    pub point_size: i32,
    pub weight: i32,
    pub italic: bool,
    pub margins: [i32; 4],
}

/// `ask` = show the Print dialog; otherwise the default printer (/P).
pub fn print(owner: HWND, job: &Job, ask: bool) -> Result<(), String> {
    unsafe {
        let mut pd: PRINTDLGW = std::mem::zeroed();
        pd.lStructSize = std::mem::size_of::<PRINTDLGW>() as u32;
        pd.hwndOwner = owner;
        pd.hDevMode = DEVMODE.get();
        pd.hDevNames = DEVNAMES.get();
        pd.Flags = PD_RETURNDC | PD_NOPAGENUMS | PD_NOSELECTION | PD_USEDEVMODECOPIESANDCOLLATE;
        if !ask {
            pd.Flags |= PD_RETURNDEFAULT;
            pd.hDevMode = std::ptr::null_mut();
            pd.hDevNames = std::ptr::null_mut();
        }
        if PrintDlgW(&mut pd) == 0 {
            return if CommDlgExtendedError() == 0 { Ok(()) } else { Err("No printer is available.".into()) };
        }
        if ask {
            DEVMODE.set(pd.hDevMode);
            DEVNAMES.set(pd.hDevNames);
        }
        let hdc = pd.hDC;
        if hdc.is_null() {
            return Err("Could not open the printer.".into());
        }
        let r = print_dc(hdc, job);
        DeleteDC(hdc);
        r
    }
}

unsafe fn print_dc(hdc: HDC, job: &Job) -> Result<(), String> {
    let dpi_x = GetDeviceCaps(hdc, LOGPIXELSX as i32);
    let dpi_y = GetDeviceCaps(hdc, LOGPIXELSY as i32);
    let off_x = GetDeviceCaps(hdc, PHYSICALOFFSETX as i32);
    let off_y = GetDeviceCaps(hdc, PHYSICALOFFSETY as i32);
    let phys_w = GetDeviceCaps(hdc, PHYSICALWIDTH as i32);
    let phys_h = GetDeviceCaps(hdc, PHYSICALHEIGHT as i32);
    let printable_w = GetDeviceCaps(hdc, HORZRES as i32);
    let printable_h = GetDeviceCaps(hdc, VERTRES as i32);
    let [ml, mt, mr, mb] = job.margins;
    let mut page = RECT {
        left: (crate::muldiv(ml, dpi_x, 1000) - off_x).max(0),
        top: (crate::muldiv(mt, dpi_y, 1000) - off_y).max(0),
        right: (phys_w - crate::muldiv(mr, dpi_x, 1000) - off_x).min(printable_w),
        bottom: (phys_h - crate::muldiv(mb, dpi_y, 1000) - off_y).min(printable_h),
    };

    let mut lf: LOGFONTW = std::mem::zeroed();
    lf.lfHeight = -crate::muldiv(job.point_size, dpi_y, 720);
    lf.lfWeight = job.weight;
    lf.lfItalic = job.italic as u8;
    lf.lfCharSet = DEFAULT_CHARSET;
    for (d, s) in lf.lfFaceName.iter_mut().zip(job.face.encode_utf16().take(31)) {
        *d = s;
    }
    let font = CreateFontIndirectW(&lf);
    let old = SelectObject(hdc, font);
    SetBkMode(hdc, TRANSPARENT as i32);
    let mut tm: TEXTMETRICW = std::mem::zeroed();
    GetTextMetricsW(hdc, &mut tm);
    let line_h = tm.tmHeight + tm.tmExternalLeading;

    // Header line at the top of the margin box, footer line at the bottom.
    let header = RECT { bottom: page.top + line_h, ..page };
    let footer = RECT { top: page.bottom - line_h, ..page };
    page.top += line_h * 2;
    page.bottom -= line_h * 2;
    if page.bottom - page.top < line_h {
        SelectObject(hdc, old);
        DeleteObject(font);
        return Err("The page margins leave no room for text.".into());
    }

    let name = wide(job.title);
    let mut doc: DOCINFOW = std::mem::zeroed();
    doc.cbSize = std::mem::size_of::<DOCINFOW>() as i32;
    doc.lpszDocName = name.as_ptr();
    let mut result = Ok(());
    if StartDocW(hdc, &doc) <= 0 {
        result = Err("Printing could not start.".into());
    } else {
        let mut rest = job.text;
        let mut n = 1;
        loop {
            if StartPage(hdc) <= 0 {
                result = Err("Printing failed.".into());
                break;
            }
            let title: Vec<u16> = job.title.encode_utf16().collect();
            let mut h = header;
            DrawTextW(hdc, title.as_ptr(), title.len() as i32, &mut h, DT_CENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
            let foot: Vec<u16> = format!("Page {n}").encode_utf16().collect();
            let mut f = footer;
            DrawTextW(hdc, foot.as_ptr(), foot.len() as i32, &mut f, DT_CENTER | DT_SINGLELINE | DT_NOPREFIX);
            let mut params = DRAWTEXTPARAMS { cbSize: std::mem::size_of::<DRAWTEXTPARAMS>() as u32, iTabLength: 8, ..std::mem::zeroed() };
            let mut body = page;
            if !rest.is_empty() {
                DrawTextExW(hdc, rest.as_ptr() as *mut u16, rest.len() as i32, &mut body, DT_WORDBREAK | DT_EXPANDTABS | DT_NOPREFIX | DT_EDITCONTROL, &mut params as *mut DRAWTEXTPARAMS); // the API writes uiLengthDrawn
            }
            if EndPage(hdc) <= 0 {
                result = Err("Printing failed.".into());
                break;
            }
            let drawn = params.uiLengthDrawn as usize;
            if rest.is_empty() || drawn == 0 || drawn >= rest.len() {
                break;
            }
            rest = &rest[drawn..];
            n += 1;
        }
        EndDoc(hdc);
    }
    SelectObject(hdc, old);
    DeleteObject(font);
    result
}
