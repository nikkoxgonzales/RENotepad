//! RENotepad (RE:Notepad) by Nikko Gonzales: Windows 10 Notepad, reverse-engineered
//! and rebuilt in Rust, with the Windows 11 look. Dark by default.
#![windows_subsystem = "windows"]

mod app;
mod chrome;
mod content_dialog;
mod dialogs;
mod encoding;
mod filedlg;
mod jumplist;
mod print;
mod settings;
mod status;
mod text;
mod theme;
mod update;

use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::System::Environment::GetCommandLineW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, IsChild, IsDialogMessageW, TranslateAcceleratorW, TranslateMessage, MSG, SW_SHOWDEFAULT,
};

/// NUL-terminated UTF-16 for Win32.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// a * b / c, rounded, like Win32 MulDiv.
pub fn muldiv(a: i32, b: i32, c: i32) -> i32 {
    if c == 0 {
        return -1;
    }
    let p = a as i64 * b as i64;
    let half = (c as i64).abs() / 2;
    let r = if (p < 0) != (c < 0) { (p - half) / c as i64 } else { (p + half) / c as i64 };
    r as i32
}

fn main() {
    unsafe {
        CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let cmdline = {
            let p = GetCommandLineW();
            let mut n = 0;
            while *p.add(n) != 0 {
                n += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
        };
        let args = text::parse_args(&cmdline);
        if args.import_pins || args.pin {
            let result = if args.pin { jumplist::pin_file(args.file.as_deref().unwrap_or("")) } else { jumplist::import_notepad_pins() };
            let code = result.map_or(jumplist::FAILED, |t| t.code());
            CoUninitialize();
            std::process::exit(code as i32);
        }
        // Before any window: the taskbar button and jump list belong to this ID.
        jumplist::set_app_id();
        if app::create(SW_SHOWDEFAULT, &args).is_some() {
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                let a = app::app();
                let find = a.find_dlg;
                if !find.is_null() && (msg.hwnd == find || IsChild(find, msg.hwnd) != 0)
                    && (TranslateAcceleratorW(a.hwnd, a.global_accel, &msg) != 0
                        || dialogs::check_mnemonic(find, &msg)
                        || IsDialogMessageW(find, &msg) != 0)
                    {
                        continue;
                    }
                if TranslateAcceleratorW(a.hwnd, a.accel, &msg) != 0 {
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        CoUninitialize();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn muldiv_rounds_like_win32() {
        assert_eq!(super::muldiv(110, 96, 720), 15);
        assert_eq!(super::muldiv(110, 144, 720), 22);
        assert_eq!(super::muldiv(-7, 3, 2), -11);
    }
}
