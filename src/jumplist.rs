//! Taskbar jump list: RENotepad's own AppUserModelID, and bringing over the
//! files pinned to Notepad's jump list.
//!
//! Windows has no public API for pinning. The shell's own AutomaticDestinationList
//! object (undocumented, the one Explorer uses) can read and pin; its layout
//! here was probed on Windows 11 and is the one documented for Windows 10. A
//! changed layout would crash, so importing and pinning run in a child
//! process (`/ImportNotepadPins`, `/PinToJumpList file`), never in the editor,
//! and only when asked.

use crate::wide;
use windows::core::{interface, IUnknown, IUnknown_Vtbl, Interface, BOOL, GUID, HRESULT, HSTRING, PCWSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::Common::IObjectArray;
use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName, SIGDN_DESKTOPABSOLUTEPARSING};
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Never change it: the taskbar button, the Start menu shortcut and the jump
/// list (pins included) all hang off this string.
pub const APP_ID: &str = "NikkoGonzales.RENotepad";

/// Notepad's jump lists: the Windows 11 app, then classic Notepad, whose ID is
/// its path with the known folder as a GUID (System32, Windows, SysWOW64).
const NOTEPAD_IDS: [&str; 4] = [
    "Microsoft.WindowsNotepad_8wekyb3d8bbwe!App",
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\notepad.exe",
    "{F38BF404-1D43-42F2-9305-67DE0B28FC23}\\notepad.exe",
    "{D65231B0-B2F1-4857-A4CE-A8E7C6EA7D27}\\notepad.exe",
];

/// Posted to the main window when an import or a pin finishes (see run_async).
pub const WM_JUMPLIST_RESULT: u32 = WM_APP + 2;

pub fn set_app_id() {
    let id = wide(APP_ID);
    unsafe { windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(id.as_ptr()) };
}


#[interface("E9C5EF8D-FD41-4F72-BA87-EB03BAD5817C")]
unsafe trait IAutomaticDestinationList: IUnknown {
    fn initialize(&self, app_id: PCWSTR, lnk: PCWSTR, unused: PCWSTR) -> HRESULT;
    fn has_list(&self, has: *mut BOOL) -> HRESULT;
    /// kind 0 = pinned, 1 = recent, 2 = frequent.
    fn get_list(&self, kind: i32, max: i32, flags: i32, riid: *const GUID, list: *mut *mut core::ffi::c_void) -> HRESULT;
    fn add_usage_point(&self, item: *mut core::ffi::c_void) -> HRESULT;
    /// index -1 = pin at the end, -2 = unpin.
    fn pin_item(&self, item: *mut core::ffi::c_void, index: i32) -> HRESULT;
    /// S_OK + position when pinned, ERROR_NOT_FOUND otherwise.
    fn is_pinned(&self, item: *mut core::ffi::c_void, index: *mut i32) -> HRESULT;
}

const CLSID_AUTOMATIC_DESTINATION_LIST: GUID = GUID::from_u128(0xF0AE1542_F497_484B_A175_A20DB09144BA);

fn open(app_id: &str) -> windows::core::Result<IAutomaticDestinationList> {
    unsafe {
        let list: IAutomaticDestinationList = CoCreateInstance(&CLSID_AUTOMATIC_DESTINATION_LIST, None, CLSCTX_INPROC_SERVER)?;
        let id = wide(app_id);
        list.initialize(PCWSTR(id.as_ptr()), PCWSTR::null(), PCWSTR::null()).ok()?;
        Ok(list)
    }
}

/// The paths pinned to `app_id`'s jump list, in order.
fn pinned(app_id: &str) -> windows::core::Result<Vec<String>> {
    let list = open(app_id)?;
    unsafe {
        let mut has = BOOL(0);
        if list.has_list(&mut has).is_err() || !has.as_bool() {
            return Ok(Vec::new());
        }
        let mut raw = std::ptr::null_mut();
        list.get_list(0, 1000, 1, &IObjectArray::IID, &mut raw).ok()?;
        let items = IObjectArray::from_raw(raw);
        let mut out = Vec::new();
        for i in 0..items.GetCount()? {
            let Ok(item) = items.GetAt::<IShellItem>(i) else { continue };
            let Ok(name) = item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING) else { continue };
            let path = name.to_string().unwrap_or_default();
            CoTaskMemFree(Some(name.0 as _));
            out.push(path);
        }
        Ok(out)
    }
}

/// What an import or a pin did. It travels as the child process's exit code.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Newly pinned to RENotepad.
    pub pinned: u32,
    /// Already pinned here.
    pub already: u32,
    /// Skipped: the file isn't there (moved, deleted, or its drive is offline).
    pub missing: u32,
}

impl Tally {
    /// 10 bits each: pinned, already, missing. The top two bits stay clear.
    pub fn code(&self) -> u32 {
        self.pinned.min(1023) | self.already.min(1023) << 10 | self.missing.min(1023) << 20
    }

    /// None for FAILED and for crash codes (NTSTATUS, high bit set), which
    /// a real result never has.
    pub fn from_code(code: u32) -> Option<Tally> {
        (code & 0xC000_0000 == 0).then_some(Tally { pinned: code & 1023, already: code >> 10 & 1023, missing: code >> 20 & 1023 })
    }
}

pub const FAILED: u32 = u32::MAX;

fn target() -> windows::core::Result<IAutomaticDestinationList> {
    open(&std::env::var("RENOTEPAD_PINS_TO").unwrap_or_else(|_| APP_ID.to_string()))
}

/// Pins `path` at the end of `list` unless it's already there or gone.
fn pin_into(list: &IAutomaticDestinationList, path: &str, tally: &mut Tally) -> windows::core::Result<()> {
    if !std::path::Path::new(path).is_file() {
        tally.missing += 1;
        return Ok(());
    }
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path), None)?;
        let mut at = 0;
        if list.is_pinned(item.as_raw(), &mut at).is_ok() {
            tally.already += 1;
        } else {
            list.pin_item(item.as_raw(), -1).ok()?;
            tally.pinned += 1;
        }
    }
    Ok(())
}

/// Pins every file pinned in Notepad's jump lists to RENotepad's, in Notepad's
/// order, at the end of what RENotepad already has. Files that no longer exist
/// are skipped: a pin to them would only open a "can't find" box. Notepad's
/// lists are only read. Test hooks: RENOTEPAD_PINS_FROM / RENOTEPAD_PINS_TO
/// replace the source IDs and the target ID.
pub fn import_notepad_pins() -> windows::core::Result<Tally> {
    let from = std::env::var("RENOTEPAD_PINS_FROM").ok();
    let sources: Vec<&str> = match &from {
        Some(f) => f.split(';').collect(),
        None => NOTEPAD_IDS.to_vec(),
    };
    let target = target()?;
    let mut seen = Vec::new();
    let mut tally = Tally::default();
    for source in sources {
        // A Notepad that was never installed or never used has no list.
        let Ok(items) = pinned(source) else { continue };
        for path in items {
            let key = path.to_lowercase();
            if !seen.contains(&key) {
                seen.push(key);
                pin_into(&target, &path, &mut tally)?;
            }
        }
    }
    Ok(tally)
}

/// Pins one file to RENotepad's jump list (`/PinToJumpList file`). With
/// "Show recently opened items" off, Windows lists no recent files to pin
/// from, so this is the way in.
pub fn pin_file(path: &str) -> windows::core::Result<Tally> {
    let mut tally = Tally::default();
    pin_into(&target()?, path, &mut tally)?;
    Ok(tally)
}

/// What a WM_JUMPLIST_RESULT is about (its wParam).
pub const IMPORT: usize = 0;
pub const PIN: usize = 1;

/// Runs `RENotepad.exe <args>` and posts WM_JUMPLIST_RESULT (wParam = `what`,
/// lParam = exit code, FAILED if it crashed, hung or couldn't start).
pub fn run_async(hwnd: HWND, what: usize, args: Vec<String>) {
    let target = hwnd as usize;
    std::thread::spawn(move || {
        let code = run_child(&args).unwrap_or(FAILED);
        unsafe { PostMessageW(target as HWND, WM_JUMPLIST_RESULT, what as WPARAM, code as LPARAM) };
    });
}

fn run_child(args: &[String]) -> Option<u32> {
    let mut child = std::process::Command::new(std::env::current_exe().ok()?).args(args).spawn().ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().ok()? {
            return status.code().map(|c| c as u32);
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_round_trips() {
        for t in [
            Tally::default(),
            Tally { pinned: 4, already: 0, missing: 0 },
            Tally { pinned: 2, already: 3, missing: 4 },
            Tally { pinned: 1023, already: 1023, missing: 1023 },
        ] {
            assert_eq!(Tally::from_code(t.code()), Some(t));
        }
        assert_eq!(Tally::from_code(FAILED), None);
        assert_eq!(Tally::from_code(0xC000_0005), None); // access violation
    }
}