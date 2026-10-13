//! Open / Save As: the Vista file dialog with Notepad's "Encoding" combo.
//! (Win10 Notepad gets the same UI via GetSaveFileName + a template that comdlg
//! auto-upgrades; IFileDialogCustomize is the documented way to get there.)

use crate::encoding::Encoding;
use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, FileSaveDialog, IFileDialog, IFileDialogCustomize, IFileOpenDialog, IFileSaveDialog, IShellItem,
    SHCreateItemFromParsingName, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST,
    SIGDN_FILESYSPATH,
};

const COMBO: u32 = 1;
const GROUP: u32 = 2;
/// Open dialog: item 0 = Auto-Detect, then each encoding at index + 1.
const AUTO: u32 = 0;

pub struct Picked {
    pub path: String,
    /// None = auto-detect (open only).
    pub encoding: Option<Encoding>,
}

pub fn open(owner: *mut core::ffi::c_void, current: Option<&str>) -> Option<Picked> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = dlg.GetOptions().ok()?;
        dlg.SetOptions(opts | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST | FOS_FORCEFILESYSTEM).ok()?;
        let base: IFileDialog = dlg.cast().ok()?;
        prepare(&base, current, false)?;
        let cust: IFileDialogCustomize = dlg.cast().ok()?;
        cust.StartVisualGroup(GROUP, &HSTRING::from("&Encoding:")).ok()?;
        cust.AddComboBox(COMBO).ok()?;
        cust.AddControlItem(COMBO, AUTO, &HSTRING::from("Auto-Detect")).ok()?;
        for e in Encoding::ALL {
            cust.AddControlItem(COMBO, e.to_index() + 1, &HSTRING::from(e.label())).ok()?;
        }
        cust.SetSelectedControlItem(COMBO, AUTO).ok()?;
        cust.EndVisualGroup().ok()?;
        dlg.Show(Some(HWND(owner))).ok()?;
        let path = result_path(&dlg.GetResult().ok()?)?;
        let sel = cust.GetSelectedControlItem(COMBO).unwrap_or(AUTO);
        let encoding = if sel == AUTO { None } else { Encoding::from_index(sel - 1) };
        Some(Picked { path, encoding })
    }
}

pub fn save_as(owner: *mut core::ffi::c_void, current: Option<&str>, enc: Encoding) -> Option<Picked> {
    unsafe {
        let dlg: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = dlg.GetOptions().ok()?;
        dlg.SetOptions(opts | FOS_OVERWRITEPROMPT | FOS_PATHMUSTEXIST | FOS_FORCEFILESYSTEM).ok()?;
        let base: IFileDialog = dlg.cast().ok()?;
        prepare(&base, current, true)?;
        let cust: IFileDialogCustomize = dlg.cast().ok()?;
        cust.StartVisualGroup(GROUP, &HSTRING::from("&Encoding:")).ok()?;
        cust.AddComboBox(COMBO).ok()?;
        for e in Encoding::ALL {
            cust.AddControlItem(COMBO, e.to_index(), &HSTRING::from(e.label())).ok()?;
        }
        cust.SetSelectedControlItem(COMBO, enc.to_index()).ok()?;
        cust.EndVisualGroup().ok()?;
        dlg.Show(Some(HWND(owner))).ok()?;
        let path = result_path(&dlg.GetResult().ok()?)?;
        let sel = cust.GetSelectedControlItem(COMBO).ok().and_then(Encoding::from_index).unwrap_or(enc);
        Some(Picked { path, encoding: Some(sel) })
    }
}

/// Filters "Text Documents (*.txt)" / "All Files (*.*)", default extension .txt,
/// starting in the current file's folder.
unsafe fn prepare(dlg: &IFileDialog, current: Option<&str>, saving: bool) -> Option<()> {
    let txt_name = HSTRING::from("Text Documents (*.txt)");
    let txt_spec = HSTRING::from("*.txt");
    let all_name = HSTRING::from("All Files (*.*)");
    let all_spec = HSTRING::from("*.*");
    let specs = [
        COMDLG_FILTERSPEC { pszName: PCWSTR(txt_name.as_ptr()), pszSpec: PCWSTR(txt_spec.as_ptr()) },
        COMDLG_FILTERSPEC { pszName: PCWSTR(all_name.as_ptr()), pszSpec: PCWSTR(all_spec.as_ptr()) },
    ];
    dlg.SetFileTypes(&specs).ok()?;
    dlg.SetFileTypeIndex(1).ok()?;
    dlg.SetDefaultExtension(&HSTRING::from("txt")).ok()?;
    match current {
        Some(path) => {
            let p = std::path::Path::new(path);
            if let Some(dir) = p.parent().filter(|d| d.is_dir()) {
                if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None) {
                    let _ = dlg.SetFolder(&item);
                }
            }
            if let Some(name) = p.file_name() {
                let _ = dlg.SetFileName(&HSTRING::from(name));
            }
        }
        None if saving => {
            let _ = dlg.SetFileName(&HSTRING::from("*.txt"));
        }
        None => {}
    }
    Some(())
}

unsafe fn result_path(item: &IShellItem) -> Option<String> {
    let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
    let s = p.to_string().ok();
    CoTaskMemFree(Some(p.0 as *const _));
    s
}
