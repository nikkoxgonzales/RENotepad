//! Persistent settings. Same value names Windows 10 Notepad keeps under
//! HKCU\Software\Microsoft\Notepad, but in our own key so the real Notepad
//! is never touched.

use crate::wide;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE,
    REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
};

const KEY: &str = r"Software\RENotepad";

pub struct Settings {
    pub face: String,
    /// Tenths of a point, like ChooseFont's iPointSize.
    pub point_size: i32,
    pub weight: i32,
    pub italic: bool,
    pub charset: u8,
    pub wrap: bool,
    pub status_bar: bool,
    pub dark: bool,
    /// Ask GitHub for a newer release at startup.
    pub check_updates: bool,
    pub match_case: bool,
    pub wrap_around: bool,
    pub search_up: bool,
    pub search: String,
    pub replace: String,
    pub window: Option<(i32, i32, i32, i32)>,
    pub maximized: bool,
    /// Page margins in thousandths of an inch: left, top, right, bottom.
    pub margins: [i32; 4],
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            face: "Consolas".into(),
            point_size: 110,
            weight: 400,
            italic: false,
            charset: 1, // DEFAULT_CHARSET
            wrap: false,
            status_bar: true,
            dark: true,
            check_updates: true,
            match_case: false,
            wrap_around: false,
            search_up: false,
            search: String::new(),
            replace: String::new(),
            window: None,
            maximized: false,
            margins: [750, 1000, 750, 1000],
        }
    }
}

struct Key(HKEY);

impl Key {
    fn open(write: bool) -> Option<Key> {
        let mut hkey: HKEY = std::ptr::null_mut();
        let access = if write { KEY_READ | KEY_WRITE } else { KEY_READ };
        let path = wide(KEY);
        // Reads only open the key; it is created on the first save.
        let rc = unsafe {
            if write {
                RegCreateKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, std::ptr::null(), REG_OPTION_NON_VOLATILE, access, std::ptr::null(), &mut hkey, std::ptr::null_mut())
            } else {
                RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut hkey)
            }
        };
        (rc == 0).then_some(Key(hkey))
    }

    fn dword(&self, name: &str) -> Option<i32> {
        let mut v: u32 = 0;
        let mut size = 4u32;
        let mut ty = 0u32;
        let n = wide(name);
        let rc = unsafe { RegQueryValueExW(self.0, n.as_ptr(), std::ptr::null(), &mut ty, (&mut v as *mut u32).cast(), &mut size) };
        (rc == 0 && ty == REG_DWORD).then_some(v as i32)
    }

    fn string(&self, name: &str) -> Option<String> {
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let mut ty = 0u32;
        let n = wide(name);
        let rc = unsafe { RegQueryValueExW(self.0, n.as_ptr(), std::ptr::null(), &mut ty, buf.as_mut_ptr().cast(), &mut size) };
        if rc != 0 || ty != REG_SZ {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    fn set_dword(&self, name: &str, v: i32) {
        let n = wide(name);
        unsafe { RegSetValueExW(self.0, n.as_ptr(), 0, REG_DWORD, (&v as *const i32).cast(), 4) };
    }

    fn set_string(&self, name: &str, v: &str) {
        let n = wide(name);
        let data = wide(v);
        unsafe { RegSetValueExW(self.0, n.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32) };
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

impl Settings {
    pub fn load() -> Settings {
        let mut s = Settings::default();
        let Some(k) = Key::open(false) else { return s };
        let flag = |name: &str, def: bool| k.dword(name).map_or(def, |v| v != 0);
        if let Some(f) = k.string("lfFaceName").filter(|f| !f.is_empty()) {
            s.face = f;
        }
        s.point_size = k.dword("iPointSize").filter(|v| (10..=7200).contains(v)).unwrap_or(s.point_size);
        s.weight = k.dword("lfWeight").unwrap_or(s.weight);
        s.italic = flag("lfItalic", s.italic);
        s.charset = k.dword("lfCharSet").map_or(s.charset, |v| v as u8);
        s.wrap = flag("fWrap", s.wrap);
        s.status_bar = flag("StatusBar", s.status_bar);
        s.dark = flag("fDarkMode", s.dark);
        s.check_updates = flag("fCheckUpdates", s.check_updates);
        s.match_case = flag("fMatchCase", s.match_case);
        s.wrap_around = flag("fWrapAround", s.wrap_around);
        s.search_up = flag("fReverse", s.search_up);
        s.search = k.string("searchString").unwrap_or_default();
        s.replace = k.string("replaceString").unwrap_or_default();
        if let (Some(x), Some(y), Some(dx), Some(dy)) =
            (k.dword("iWindowPosX"), k.dword("iWindowPosY"), k.dword("iWindowPosDX"), k.dword("iWindowPosDY"))
        {
            if dx > 100 && dy > 100 {
                s.window = Some((x, y, dx, dy));
            }
        }
        s.maximized = flag("fMaximized", false);
        for (i, name) in ["iMarginLeft", "iMarginTop", "iMarginRight", "iMarginBottom"].iter().enumerate() {
            if let Some(v) = k.dword(name) {
                s.margins[i] = v;
            }
        }
        s
    }

    pub fn save(&self) {
        let Some(k) = Key::open(true) else { return };
        k.set_string("lfFaceName", &self.face);
        k.set_dword("iPointSize", self.point_size);
        k.set_dword("lfWeight", self.weight);
        k.set_dword("lfItalic", self.italic as i32);
        k.set_dword("lfCharSet", self.charset as i32);
        k.set_dword("fWrap", self.wrap as i32);
        k.set_dword("StatusBar", self.status_bar as i32);
        k.set_dword("fDarkMode", self.dark as i32);
        k.set_dword("fCheckUpdates", self.check_updates as i32);
        k.set_dword("fMatchCase", self.match_case as i32);
        k.set_dword("fWrapAround", self.wrap_around as i32);
        k.set_dword("fReverse", self.search_up as i32);
        k.set_string("searchString", &self.search);
        k.set_string("replaceString", &self.replace);
        if let Some((x, y, dx, dy)) = self.window {
            k.set_dword("iWindowPosX", x);
            k.set_dword("iWindowPosY", y);
            k.set_dword("iWindowPosDX", dx);
            k.set_dword("iWindowPosDY", dy);
        }
        k.set_dword("fMaximized", self.maximized as i32);
        for (i, name) in ["iMarginLeft", "iMarginTop", "iMarginRight", "iMarginBottom"].iter().enumerate() {
            k.set_dword(name, self.margins[i]);
        }
    }
}
