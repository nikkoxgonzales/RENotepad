//! Folder pane: a tree of one folder at the left of the window.
//!
//! A native SysTreeView32. Folders are listed on worker threads, and a watcher
//! thread reports the folders that changed. Both post their results to the main
//! window, so only the UI thread touches the tree. Actions that come out of a
//! tree notification (open, rename, drag...) are handed back to the app as an
//! [`Action`], which the app posts to itself and runs after the notification:
//! opening a tab or showing a dialog inside the tree's own notification is not safe.

use crate::{theme, wide};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::{DeleteObject, ScreenToClient, HFONT};
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, GetOverlappedResultEx, OVERLAPPED};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// LPARAM = Box<Listing>.
pub const WM_FOLDER_LISTED: u32 = WM_APP + 4;
/// LPARAM = Box<Changes>.
pub const WM_FOLDER_CHANGED: u32 = WM_APP + 5;

/// A folder shows this many items at most, then one "N of M items shown" row.
pub const MAX_ITEMS: usize = 2000;

// Context menu commands. The app sends each id in CMD_FIRST..=CMD_LAST to its tree_command.
pub const CMD_OPEN: u16 = 200;
pub const CMD_NEW_WINDOW: u16 = 201;
pub const CMD_REVEAL: u16 = 202;
pub const CMD_COPY_PATH: u16 = 203;
pub const CMD_NEW_FILE: u16 = 204;
pub const CMD_NEW_FOLDER: u16 = 205;
pub const CMD_RENAME: u16 = 206;
pub const CMD_DELETE: u16 = 207;
pub const CMD_SET_ROOT: u16 = 208;
pub const CMD_REFRESH: u16 = 209;
pub const CMD_COLLAPSE_ALL: u16 = 210;
pub const CMD_SHOW_HIDDEN: u16 = 211;
pub const CMD_CLOSE_FOLDER: u16 = 212;
pub const CMD_TOGGLE: u16 = 213;
pub const CMD_OPEN_FOLDER: u16 = 214;
pub const CMD_FIRST: u16 = CMD_OPEN;
pub const CMD_LAST: u16 = CMD_OPEN_FOLDER;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Root,
    Dir,
    File,
    /// A junction or a symbolic link to a folder: shown, never expanded.
    Link,
    /// "Loading...", "N of M items shown", "Can't open this folder".
    Note,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Load {
    No,
    Busy,
    Done,
}

struct Node {
    item: HTREEITEM,
    path: PathBuf,
    name: String,
    /// NUL-terminated, for StrCmpLogicalW in the sort callback.
    wname: Vec<u16>,
    kind: Kind,
    load: Load,
}

/// One entry of a listed folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub link: bool,
}

impl Entry {
    fn kind(&self) -> Kind {
        match (self.dir, self.link) {
            (true, true) => Kind::Link,
            (true, false) => Kind::Dir,
            _ => Kind::File,
        }
    }
}

pub struct Listing {
    gen: u32,
    path: PathBuf,
    /// The shown entries (at most MAX_ITEMS) and how many there are in all.
    result: Result<(Vec<Entry>, usize), String>,
}

pub struct Changes {
    gen: u32,
    /// The folders whose list changed; None = anything may have changed.
    dirs: Option<Vec<PathBuf>>,
}

/// What a tree notification asks the app to do, after the notification.
pub enum Action {
    Open(PathBuf),
    Command(u16),
    Rename { id: isize, name: String },
    Drag(PathBuf),
    FocusEditor,
}

/// The item a command acts on.
pub struct Target {
    pub path: PathBuf,
    pub kind: Kind,
}

pub struct Explorer {
    pub hwnd: HWND,
    parent: HWND,
    nodes: HashMap<isize, Node>,
    next_id: isize,
    root: Option<PathBuf>,
    root_id: isize,
    /// Bumped on each new root; results of an older root are dropped.
    gen: u32,
    pub show_hidden: bool,
    /// The item that shows in bold: the file of the active tab.
    bold: HTREEITEM,
    /// A file to show once the folders on its way have loaded.
    reveal: Option<PathBuf>,
    /// A new item to rename once its folder has listed it.
    edit_after: Option<PathBuf>,
    watch_stop: Option<Arc<AtomicBool>>,
    /// System image list indexes by icon key ("<dir>", "<open>" or the lowercase extension).
    icons: HashMap<String, i32>,
    font: HFONT,
}

impl Explorer {
    /// No tree yet: the app state exists before its window.
    pub fn empty() -> Explorer {
        Explorer {
            hwnd: std::ptr::null_mut(),
            parent: std::ptr::null_mut(),
            nodes: HashMap::new(),
            next_id: 1,
            root: None,
            root_id: 0,
            gen: 0,
            show_hidden: false,
            bold: 0,
            reveal: None,
            edit_after: None,
            watch_stop: None,
            icons: HashMap::new(),
            font: std::ptr::null_mut(),
        }
    }

    pub fn create(parent: HWND, id: usize, dpi: u32, show_hidden: bool) -> Explorer {
        unsafe {
            let icc = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_TREEVIEW_CLASSES };
            InitCommonControlsEx(&icc);
            let style = WS_CHILD | WS_CLIPSIBLINGS | WS_TABSTOP | TVS_HASBUTTONS | TVS_SHOWSELALWAYS | TVS_EDITLABELS | TVS_FULLROWSELECT | TVS_TRACKSELECT | TVS_NOHSCROLL;
            let hwnd = CreateWindowExW(0, WC_TREEVIEWW, std::ptr::null(), style, 0, 0, 0, 0, parent, id as HMENU, crate::dialogs::hinstance(), std::ptr::null());
            SendMessageW(hwnd, TVM_SETEXTENDEDSTYLE, TVS_EX_DOUBLEBUFFER as usize, TVS_EX_DOUBLEBUFFER as isize);
            // The shell's small system image list: real file type icons, shared, never destroyed by us.
            let mut sfi: SHFILEINFOW = std::mem::zeroed();
            let x = wide("x");
            let list = SHGetFileInfoW(x.as_ptr(), FILE_ATTRIBUTE_NORMAL, &mut sfi, std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_SYSICONINDEX | SHGFI_SMALLICON | SHGFI_USEFILEATTRIBUTES);
            SendMessageW(hwnd, TVM_SETIMAGELIST, TVSIL_NORMAL as usize, list as isize);
            let mut e = Explorer { hwnd, parent, show_hidden, ..Explorer::empty() };
            e.set_dpi(dpi);
            e.apply_theme();
            e
        }
    }

    pub fn set_dpi(&mut self, dpi: u32) {
        unsafe {
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
            self.font = theme::font(theme::ui_face(), 14, dpi);
            SendMessageW(self.hwnd, WM_SETFONT, self.font as usize, 1);
            SendMessageW(self.hwnd, TVM_SETITEMHEIGHT, crate::muldiv(26, dpi as i32, 96) as usize, 0);
        }
    }

    pub fn apply_theme(&self) {
        let p = theme::palette();
        theme::apply_control(self.hwnd, "DarkMode_Explorer");
        unsafe {
            SendMessageW(self.hwnd, TVM_SETBKCOLOR, 0, p.surface as isize);
            SendMessageW(self.hwnd, TVM_SETTEXTCOLOR, 0, p.text as isize);
        }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Show `root` (None = empty tree). Lists it and starts the watcher.
    pub fn set_root(&mut self, root: Option<PathBuf>) {
        if let Some(stop) = self.watch_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        self.gen = self.gen.wrapping_add(1);
        self.reveal = None;
        self.edit_after = None;
        self.bold = 0;
        if self.root_id != 0 {
            self.delete_item(self.root_id);
            self.root_id = 0;
        }
        self.root = root.clone();
        let Some(root) = root else { return };
        let name = root.file_name().map_or_else(|| root.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
        let id = self.insert(TVI_ROOT, Kind::Root, &name, root.clone());
        self.root_id = id;
        self.load(id);
        unsafe { SendMessageW(self.hwnd, TVM_EXPAND, TVE_EXPAND as usize, self.nodes[&id].item) };
        let stop = Arc::new(AtomicBool::new(false));
        self.watch_stop = Some(stop.clone());
        let (hwnd, gen) = (self.parent as usize, self.gen);
        std::thread::spawn(move || watch(hwnd, gen, root, stop));
    }

    // ------------------------------------------------------------ items

    fn icon(&mut self, kind: Kind, name: &str, open: bool) -> i32 {
        let dir = matches!(kind, Kind::Dir | Kind::Link | Kind::Root);
        let key = match (dir, open) {
            (true, true) => "<open>".to_string(),
            (true, false) => "<dir>".to_string(),
            _ => Path::new(name).extension().map_or(String::new(), |e| e.to_string_lossy().to_lowercase()),
        };
        if let Some(&i) = self.icons.get(&key) {
            return i;
        }
        let probe = if dir { wide("folder") } else { wide(&format!("x.{key}")) };
        let attr = if dir { FILE_ATTRIBUTE_DIRECTORY } else { FILE_ATTRIBUTE_NORMAL };
        let mut sfi: SHFILEINFOW = unsafe { std::mem::zeroed() };
        let flags = SHGFI_SYSICONINDEX | SHGFI_SMALLICON | SHGFI_USEFILEATTRIBUTES | if open { SHGFI_OPENICON } else { 0 };
        unsafe { SHGetFileInfoW(probe.as_ptr(), attr, &mut sfi, std::mem::size_of::<SHFILEINFOW>() as u32, flags) };
        self.icons.insert(key, sfi.iIcon);
        sfi.iIcon
    }

    /// Add an item at the end of `parent`'s children. Returns its node id.
    fn insert(&mut self, parent: HTREEITEM, kind: Kind, name: &str, path: PathBuf) -> isize {
        let id = self.next_id;
        self.next_id += 1;
        let (image, open) = if kind == Kind::Note { (-1, -1) } else { (self.icon(kind, name, false), self.icon(kind, name, true)) };
        let mut text = wide(name);
        let mut ins = TVINSERTSTRUCTW { hParent: parent, hInsertAfter: TVI_LAST, ..Default::default() };
        ins.Anonymous.itemex = TVITEMEXW {
            mask: TVIF_TEXT | TVIF_PARAM | TVIF_IMAGE | TVIF_SELECTEDIMAGE | TVIF_EXPANDEDIMAGE | TVIF_CHILDREN,
            pszText: text.as_mut_ptr(),
            iImage: image,
            iSelectedImage: image,
            iExpandedImage: open,
            cChildren: matches!(kind, Kind::Root | Kind::Dir) as i32,
            lParam: id,
            ..Default::default()
        };
        let item = unsafe { SendMessageW(self.hwnd, TVM_INSERTITEMW, 0, &ins as *const _ as isize) };
        self.nodes.insert(id, Node { item, path, wname: wide(name), name: name.to_string(), kind, load: Load::No });
        id
    }

    /// Delete an item and forget its node and the nodes below it. The tree's own
    /// TVN_DELETEITEM is not used: it arrives while we are inside our own call.
    fn delete_item(&mut self, id: isize) {
        let Some(item) = self.nodes.get(&id).map(|n| n.item) else { return };
        let mut stack = vec![id];
        while let Some(i) = stack.pop() {
            if let Some(n) = self.nodes.remove(&i) {
                if n.item == self.bold {
                    self.bold = 0;
                }
                stack.extend(self.children(n.item));
            }
        }
        unsafe { SendMessageW(self.hwnd, TVM_DELETEITEM, 0, item) };
    }

    fn id_of(&self, item: HTREEITEM) -> Option<isize> {
        if item == 0 {
            return None;
        }
        let mut it = TVITEMW { mask: TVIF_PARAM | TVIF_HANDLE, hItem: item, ..Default::default() };
        let ok = unsafe { SendMessageW(self.hwnd, TVM_GETITEMW, 0, &mut it as *mut _ as isize) } != 0;
        (ok && self.nodes.contains_key(&it.lParam)).then_some(it.lParam)
    }

    /// Node ids of the children of `item`, in tree order.
    fn children(&self, item: HTREEITEM) -> Vec<isize> {
        let mut out = Vec::new();
        let mut c = unsafe { SendMessageW(self.hwnd, TVM_GETNEXTITEM, TVGN_CHILD as usize, item) };
        while c != 0 {
            if let Some(id) = self.id_of(c) {
                out.push(id);
            }
            c = unsafe { SendMessageW(self.hwnd, TVM_GETNEXTITEM, TVGN_NEXT as usize, c) };
        }
        out
    }

    fn child_named(&self, id: isize, name: &str) -> Option<isize> {
        self.children(self.nodes.get(&id)?.item).into_iter().find(|c| self.nodes[c].kind != Kind::Note && self.nodes[c].name.eq_ignore_ascii_case(name))
    }

    /// The node of `path`, if the tree shows it.
    fn find(&self, path: &Path) -> Option<isize> {
        let comps = rel_components(self.root.as_deref()?, path)?;
        let mut cur = self.root_id;
        for c in comps {
            cur = self.child_named(cur, &c)?;
        }
        Some(cur)
    }

    fn set_children_flag(&self, item: HTREEITEM, has: bool) {
        let it = TVITEMW { mask: TVIF_CHILDREN | TVIF_HANDLE, hItem: item, cChildren: has as i32, ..Default::default() };
        unsafe { SendMessageW(self.hwnd, TVM_SETITEMW, 0, &it as *const _ as isize) };
    }

    fn sort_children(&mut self, item: HTREEITEM) {
        unsafe extern "system" fn compare(a: LPARAM, b: LPARAM, nodes: LPARAM) -> i32 {
            let nodes = &*(nodes as *const HashMap<isize, Node>);
            match (nodes.get(&a), nodes.get(&b)) {
                (Some(a), Some(b)) => rank(a.kind).cmp(&rank(b.kind)).then_with(|| StrCmpLogicalW(a.wname.as_ptr(), b.wname.as_ptr()).cmp(&0)) as i32,
                _ => 0,
            }
        }
        let cb = TVSORTCB { hParent: item, lpfnCompare: Some(compare), lParam: &self.nodes as *const _ as isize };
        unsafe { SendMessageW(self.hwnd, TVM_SORTCHILDRENCB, 0, &cb as *const _ as isize) };
    }

    // ------------------------------------------------------------ listing

    /// First listing of a folder: a "Loading..." child until the worker answers.
    fn load(&mut self, id: isize) {
        let Some(n) = self.nodes.get_mut(&id) else { return };
        if n.load != Load::No || !matches!(n.kind, Kind::Root | Kind::Dir) {
            return;
        }
        n.load = Load::Busy;
        let item = n.item;
        self.insert(item, Kind::Note, "Loading...", PathBuf::new());
        self.list(id);
    }

    fn list(&self, id: isize) {
        let Some(n) = self.nodes.get(&id) else { return };
        let (hwnd, gen, path, hidden) = (self.parent as usize, self.gen, n.path.clone(), self.show_hidden);
        std::thread::spawn(move || {
            let result = list_dir(&path, hidden);
            post(hwnd, WM_FOLDER_LISTED, Box::new(Listing { gen, path, result }));
        });
    }

    /// A worker finished listing a folder.
    pub fn listed(&mut self, l: Listing) {
        if l.gen != self.gen {
            return;
        }
        let Some(id) = self.find(&l.path) else { return };
        let item = self.nodes[&id].item;
        match l.result {
            Ok((entries, total)) => self.merge(id, &entries, total),
            Err(_) => {
                for c in self.children(item) {
                    self.delete_item(c);
                }
                self.insert(item, Kind::Note, "Can't open this folder", PathBuf::new());
            }
        }
        if let Some(n) = self.nodes.get_mut(&id) {
            n.load = Load::Done;
        }
        let has = !self.children(item).is_empty();
        self.set_children_flag(item, has);
        self.continue_reveal();
        if let Some(target) = self.edit_after.clone() {
            if let Some(new) = self.find(&target) {
                self.edit_after = None;
                let item = self.nodes[&new].item;
                unsafe {
                    SendMessageW(self.hwnd, TVM_SELECTITEM, TVGN_CARET as usize, item);
                    windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(self.hwnd);
                    // Posted: TVM_EDITLABEL sends TVN_BEGINLABELEDIT back to the app at once.
                    PostMessageW(self.hwnd, TVM_EDITLABELW, 0, item);
                }
            }
        }
    }

    /// Bring a folder's children in line with a fresh list. Items that stay keep
    /// their handle, so the selection and the expanded folders stay too.
    fn merge(&mut self, id: isize, entries: &[Entry], total: usize) {
        let (item, dir) = (self.nodes[&id].item, self.nodes[&id].path.clone());
        let wanted: HashSet<(&str, Kind)> = entries.iter().map(|e| (e.name.as_str(), e.kind())).collect();
        let mut kept = HashSet::new();
        for c in self.children(item) {
            let n = &self.nodes[&c];
            if n.kind != Kind::Note && wanted.contains(&(n.name.as_str(), n.kind)) {
                kept.insert(n.name.clone());
            } else {
                self.delete_item(c);
            }
        }
        for e in entries {
            if !kept.contains(&e.name) {
                self.insert(item, e.kind(), &e.name, dir.join(&e.name));
            }
        }
        if total > entries.len() {
            self.insert(item, Kind::Note, &format!("{} of {} items shown", crate::app::group(entries.len()), crate::app::group(total)), PathBuf::new());
        }
        self.sort_children(item);
    }

    /// List again every folder that has listed once.
    pub fn refresh_all(&mut self) {
        let ids: Vec<isize> = self.nodes.iter().filter(|(_, n)| n.load == Load::Done).map(|(&i, _)| i).collect();
        for id in ids {
            self.list(id);
        }
    }

    /// The watcher saw changes.
    pub fn changed(&mut self, c: Changes) {
        if c.gen != self.gen {
            return;
        }
        let Some(dirs) = c.dirs else { return self.refresh_all() };
        for d in dirs {
            if let Some(id) = self.find(&d).filter(|i| self.nodes[i].load == Load::Done) {
                self.list(id);
            }
        }
    }

    // ------------------------------------------------------------ follow the tab

    /// Show `path` in bold and scroll to it, expanding (and listing) its folders.
    pub fn reveal(&mut self, path: Option<&Path>) {
        self.set_bold(0);
        self.reveal = path.map(Path::to_path_buf);
        self.continue_reveal();
    }

    fn continue_reveal(&mut self) {
        let Some(target) = self.reveal.clone() else { return };
        let Some(comps) = self.root.as_deref().and_then(|r| rel_components(r, &target)) else {
            self.reveal = None;
            return;
        };
        let mut cur = self.root_id;
        for c in &comps {
            let n = &self.nodes[&cur];
            if n.load != Load::Done {
                if n.load == Load::No {
                    self.load(cur);
                }
                return; // wait for the listing
            }
            match self.child_named(cur, c) {
                Some(next) => cur = next,
                None => {
                    self.reveal = None; // hidden, past the cap, or gone
                    return;
                }
            }
        }
        self.reveal = None;
        // Expand the folders on the way (TVM_EXPAND sends no notification).
        let mut p = unsafe { SendMessageW(self.hwnd, TVM_GETNEXTITEM, TVGN_PARENT as usize, self.nodes[&cur].item) };
        while p != 0 {
            unsafe { SendMessageW(self.hwnd, TVM_EXPAND, TVE_EXPAND as usize, p) };
            p = unsafe { SendMessageW(self.hwnd, TVM_GETNEXTITEM, TVGN_PARENT as usize, p) };
        }
        let item = self.nodes[&cur].item;
        self.set_bold(item);
        unsafe { SendMessageW(self.hwnd, TVM_SELECTITEM, TVGN_CARET as usize, item) };
    }

    fn set_bold(&mut self, item: HTREEITEM) {
        let set = |item: HTREEITEM, on: bool| {
            let it = TVITEMW { mask: TVIF_STATE | TVIF_HANDLE, hItem: item, state: if on { TVIS_BOLD } else { 0 }, stateMask: TVIS_BOLD, ..Default::default() };
            unsafe { SendMessageW(self.hwnd, TVM_SETITEMW, 0, &it as *const _ as isize) };
        };
        if self.bold != 0 {
            set(self.bold, false);
        }
        self.bold = item;
        if item != 0 {
            set(item, true);
        }
    }

    // ------------------------------------------------------------ notifications

    /// A WM_NOTIFY from the tree: the result for the tree, and an action for the
    /// app to post to itself.
    pub fn notify(&mut self, lparam: LPARAM) -> (LRESULT, Option<Action>) {
        let hdr = unsafe { &*(lparam as *const NMHDR) };
        match hdr.code {
            TVN_ITEMEXPANDINGW => {
                let nm = unsafe { &*(lparam as *const NMTREEVIEWW) };
                if nm.action & TVE_EXPAND != 0 {
                    if let Some(id) = self.id_of(nm.itemNew.hItem) {
                        self.load(id);
                    }
                }
                (0, None)
            }
            NM_DBLCLK => match self.item_at_cursor().and_then(|id| self.nodes.get(&id)) {
                Some(n) if n.kind == Kind::File => (1, Some(Action::Open(n.path.clone()))),
                _ => (0, None),
            },
            TVN_KEYDOWN => {
                let nm = unsafe { &*(lparam as *const NMTVKEYDOWN) };
                use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_DELETE, VK_ESCAPE, VK_F2, VK_RETURN};
                let act = match nm.wVKey {
                    VK_RETURN => Action::Command(CMD_OPEN),
                    VK_F2 => Action::Command(CMD_RENAME),
                    VK_DELETE => Action::Command(CMD_DELETE),
                    VK_ESCAPE => Action::FocusEditor,
                    _ => return (0, None),
                };
                (1, Some(act)) // nonzero: not part of the incremental search
            }
            TVN_BEGINLABELEDITW => {
                let nm = unsafe { &*(lparam as *const NMTVDISPINFOW) };
                let Some(n) = self.id_of(nm.item.hItem).and_then(|id| self.nodes.get(&id)) else { return (1, None) };
                if !matches!(n.kind, Kind::Dir | Kind::File | Kind::Link) {
                    return (1, None);
                }
                // Select the name without its extension, like Explorer. Posted: the
                // tree selects all of the text after this notification.
                let stem = if n.kind == Kind::File { Path::new(&n.name).file_stem().map_or(0, |s| s.to_string_lossy().encode_utf16().count()) } else { n.name.encode_utf16().count() };
                unsafe {
                    let edit = SendMessageW(self.hwnd, TVM_GETEDITCONTROL, 0, 0) as HWND;
                    PostMessageW(edit, EM_SETSEL, 0, stem as isize);
                }
                (0, None)
            }
            TVN_ENDLABELEDITW => {
                let nm = unsafe { &*(lparam as *const NMTVDISPINFOW) };
                if nm.item.pszText.is_null() {
                    return (0, None); // cancelled
                }
                let name = unsafe { pwstr(nm.item.pszText) };
                match self.id_of(nm.item.hItem) {
                    Some(id) => (0, Some(Action::Rename { id, name })),
                    None => (0, None),
                }
            }
            TVN_BEGINDRAGW => {
                let nm = unsafe { &*(lparam as *const NMTREEVIEWW) };
                match self.id_of(nm.itemNew.hItem).and_then(|id| self.nodes.get(&id)) {
                    Some(n) if n.kind == Kind::File => (0, Some(Action::Drag(n.path.clone()))),
                    _ => (0, None),
                }
            }
            _ => (0, None),
        }
    }

    fn item_at_cursor(&self) -> Option<isize> {
        let pos = unsafe { GetMessagePos() };
        let mut pt = POINT { x: (pos & 0xFFFF) as i16 as i32, y: ((pos >> 16) & 0xFFFF) as i16 as i32 };
        unsafe { ScreenToClient(self.hwnd, &mut pt) };
        let mut ht = TVHITTESTINFO { pt, ..Default::default() };
        let item = unsafe { SendMessageW(self.hwnd, TVM_HITTEST, 0, &mut ht as *mut _ as isize) };
        if ht.flags & (TVHT_ONITEM | TVHT_ONITEMBUTTON | TVHT_ONITEMINDENT | TVHT_ONITEMRIGHT) == 0 {
            return None;
        }
        self.id_of(item)
    }

    // ------------------------------------------------------------ commands

    fn selected(&self) -> Option<isize> {
        self.id_of(unsafe { SendMessageW(self.hwnd, TVM_GETNEXTITEM, TVGN_CARET as usize, 0) })
    }

    /// What a command acts on: the selection, else the root. A note row acts as the root.
    pub fn target(&self) -> Option<Target> {
        let id = self.selected().filter(|i| self.nodes[i].kind != Kind::Note).unwrap_or(self.root_id);
        self.nodes.get(&id).map(|n| Target { path: n.path.clone(), kind: n.kind })
    }

    /// The right-click menu. `lparam` is WM_CONTEXTMENU's: the screen point, or -1
    /// from the keyboard. Returns the picked command, or 0.
    pub fn context_menu(&mut self, lparam: LPARAM, show_hidden: bool) -> u16 {
        unsafe {
            let (mut x, mut y) = ((lparam & 0xFFFF) as i16 as i32, ((lparam >> 16) & 0xFFFF) as i16 as i32);
            let keyboard = lparam == -1;
            if !keyboard {
                // The item under the pointer becomes the selection; empty space means the root.
                let target = self.item_at_cursor().filter(|i| self.nodes[i].kind != Kind::Note).unwrap_or(self.root_id);
                if let Some(n) = self.nodes.get(&target) {
                    SendMessageW(self.hwnd, TVM_SELECTITEM, TVGN_CARET as usize, n.item);
                }
            }
            let Some(id) = self.selected().filter(|i| self.nodes[i].kind != Kind::Note).or((self.root_id != 0).then_some(self.root_id)) else { return 0 };
            let n = &self.nodes[&id];
            if keyboard {
                let mut r: RECT = std::mem::zeroed();
                // TVM_GETITEMRECT takes the item handle in the RECT it fills.
                *(&mut r as *mut RECT as *mut HTREEITEM) = n.item;
                SendMessageW(self.hwnd, TVM_GETITEMRECT, 1, &mut r as *mut _ as isize);
                let mut pt = POINT { x: r.left, y: r.bottom };
                windows_sys::Win32::Graphics::Gdi::ClientToScreen(self.hwnd, &mut pt);
                (x, y) = (pt.x, pt.y);
            }
            let m = CreatePopupMenu();
            let add = |id: u16, s: &str| {
                let w = wide(s);
                AppendMenuW(m, MF_STRING, id as usize, w.as_ptr());
            };
            let sep = || {
                AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
            };
            match n.kind {
                Kind::File => {
                    add(CMD_OPEN, "&Open\tEnter");
                    add(CMD_NEW_WINDOW, "Open in new &window");
                    sep();
                }
                Kind::Dir | Kind::Link => {
                    let expanded = SendMessageW(self.hwnd, TVM_GETITEMSTATE, n.item as usize, TVIS_EXPANDED as isize) as u32 & TVIS_EXPANDED != 0;
                    if n.kind == Kind::Dir {
                        add(CMD_TOGGLE, if expanded { "&Collapse" } else { "&Expand" });
                    }
                    add(CMD_SET_ROOT, "Set as &root");
                    sep();
                }
                _ => {
                    add(CMD_OPEN_FOLDER, "Open &folder...");
                    add(CMD_REFRESH, "R&efresh\tF5");
                    add(CMD_COLLAPSE_ALL, "Co&llapse all");
                    add(CMD_SHOW_HIDDEN, "Show &hidden items");
                    if show_hidden {
                        CheckMenuItem(m, CMD_SHOW_HIDDEN as u32, MF_BYCOMMAND | MF_CHECKED);
                    }
                    add(CMD_CLOSE_FOLDER, "&Close folder");
                    sep();
                }
            }
            add(CMD_REVEAL, "Reveal in File E&xplorer");
            add(CMD_COPY_PATH, "Copy &path");
            sep();
            add(CMD_NEW_FILE, "&New file");
            add(CMD_NEW_FOLDER, "New fol&der");
            if n.kind != Kind::Root {
                add(CMD_RENAME, "Rena&me\tF2");
                add(CMD_DELETE, "&Delete\tDel");
            }
            let picked = TrackPopupMenuEx(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, self.parent, std::ptr::null());
            DestroyMenu(m);
            picked as u16
        }
    }

    /// Expand or collapse the selected folder.
    pub fn toggle(&mut self) {
        let Some(id) = self.selected().filter(|i| self.nodes[i].kind == Kind::Dir) else { return };
        self.load(id);
        unsafe { SendMessageW(self.hwnd, TVM_EXPAND, TVE_TOGGLE as usize, self.nodes[&id].item) };
    }

    pub fn collapse_all(&self) {
        let mut stack = self.children(self.nodes.get(&self.root_id).map_or(0, |n| n.item));
        while let Some(id) = stack.pop() {
            let item = self.nodes[&id].item;
            stack.extend(self.children(item));
            unsafe { SendMessageW(self.hwnd, TVM_EXPAND, TVE_COLLAPSE as usize, item) };
        }
    }

    pub fn start_rename(&self) {
        if let Some(id) = self.selected().filter(|i| matches!(self.nodes[i].kind, Kind::Dir | Kind::File | Kind::Link)) {
            unsafe {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(self.hwnd);
                PostMessageW(self.hwnd, TVM_EDITLABELW, 0, self.nodes[&id].item);
            }
        }
    }

    /// Make a new file or folder in `dir` and start its rename once the tree lists it.
    pub fn create_new(&mut self, dir: &Path, folder: bool) -> Result<PathBuf, String> {
        let path = if folder { unique_name(dir, "New folder", "") } else { unique_name(dir, "New Text Document", ".txt") };
        let made = if folder { std::fs::create_dir(&path) } else { std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map(|_| ()) };
        made.map_err(|e| format!("{}\n{e}", path.display()))?;
        self.edit_after = Some(path.clone());
        if let Some(id) = self.find(dir) {
            if self.nodes[&id].load == Load::No {
                self.load(id);
            } else {
                self.list(id);
            }
            unsafe { SendMessageW(self.hwnd, TVM_EXPAND, TVE_EXPAND as usize, self.nodes[&id].item) };
        }
        Ok(path)
    }

    /// Rename an item after its label edit. Ok(Some((old, new))) when it moved.
    pub fn rename(&mut self, id: isize, typed: &str) -> Result<Option<(PathBuf, PathBuf)>, String> {
        let Some(n) = self.nodes.get(&id) else { return Ok(None) };
        let Some(name) = valid_name(typed) else {
            return Err("A file name can't be empty, can't be a reserved name like CON or NUL, and can't contain any of these characters:\n\\ / : * ? \" < > |".into());
        };
        if name == n.name {
            return Ok(None);
        }
        let old = n.path.clone();
        let new = old.with_file_name(&name);
        let (wo, wn) = (wide(&old.to_string_lossy()), wide(&new.to_string_lossy()));
        // No MOVEFILE_REPLACE_EXISTING: never overwrite a file of that name.
        if unsafe { MoveFileExW(wo.as_ptr(), wn.as_ptr(), 0) } == 0 {
            return Err(format!("{}\n{}", new.display(), std::io::Error::last_os_error()));
        }
        let kind = n.kind;
        let item = n.item;
        let image = self.icon(kind, &name, false);
        let n = self.nodes.get_mut(&id).expect("node checked above");
        n.name = name.clone();
        n.wname = wide(&name);
        n.path = new.clone();
        let mut text = wide(&name);
        let it = TVITEMW { mask: TVIF_TEXT | TVIF_IMAGE | TVIF_SELECTEDIMAGE | TVIF_HANDLE, hItem: item, pszText: text.as_mut_ptr(), iImage: image, iSelectedImage: image, ..Default::default() };
        unsafe { SendMessageW(self.hwnd, TVM_SETITEMW, 0, &it as *const _ as isize) };
        if kind == Kind::Dir {
            // The paths below changed: drop the children and list them again on the next expand.
            unsafe { SendMessageW(self.hwnd, TVM_EXPAND, TVE_COLLAPSE as usize, item) };
            for c in self.children(item) {
                self.delete_item(c);
            }
            self.nodes.get_mut(&id).expect("node checked above").load = Load::No;
            self.set_children_flag(item, true);
        }
        let parent = unsafe { SendMessageW(self.hwnd, TVM_GETNEXTITEM, TVGN_PARENT as usize, item) };
        self.sort_children(parent);
        unsafe { SendMessageW(self.hwnd, TVM_ENSUREVISIBLE, 0, item) };
        Ok(Some((old, new)))
    }

    /// Remove the item of a deleted path at once (the watcher may be slow or off).
    pub fn forget(&mut self, path: &Path) {
        if let Some(id) = self.find(path).filter(|&i| i != self.root_id) {
            let parent = unsafe { SendMessageW(self.hwnd, TVM_GETNEXTITEM, TVGN_PARENT as usize, self.nodes[&id].item) };
            self.delete_item(id);
            let has = !self.children(parent).is_empty();
            self.set_children_flag(parent, has);
        }
    }
}

fn rank(k: Kind) -> u8 {
    match k {
        Kind::Root | Kind::Dir | Kind::Link => 0,
        Kind::File => 1,
        Kind::Note => 2,
    }
}

unsafe fn pwstr(p: *const u16) -> String {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
}

/// Post a boxed result to the main window; free it if the window is gone.
fn post<T>(hwnd: usize, msg: u32, b: Box<T>) {
    let p = Box::into_raw(b);
    if unsafe { PostMessageW(hwnd as HWND, msg, 0, p as isize) } == 0 {
        drop(unsafe { Box::from_raw(p) });
    }
}

// ---------------------------------------------------------------- pure rules

/// Listed or not: hidden and system items and `.git` show only with "Show hidden items".
pub fn visible(name: &str, attributes: u32, show_hidden: bool) -> bool {
    show_hidden || (attributes & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) == 0 && !name.eq_ignore_ascii_case(".git"))
}

/// Explorer order: folders first, then files, each by StrCmpLogicalW ("file2" before "file10").
pub fn sort_entries(v: &mut [Entry]) {
    v.sort_by_cached_key(|e| (!e.dir, LogicalKey(wide(&e.name))));
}

#[derive(PartialEq, Eq)]
struct LogicalKey(Vec<u16>);

impl PartialOrd for LogicalKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LogicalKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        unsafe { StrCmpLogicalW(self.0.as_ptr(), other.0.as_ptr()) }.cmp(&0)
    }
}

/// List a folder: filtered, sorted, cut to MAX_ITEMS. Also returns the count before the cut.
pub fn list_dir(path: &Path, show_hidden: bool) -> Result<(Vec<Entry>, usize), String> {
    use std::os::windows::fs::MetadataExt;
    let mut v = Vec::new();
    for e in std::fs::read_dir(path).map_err(|e| e.to_string())?.flatten() {
        // From the directory listing itself: no extra disk access per entry.
        let Ok(md) = e.metadata() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        let attrs = md.file_attributes();
        if !visible(&name, attrs, show_hidden) {
            continue;
        }
        // is_symlink: symbolic links and junctions, not OneDrive's cloud folders.
        v.push(Entry { name, dir: attrs & FILE_ATTRIBUTE_DIRECTORY != 0, link: md.file_type().is_symlink() });
    }
    sort_entries(&mut v);
    let total = v.len();
    v.truncate(MAX_ITEMS);
    Ok((v, total))
}

/// The names from `root` down to `path`, case-insensitively; None when `path` is outside.
pub fn rel_components(root: &Path, path: &Path) -> Option<Vec<String>> {
    let r = root.to_string_lossy();
    let r = r.trim_end_matches('\\');
    let p = path.to_string_lossy();
    let head = p.get(..r.len())?;
    if !head.eq_ignore_ascii_case(r) {
        return None;
    }
    let rest = &p[r.len()..];
    if rest.is_empty() {
        return Some(Vec::new());
    }
    let rest = rest.strip_prefix('\\')?;
    Some(rest.split('\\').filter(|s| !s.is_empty()).map(str::to_string).collect())
}

/// A typed name made valid like Explorer makes it (outer spaces and trailing dots
/// dropped), or None when Windows would refuse it.
pub fn valid_name(typed: &str) -> Option<String> {
    let name = typed.trim_matches(' ').trim_end_matches('.').trim_end_matches(' ');
    if name.is_empty() || name.chars().any(|c| c < ' ' || "\\/:*?\"<>|".contains(c)) {
        return None;
    }
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    const RESERVED: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
    let numbered = (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit() && stem.as_bytes()[3] != b'0';
    if RESERVED.contains(&stem.as_str()) || numbered {
        return None;
    }
    Some(name.to_string())
}

/// `dir\base ext`, or `dir\base (2) ext`, `(3)`... for the first name not taken.
pub fn unique_name(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{base}{ext}"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{base} ({n}){ext}"))).find(|p| !p.exists()).expect("some number is free")
}

// ---------------------------------------------------------------- shell actions

/// Open File Explorer at the item's folder, with the item selected.
pub fn reveal_in_explorer(path: &Path) {
    let w = wide(&path.to_string_lossy());
    unsafe {
        let pidl = ILCreateFromPathW(w.as_ptr());
        if !pidl.is_null() {
            SHOpenFolderAndSelectItems(pidl, 0, std::ptr::null(), 0);
            ILFree(pidl);
        }
    }
}

pub fn copy_text(owner: HWND, s: &str) {
    let w = wide(s);
    unsafe {
        if OpenClipboard(owner) == 0 {
            return;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2);
        if !h.is_null() {
            let p = GlobalLock(h) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
                GlobalUnlock(h);
                if SetClipboardData(13 /* CF_UNICODETEXT */, h).is_null() {
                    GlobalFree(h);
                }
            } else {
                GlobalFree(h);
            }
        }
        CloseClipboard();
    }
}

/// Move a file or folder to the Recycle Bin, with the shell's own confirmation.
/// true = it is gone.
pub fn recycle(owner: HWND, path: &Path) -> bool {
    let mut from = wide(&path.to_string_lossy());
    from.push(0); // a list of names, ended by an empty one
    let mut op = SHFILEOPSTRUCTW { hwnd: owner, wFunc: FO_DELETE, pFrom: from.as_ptr(), fFlags: (FOF_ALLOWUNDO | FOF_WANTNUKEWARNING) as u16, ..Default::default() };
    unsafe { SHFileOperationW(&mut op) == 0 && op.fAnyOperationsAborted == 0 && !path.exists() }
}

// ---------------------------------------------------------------- watcher

/// Watch `root` and its subfolders until `stop`. Posts the changed folders after
/// 200 ms with no new change, or after 1 s of steady changes.
fn watch(hwnd: usize, gen: u32, root: PathBuf, stop: Arc<AtomicBool>) {
    unsafe {
        let w = wide(&root.to_string_lossy());
        let h = CreateFileW(w.as_ptr(), FILE_LIST_DIRECTORY, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, std::ptr::null(), OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED, std::ptr::null_mut());
        if h == INVALID_HANDLE_VALUE {
            return; // no live updates; Refresh still works
        }
        let mut buf = vec![0u32; 16 * 1024]; // 64 KB, DWORD-aligned as the API needs
        let mut ov: OVERLAPPED = std::mem::zeroed();
        let mut dirs: Vec<PathBuf> = Vec::new();
        let (mut all, mut reading) = (false, false);
        let mut first_change: Option<Instant> = None;
        let filter = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_ATTRIBUTES;
        while !stop.load(Ordering::Relaxed) {
            if !reading {
                if ReadDirectoryChangesW(h, buf.as_mut_ptr().cast(), (buf.len() * 4) as u32, 1, filter, std::ptr::null_mut(), &mut ov, None) == 0 {
                    break;
                }
                reading = true;
            }
            let dirty = all || !dirs.is_empty();
            let overdue = first_change.is_some_and(|t| t.elapsed() > Duration::from_secs(1));
            let mut n = 0u32;
            let got = !overdue && GetOverlappedResultEx(h, &ov, &mut n, if dirty { 200 } else { 500 }, 0) != 0;
            if got {
                reading = false;
                if n == 0 {
                    all = true; // the buffer overflowed: list everything again
                } else {
                    for rel in notify_names(&buf, n as usize) {
                        let dir = root.join(&rel).parent().map_or(root.clone(), Path::to_path_buf);
                        if !dirs.contains(&dir) {
                            dirs.push(dir);
                        }
                    }
                }
                first_change.get_or_insert_with(Instant::now);
                continue;
            }
            if !overdue && GetLastError() != WAIT_TIMEOUT {
                break;
            }
            if all || !dirs.is_empty() {
                let changes = Changes { gen, dirs: if all { None } else { Some(std::mem::take(&mut dirs)) } };
                post(hwnd, WM_FOLDER_CHANGED, Box::new(changes));
                dirs.clear();
                all = false;
                first_change = None;
            }
        }
        if reading {
            CancelIoEx(h, &ov);
            let mut n = 0u32;
            GetOverlappedResult(h, &ov, &mut n, 1); // the buffer must outlive the read
        }
        CloseHandle(h);
    }
}

/// The relative names in a FILE_NOTIFY_INFORMATION chain of `len` bytes.
fn notify_names(buf: &[u32], len: usize) -> Vec<String> {
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, len.min(buf.len() * 4)) };
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 12 <= bytes.len() {
        let u32_at = |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]) as usize;
        let (next, name_len) = (u32_at(at), u32_at(at + 8));
        let start = at + 12;
        if start + name_len > bytes.len() {
            break;
        }
        let name: Vec<u16> = bytes[start..start + name_len].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        out.push(String::from_utf16_lossy(&name));
        if next == 0 {
            break;
        }
        at += next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, dir: bool) -> Entry {
        Entry { name: name.into(), dir, link: false }
    }

    #[test]
    fn folders_come_first_then_explorer_number_order() {
        let mut v = vec![e("file10.txt", false), e("b", true), e("File2.txt", false), e("A", true), e("file1.txt", false)];
        sort_entries(&mut v);
        let names: Vec<&str> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["A", "b", "file1.txt", "File2.txt", "file10.txt"]);
    }

    #[test]
    fn hidden_system_and_git_show_only_when_asked() {
        assert!(visible("notes.txt", FILE_ATTRIBUTE_NORMAL, false));
        assert!(!visible("secret", FILE_ATTRIBUTE_HIDDEN, false));
        assert!(!visible("desktop.ini", FILE_ATTRIBUTE_SYSTEM | FILE_ATTRIBUTE_HIDDEN, false));
        assert!(!visible(".GIT", FILE_ATTRIBUTE_DIRECTORY, false));
        assert!(visible(".git", FILE_ATTRIBUTE_DIRECTORY, true));
        assert!(visible("secret", FILE_ATTRIBUTE_HIDDEN, true));
    }

    #[test]
    fn rel_components_match_case_insensitively_and_refuse_outsiders() {
        let root = Path::new(r"C:\Notes");
        assert_eq!(rel_components(root, Path::new(r"c:\notes\a\B.txt")), Some(vec!["a".to_string(), "B.txt".to_string()]));
        assert_eq!(rel_components(root, Path::new(r"C:\Notes")), Some(vec![]));
        assert_eq!(rel_components(root, Path::new(r"C:\NotesOld\x.txt")), None);
        assert_eq!(rel_components(root, Path::new(r"D:\x.txt")), None);
        assert_eq!(rel_components(Path::new(r"C:\"), Path::new(r"C:\x\y.txt")), Some(vec!["x".to_string(), "y.txt".to_string()]));
    }

    #[test]
    fn valid_name_trims_like_explorer_and_refuses_bad_names() {
        assert_eq!(valid_name("  notes.txt  ").as_deref(), Some("notes.txt"));
        assert_eq!(valid_name("draft...").as_deref(), Some("draft"));
        assert_eq!(valid_name("a b.md").as_deref(), Some("a b.md"));
        for bad in ["", "   ", "...", "a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b", "con", "NUL.txt", "com1", "LPT9.log"] {
            assert_eq!(valid_name(bad), None, "{bad:?} should be refused");
        }
        assert_eq!(valid_name("com10").as_deref(), Some("com10"));
        assert_eq!(valid_name("console.txt").as_deref(), Some("console.txt"));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("renotepad-explorer-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn unique_name_counts_up_from_two() {
        let dir = scratch("unique");
        assert_eq!(unique_name(&dir, "New Text Document", ".txt"), dir.join("New Text Document.txt"));
        std::fs::write(dir.join("New Text Document.txt"), b"").unwrap();
        assert_eq!(unique_name(&dir, "New Text Document", ".txt"), dir.join("New Text Document (2).txt"));
        std::fs::write(dir.join("New Text Document (2).txt"), b"").unwrap();
        assert_eq!(unique_name(&dir, "New Text Document", ".txt"), dir.join("New Text Document (3).txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_dir_sorts_filters_and_caps() {
        let dir = scratch("list");
        std::fs::create_dir(dir.join("sub")).unwrap();
        for i in 0..(MAX_ITEMS + 5) {
            std::fs::write(dir.join(format!("f{i}.txt")), b"").unwrap();
        }
        let hidden = dir.join("hidden.txt");
        std::fs::write(&hidden, b"").unwrap();
        assert!(std::process::Command::new("attrib").args(["+h", &hidden.to_string_lossy()]).status().unwrap().success());
        let (v, total) = list_dir(&dir, false).unwrap();
        assert_eq!(total, MAX_ITEMS + 6); // the files and "sub", not the hidden file
        assert_eq!(v.len(), MAX_ITEMS);
        assert_eq!(v[0], Entry { name: "sub".into(), dir: true, link: false });
        assert_eq!(v[1].name, "f0.txt");
        assert_eq!(v[2].name, "f1.txt");
        assert_eq!(list_dir(&dir, true).unwrap().1, MAX_ITEMS + 7);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notify_names_walks_the_chain() {
        // Two records: "a.txt" then "sub\b.md".
        let mut bytes = Vec::new();
        let rec = |next: u32, name: &str| {
            let w: Vec<u8> = name.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
            let mut r = Vec::new();
            r.extend(next.to_le_bytes());
            r.extend(1u32.to_le_bytes());
            r.extend((w.len() as u32).to_le_bytes());
            r.extend(w);
            while r.len() % 4 != 0 {
                r.push(0);
            }
            r
        };
        let first = rec(0, "a.txt");
        let first = rec(first.len() as u32, "a.txt");
        bytes.extend(&first);
        bytes.extend(rec(0, "sub\\b.md"));
        let mut buf = vec![0u32; bytes.len().div_ceil(4)];
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf.as_mut_ptr() as *mut u8, bytes.len()) };
        assert_eq!(notify_names(&buf, bytes.len()), ["a.txt", "sub\\b.md"]);
    }
}
