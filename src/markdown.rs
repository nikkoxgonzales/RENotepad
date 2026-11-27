//! Markdown preview, the pure part: GitHub-flavored Markdown turned into RTF
//! for a read-only RichEdit control (preview.rs), made to look as GitHub
//! renders it. Sizes and colors come from github-markdown-css, at 15 twips a
//! CSS pixel. Also as GitHub: YAML front matter as a table, alerts
//! (`> [!NOTE]`), footnotes, bare URLs as links, `:emoji:` codes, syntax colors
//! in fenced code, and the HTML that GitHub's sanitizer keeps. Nothing here
//! reads a file or the network: pictures come from `Options::picture`.
//!
//! RichEdit draws no paragraph borders or shading, and a table with a percent
//! width paints nothing. So each block box (code, quote, alert, the h1/h2
//! underline, the rule, footnotes) is a one-cell table row as wide as the
//! window, a box in a box is a nested table, and vertical space is a spacer
//! paragraph of exact height. The preview renders again when its width changes.

use crate::frontmatter::{self, Value};
use crate::highlight;
use crate::mdhtml::{self, Token};
use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options as ParseOptions, Parser, Tag, TagEnd, TextMergeStream};
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// Preview colors as COLORREF (0x00BBGGRR).
pub struct Colors {
    pub text: u32,
    pub muted: u32,
    pub link: u32,
    /// Behind inline code.
    pub code_bg: u32,
    /// Behind code blocks and every second table row.
    pub pre_bg: u32,
    /// Table borders, the quote bar and the rule.
    pub border: u32,
    /// The h1/h2 underline.
    pub border_muted: u32,
    /// Behind `<mark>` text.
    pub mark: u32,
    /// Note, Tip, Important, Warning, Caution: (bar, title).
    pub alerts: [(u32, u32); 5],
    /// The roles of `highlight`: comment, keyword, string, constant, entity,
    /// variable, tag, inserted, deleted.
    pub syntax: [u32; 9],
}

/// A picture that the preview can show, from `Options::picture`.
pub struct Picture {
    /// PNG, else JPEG.
    pub png: bool,
    pub width: u32,
    pub height: u32,
    /// The file's bytes as hex digits, as RTF wants them.
    pub hex: std::rc::Rc<str>,
}

pub struct Options<'a> {
    /// The width of the text area in twips.
    pub width: i32,
    /// The `<details>` blocks that the user toggled, by their number in the document:
    /// a closed block shows open, a `<details open>` block shows closed.
    pub toggled_details: &'a [u32],
    /// The picture at an image's URL, or None for a placeholder.
    pub picture: &'a dyn Fn(&str) -> Option<Picture>,
    /// The width of a code character (10pt Consolas) in twips, as the control draws it.
    pub code_char: f32,
}

// Color table entries, in the order `to_rtf` writes them.
const TEXT: u8 = 1;
const MUTED: u8 = 2;
const LINK: u8 = 3;
const CODE_BG: u8 = 4;
const PRE_BG: u8 = 5;
const BORDER: u8 = 6;
const BORDER_MUTED: u8 = 7;
const MARK: u8 = 8;
const ALERT_BAR: u8 = 9;
const ALERT_TITLE: u8 = 14;
const SYNTAX: u8 = 19;

// Font sizes in half-points, distances in twips (a CSS pixel is 15 twips at 96 DPI).
const PX: i32 = 15;
const BODY: u32 = 24; // 16px
const SMALL: u32 = 18; // 12px: footnotes
const CODE: u32 = 20; // 85%
const LINE: i32 = 360; // line-height 1.5
const GAP: i32 = 240; // the 16px margin under paragraphs, lists, tables and boxes
const LIST: i32 = 480; // padding-left 2em
const PAD: i32 = 240; // 1em inside quotes, alerts and code blocks
const BAR: i32 = 60; // the 4px quote bar and rule
const CELL_X: i32 = 195; // 13px
const CELL_Y: i32 = 90; // 6px
const CHAR_W: i32 = 140; // a 16px Segoe UI letter with room: a column that is too narrow wraps its words

/// GitHub's alert icons (octicons) as text symbols, and titles.
const ALERTS: [(&str, &str); 5] = [("\u{2139}", "Note"), ("\u{1F4A1}", "Tip"), ("\u{2757}", "Important"), ("\u{26A0}", "Warning"), ("\u{1F6D1}", "Caution")];

/// The tags whose text GitHub shows escaped (the GFM tagfilter).
const FILTERED: [&str; 9] = ["title", "textarea", "style", "xmp", "iframe", "noembed", "noframes", "script", "plaintext"];

/// The tab's file is Markdown: the preview button shows for it.
pub fn is_markdown(path: &str) -> bool {
    let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
    matches!(ext.as_deref(), Some("md" | "markdown" | "mdown" | "mkd"))
}

/// What clicking a link in the preview does.
#[derive(Debug, PartialEq)]
pub enum Target {
    /// http(s): open in the browser.
    Web(String),
    /// A relative or absolute path to a Markdown file: open in a tab.
    File(PathBuf),
    /// A `<details>` summary: open or close that block (its number in the document).
    Details(u32),
    /// Anything else (mailto:, file:, #anchors, other files) does nothing.
    Nothing,
}

pub fn resolve(url: &str, doc_dir: Option<&Path>) -> Target {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return Target::Web(url.to_string());
    }
    if let Some(n) = url.strip_prefix(DETAILS).and_then(|n| n.parse().ok()) {
        return Target::Details(n);
    }
    // A scheme is letters before ':'; one letter is a drive (C:/notes.md).
    if let Some(colon) = url.find(':') {
        if colon > 1 && url[..colon].chars().all(|c| c.is_ascii_alphabetic()) {
            return Target::Nothing;
        }
    }
    let path = url.split(['#', '?']).next().unwrap_or("");
    let path = percent_decode(path);
    if path.is_empty() || !is_markdown(&path) {
        return Target::Nothing;
    }
    // A UNC link (//host/a.md, \\host\a.md) would connect to that host and can
    // send it the user's Windows credentials. A relative link in a file on a share still works.
    if is_unc(&path) {
        return Target::Nothing;
    }
    let p = Path::new(&path);
    match doc_dir {
        _ if p.is_absolute() => Target::File(p.to_path_buf()),
        Some(dir) => Target::File(dir.join(p)),
        None => Target::Nothing,
    }
}

/// The URL of a `<details>` summary link. A path cannot start so: the colon is not a drive's.
const DETAILS: &str = "details:";

/// A path that starts with two slashes (a network path).
pub fn is_unc(path: &str) -> bool {
    path.chars().take(2).filter(|&c| c == '/' || c == '\\').count() == 2
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The URL in a clicked link's text range. On a click, RichEdit reports the
/// URL itself (the quoted part of the hidden field instruction); a range over
/// the whole field reads `HYPERLINK "url"shown text`. to_rtf keeps quotes out of URLs.
pub fn link_target(range: &str) -> Option<&str> {
    let url = match range.find('"') {
        Some(q) => range[q + 1..].split('"').next()?,
        None => range,
    };
    (!url.is_empty()).then_some(url)
}

/// The kind and pixel size of a PNG or JPEG file, from its first bytes.
pub fn picture_info(b: &[u8]) -> Option<(bool, u32, u32)> {
    let be32 = |i: usize| Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?));
    let be16 = |i: usize| Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?) as u32);
    if b.starts_with(b"\x89PNG\r\n\x1a\n") && b.get(12..16) == Some(b"IHDR") {
        return Some((true, be32(16)?, be32(20)?));
    }
    if !b.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    // JPEG: walk the segments to a start-of-frame marker.
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let m = b[i + 1];
        if m == 0xFF {
            i += 1;
            continue;
        }
        if matches!(m, 0xC0..=0xCF) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
            return Some((false, be16(i + 7)?, be16(i + 5)?));
        }
        i += 2 + be16(i + 2)? as usize;
    }
    None
}

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    Quote,
    Alert,
    Other,
}

/// An open block box: a one-cell table row whose cell holds the box's blocks.
struct Frame {
    /// The row's properties. A nested row writes them after its cell.
    props: String,
    /// The width inside the cell's padding.
    width: i32,
    /// The lists open outside the box: their indent is the box's, not its text's.
    lists_base: usize,
    kind: Kind,
}

struct List {
    /// The next number, or None for bullets.
    next: Option<u64>,
    items: u32,
    footnote: bool,
}

enum Body {
    Inline { rtf: String, chars: usize },
    Table(TableModel),
}

struct Cell {
    body: Body,
    header: bool,
    align: Alignment,
}

struct Row {
    head: bool,
    cells: Vec<Cell>,
}

#[derive(Default)]
struct TableModel {
    align: Vec<Alignment>,
    rows: Vec<Row>,
}

/// A table cell being written: the output that it interrupted, its shown length.
struct CellCapture {
    saved: String,
    chars: usize,
    header: bool,
    align: Alignment,
    inline_base: usize,
}

/// An inline style group opened by an HTML tag.
struct HtmlInline {
    name: String,
    closer: &'static str,
    open: bool,
}

struct Details {
    id: u32,
    open: bool,
    hide: Option<Hide>,
}

/// The state to go back to at the end of a closed `<details>` block, whose content is not shown.
struct Hide {
    at: usize,
    gap: Option<i32>,
    lists: usize,
    frames: usize,
    itap: usize,
    quote: usize,
}

/// The state that a footnote definition interrupts.
struct NoteSaved {
    label: String,
    out: String,
    frames: Vec<Frame>,
    lists: Vec<List>,
    gap: Option<i32>,
    itap: usize,
    quote: usize,
}

struct Writer<'a> {
    opts: &'a Options<'a>,
    out: String,
    frames: Vec<Frame>,
    /// The table depth of the paragraphs being written (RTF \itap).
    itap: usize,
    lists: Vec<List>,
    quote: usize,
    /// The space owed before the next block: the larger of the last block's
    /// bottom margin and the next one's top margin, as CSS margins collapse.
    /// None at the start of a box or the document, where a first block has no top margin.
    gap: Option<i32>,
    /// Paragraph properties not written yet. They are written with the first
    /// text, so a loose list item's own paragraph does not start a second one.
    pending: Option<String>,
    /// A paragraph has been started (its properties are written).
    open: bool,
    /// The table depth of the last paragraph end written.
    last_par_itap: usize,
    /// List marker waiting for the item's first text.
    marker: Option<String>,
    /// Inside a code block: its text and language.
    code: Option<(String, String)>,
    html_pre: bool,
    table: Option<TableModel>,
    cell: Option<CellCapture>,
    /// Inside an image: its URL and alt text.
    image: Option<(String, String)>,
    /// Open links: text in them is not linked again.
    links: usize,
    /// Inside a heading: its font size and underline padding (0: no underline).
    heading: Option<(u32, i32)>,
    /// HTML `align` values, innermost last.
    align: Vec<Option<&'static str>>,
    html_buf: String,
    html_inline: Vec<HtmlInline>,
    html_head: bool,
    details: Vec<Details>,
    details_next: u32,
    /// Writing a footnote: small, muted text.
    small: bool,
    notes: Vec<(String, String)>,
    note_refs: Vec<String>,
    note_saved: Option<NoteSaved>,
}

impl<'a> Writer<'a> {
    fn new(opts: &'a Options<'a>) -> Self {
        Writer {
            opts,
            out: String::new(),
            frames: Vec::new(),
            itap: 0,
            lists: Vec::new(),
            quote: 0,
            gap: None,
            pending: None,
            open: false,
            last_par_itap: 0,
            marker: None,
            code: None,
            html_pre: false,
            table: None,
            cell: None,
            image: None,
            links: 0,
            heading: None,
            align: Vec::new(),
            html_buf: String::new(),
            html_inline: Vec::new(),
            html_head: false,
            details: Vec::new(),
            details_next: 0,
            small: false,
            notes: Vec::new(),
            note_refs: Vec::new(),
            note_saved: None,
        }
    }

    fn width(&self) -> i32 {
        self.frames.last().map_or(self.opts.width.max(2000), |f| f.width)
    }

    /// The indent of the open lists inside the current box.
    fn indent(&self) -> i32 {
        let base = self.frames.last().map_or(0, |f| f.lists_base);
        self.lists.len().saturating_sub(base) as i32 * LIST
    }

    fn color(&self) -> u8 {
        if self.small || self.quote > 0 { MUTED } else { TEXT }
    }

    fn size(&self) -> u32 {
        if self.small { SMALL } else { BODY }
    }

    fn tbl(&self) -> String {
        if self.itap > 0 { format!("\\intbl\\itap{}", self.itap) } else { String::new() }
    }

    fn props(&self, size: u32, color: u8, line: i32, para: &str, chars: &str) -> String {
        let align = self.align.iter().rev().flatten().next().copied().unwrap_or("");
        format!("\\pard{}{align}\\li{}\\sl{line}\\slmult0{para}\\plain\\f0\\fs{size}\\cf{color}{chars} ", self.tbl(), self.indent())
    }

    fn par_end(&mut self) {
        self.out.push_str("\\par\n");
        self.last_par_itap = self.itap;
    }

    /// An empty paragraph exactly `h` twips high.
    fn spacer(&mut self, h: i32) {
        if h > 0 {
            let t = self.tbl();
            self.out.push_str(&format!("\\pard{t}\\plain\\fs2\\sl-{h}\\slmult0"));
            self.par_end();
        }
    }

    /// Write the space owed before a block whose top margin is `top`.
    fn block(&mut self, top: i32) {
        if let Some(g) = self.gap.take() {
            self.spacer(g.max(top));
        }
    }

    fn para(&mut self) {
        self.end();
        self.block(0);
        self.pending = Some(self.props(self.size(), self.color(), LINE, "", ""));
    }

    fn flush(&mut self) {
        if let Some(p) = self.pending.take() {
            self.out.push_str(&p);
            self.open = true;
        }
        if let Some(m) = self.marker.take() {
            self.out.push_str(&m);
        }
    }

    fn end(&mut self) {
        if self.pending.is_some() || self.marker.is_some() {
            self.flush();
        }
        if self.open {
            self.close_html_inline(0);
            self.par_end();
            self.open = false;
        }
    }

    /// Start a paragraph for HTML content that has none.
    fn ensure_para(&mut self) {
        if !self.open && self.pending.is_none() && self.cell.is_none() && self.image.is_none() && self.code.is_none() {
            self.para();
        }
    }

    // ---- inline content ----

    fn text(&mut self, t: &str) {
        if let Some((code, _)) = &mut self.code {
            code.push_str(t);
            return;
        }
        if let Some((_, alt)) = &mut self.image {
            alt.push_str(t);
            return;
        }
        self.flush();
        if self.links > 0 {
            return self.plain(t);
        }
        for seg in autolinks(t) {
            match seg {
                Seg::Text(s) => self.plain(s),
                Seg::Link(url, shown) => {
                    self.link_open(&url, LINK);
                    self.plain(shown);
                    self.out.push_str("}}}");
                }
            }
        }
    }

    fn plain(&mut self, t: &str) {
        let t = emoji(t);
        if let Some(c) = &mut self.cell {
            c.chars += t.chars().count();
        }
        escape(&mut self.out, &t);
    }

    fn inline_code(&mut self, t: &str) {
        if let Some((_, alt)) = &mut self.image {
            alt.push_str(t);
            return;
        }
        self.flush();
        // In a heading, code has the heading's size (font-size: inherit).
        let size = self.heading.map_or(CODE, |h| h.0);
        self.out.push_str(&format!("{{\\f1\\fs{size}\\highlight{CODE_BG} "));
        if let Some(c) = &mut self.cell {
            c.chars += t.chars().count();
        }
        escape(&mut self.out, t);
        self.out.push('}');
    }

    fn group(&mut self, open: &str) {
        if self.image.is_none() {
            self.flush();
            self.out.push_str(open);
        }
    }

    fn group_end(&mut self, close: &str) {
        if self.image.is_none() {
            self.out.push_str(close);
        }
    }

    fn link_open(&mut self, url: &str, color: u8) {
        // A quote would end the field instruction; a backslash would start a switch.
        let url = url.replace('"', "%22").replace('\\', "/");
        let mut f = String::from("{\\field{\\*\\fldinst{HYPERLINK \"");
        escape(&mut f, &url);
        f.push_str(&format!("\"}}}}{{\\fldrslt{{\\cf{color} "));
        self.out.push_str(&f);
    }

    fn image_end(&mut self) {
        let Some((url, alt)) = self.image.take() else { return };
        self.ensure_para();
        self.flush();
        self.picture(&url, &alt, None, None);
    }

    /// An image: the picture, scaled to fit, or GitHub's look for a missing one (its alt text).
    fn picture(&mut self, url: &str, alt: &str, w: Option<i32>, h: Option<i32>) {
        let max = ((self.width() - self.indent()) / PX).max(16);
        match (self.opts.picture)(url) {
            Some(p) => {
                // The sizes come from the file and the HTML: i64, then clamped, cannot overflow.
                const BIG: i64 = 1 << 20;
                let (nw, nh) = (i64::from(p.width).clamp(1, BIG), i64::from(p.height).clamp(1, BIG));
                let (w, h) = (w.map(|v| i64::from(v).min(BIG)), h.map(|v| i64::from(v).min(BIG)));
                let (mut pw, mut ph) = match (w, h) {
                    (Some(w), Some(h)) => (w, h),
                    (Some(w), None) => (w, nh * w / nw),
                    (None, Some(h)) => (nw * h / nh, h),
                    _ => (nw, nh),
                };
                let max = i64::from(max);
                if pw > max {
                    ph = ph * max / pw;
                    pw = max;
                }
                // At most `max` wide and one picture height of 2^20 px: the twips fit an i32.
                let (pw, ph) = (pw.max(1) as i32, ph.clamp(1, BIG) as i32);
                let blip = if p.png { "\\pngblip" } else { "\\jpegblip" };
                self.out.push_str(&format!("{{\\pict{blip}\\picw{nw}\\pich{nh}\\picwgoal{}\\pichgoal{} ", pw * PX, ph.max(1) * PX));
                self.out.push_str(&p.hex);
                self.out.push('}');
                if let Some(c) = &mut self.cell {
                    c.chars += (pw * PX / CHAR_W) as usize;
                }
            }
            None => {
                let shown = format!("\u{1F5BC}\u{FE0E} {}", if alt.is_empty() { "image" } else { alt });
                self.out.push_str(&format!("{{\\cf{MUTED} "));
                escape(&mut self.out, &shown);
                self.out.push('}');
                if let Some(c) = &mut self.cell {
                    c.chars += shown.chars().count();
                }
            }
        }
    }

    fn note_ref(&mut self, label: &str) {
        let n = match self.note_refs.iter().position(|l| l == label) {
            Some(i) => i + 1,
            None => {
                self.note_refs.push(label.to_string());
                self.note_refs.len()
            }
        };
        self.flush();
        self.out.push_str(&format!("{{\\super\\cf{LINK} {n}}}"));
        if let Some(c) = &mut self.cell {
            c.chars += 1;
        }
    }

    // ---- blocks ----

    /// Open a box: `cell` holds the cell's border and shading words, `pad` its
    /// left and right padding, `top` the box's top margin.
    fn open_box(&mut self, cell: &str, pad: i32, top: i32, kind: Kind) {
        self.end();
        self.block(top);
        let left = self.indent();
        let right = self.width();
        let props = format!("\\trowd\\trgaph{pad}\\trleft{left}{cell}\\cellx{right}");
        let d = self.itap + 1;
        if d == 1 {
            self.out.push_str(&props);
            self.out.push('\n');
        }
        self.frames.push(Frame { props, width: (right - left - 2 * pad).max(1000), lists_base: self.lists.len(), kind });
        self.itap = d;
        self.gap = None;
        if kind == Kind::Quote {
            self.quote += 1;
        }
    }

    /// Close the innermost box: `pad` is its bottom padding, `bottom` its bottom margin.
    fn close_box(&mut self, bottom: i32, pad: i32) {
        self.end();
        let Some(f) = self.frames.pop() else { return };
        self.gap = None; // a last block has no bottom margin in its box
        self.spacer(pad);
        let d = self.itap;
        self.end_cell(d);
        if d == 1 {
            self.out.push_str("\\row\n");
        } else {
            self.out.push_str(&format!("{{\\*\\nesttableprops{}\\nestrow}}{{\\nonesttables\\par}}\n", f.props));
        }
        self.itap = d - 1;
        if f.kind == Kind::Quote {
            self.quote = self.quote.saturating_sub(1);
        }
        self.gap = Some(bottom);
    }

    /// End a table cell at depth `d`: its last paragraph ends with the cell.
    fn end_cell(&mut self, d: usize) {
        if self.out.ends_with("\\par\n") && self.last_par_itap == d {
            self.out.truncate(self.out.len() - 5);
        } else {
            self.out.push_str(&format!("\\pard\\intbl\\itap{d}\\plain\\fs2\\sl-15\\slmult0"));
        }
        self.out.push_str(if d == 1 { "\\cell\n" } else { "\\nestcell\n" });
    }

    fn heading_start(&mut self, level: u8) {
        self.end();
        let (size, pad) = match level {
            1 => (48, 144),
            2 => (36, 108),
            3 => (30, 0),
            4 => (24, 0),
            5 => (21, 0),
            _ => (20, 0),
        };
        if pad > 0 {
            self.open_box(&format!("\\clbrdrb\\brdrs\\brdrw15\\brdrcf{BORDER_MUTED}"), 0, GAP + 120, Kind::Other);
        } else {
            self.block(GAP + 120);
        }
        let color = if level >= 6 { MUTED } else { self.color() };
        self.pending = Some(self.props(size, color, size as i32 * 25 / 2, "", "\\b"));
        self.heading = Some((size, pad));
    }

    fn heading_end(&mut self) {
        self.end();
        let (_, pad) = self.heading.take().unwrap_or((0, 0));
        if pad > 0 {
            self.close_box(GAP, pad);
        } else {
            self.gap = Some(GAP);
        }
    }

    fn quote_start(&mut self, kind: Option<BlockQuoteKind>) {
        let Some(k) = kind else {
            return self.open_box(&format!("\\clbrdrl\\brdrs\\brdrw{BAR}\\brdrcf{BORDER}"), PAD, 0, Kind::Quote);
        };
        let i = match k {
            BlockQuoteKind::Note => 0,
            BlockQuoteKind::Tip => 1,
            BlockQuoteKind::Important => 2,
            BlockQuoteKind::Warning => 3,
            BlockQuoteKind::Caution => 4,
        };
        self.open_box(&format!("\\clbrdrl\\brdrs\\brdrw{BAR}\\brdrcf{}", ALERT_BAR + i as u8), PAD, 0, Kind::Alert);
        self.spacer(120);
        let (icon, name) = ALERTS[i];
        self.pending = Some(self.props(BODY, ALERT_TITLE + i as u8, LINE, "", "\\b"));
        self.flush();
        escape(&mut self.out, &format!("{icon}\u{FE0E}  {name}"));
        self.end();
        self.gap = Some(GAP);
    }

    fn quote_end(&mut self) {
        let alert = self.frames.last().is_some_and(|f| f.kind == Kind::Alert);
        self.close_box(GAP, if alert { 120 } else { 0 });
    }

    /// A code block: GitHub's shaded box with syntax colors. RichEdit draws
    /// text in a shaded table cell in its own color, so the box is the text's
    /// highlight instead: each line is padded with spaces to the box width (the
    /// font is monospaced), and a blank line above and below is the padding.
    /// Ceiling: a wide character (CJK, emoji) takes two columns, so its line
    /// ends short; a line longer than the box wraps at the box edge.
    fn code_block(&mut self, code: &str, lang: &str) {
        let code = code.replace("\r\n", "\n").replace('\t', "    ");
        let code = code.strip_suffix('\n').unwrap_or(&code);
        self.end();
        self.block(0);
        let avail = (self.width() - self.indent()) as f32;
        // One column less than fits: the last character must not wrap.
        let cols = ((avail / self.opts.code_char.max(50.0)) as usize).saturating_sub(1).max(12);
        let text_cols = cols - 4;
        // Cut the spans into box lines of at most text_cols characters.
        let mut lines: Vec<Vec<(Option<u8>, &str)>> = vec![Vec::new()];
        let mut used = 0;
        for (role, piece) in highlight::spans(lang, code) {
            for (k, part) in piece.split('\n').enumerate() {
                if k > 0 {
                    lines.push(Vec::new());
                    used = 0;
                }
                let mut rest = part;
                while !rest.is_empty() {
                    if used == text_cols {
                        lines.push(Vec::new());
                        used = 0;
                    }
                    let take = rest.char_indices().nth(text_cols - used).map_or(rest.len(), |(i, _)| i);
                    let (head, tail) = rest.split_at(take);
                    used += head.chars().count();
                    if let Some(l) = lines.last_mut() {
                        l.push((role, head));
                    }
                    rest = tail;
                }
            }
        }
        self.pending = Some(self.props(CODE, TEXT, 0, "", &format!("\\f1\\highlight{PRE_BG}")));
        self.flush();
        let blank = format!("{}\\u160?", " ".repeat(cols - 1));
        self.out.push_str(&blank);
        for line in &lines {
            self.out.push_str("\\line   ");
            let mut n = 0;
            for (role, s) in line {
                n += s.chars().count();
                match role {
                    Some(r) => {
                        self.out.push_str(&format!("{{\\cf{} ", SYNTAX + r));
                        escape(&mut self.out, s);
                        self.out.push('}');
                    }
                    None => escape(&mut self.out, s),
                }
            }
            // A no-break space ends the line: RichEdit does not highlight trailing spaces.
            self.out.push_str(&" ".repeat(cols.saturating_sub(n + 3)));
            self.out.push_str("\\u160?");
        }
        self.out.push_str("\\line ");
        self.out.push_str(&blank);
        self.end();
        self.gap = Some(GAP);
    }

    fn rule(&mut self) {
        self.open_box(&format!("\\clbrdrb\\brdrs\\brdrw{BAR}\\brdrcf{BORDER}"), 0, 360, Kind::Other);
        self.close_box(360, 0);
    }

    fn list_start(&mut self, first: Option<u64>) {
        self.end();
        self.lists.push(List { next: first, items: 0, footnote: false });
    }

    fn list_end(&mut self) {
        self.end();
        self.lists.pop();
        let base = self.frames.last().map_or(0, |f| f.lists_base);
        if self.lists.len() <= base {
            self.gap = Some(self.gap.unwrap_or(0).max(GAP));
        }
    }

    fn item_start(&mut self) {
        self.end();
        let depth = self.lists.len();
        let Some(list) = self.lists.last_mut() else { return self.para() };
        let first = list.items == 0;
        list.items += 1;
        let footnote = list.footnote;
        let marker = match &mut list.next {
            Some(n) => {
                let v = *n;
                *n = n.saturating_add(1);
                // ol, ol ol, ol ol ol: decimal, lower-roman, lower-alpha.
                match depth {
                    1 => format!("{v}."),
                    2 => format!("{}.", roman(v)),
                    _ => format!("{}.", alpha(v)),
                }
            }
            None => ["\u{2022}", "\u{25E6}", "\u{25AA}"][depth.clamp(1, 3) - 1].to_string(),
        };
        if !first {
            self.gap = Some(self.gap.unwrap_or(0).max(60)); // li + li: margin-top .25em
        }
        self.block(0);
        let left = self.indent();
        // The marker ends just before the text, at a right tab.
        self.pending = Some(self.props(self.size(), self.color(), LINE, &format!("\\fi-{LIST}\\tqr\\tx{}\\tx{left}", left - 120), ""));
        let mut m = String::from("\\tab ");
        if footnote {
            m.push('\u{1}'); // the number, known at the end
        } else {
            escape(&mut m, &marker);
        }
        m.push_str("\\tab ");
        self.marker = Some(m);
    }

    // ---- tables ----

    fn cell_start(&mut self, header: bool, align: Alignment) {
        let saved = std::mem::take(&mut self.out);
        self.cell = Some(CellCapture { saved, chars: 0, header, align, inline_base: self.html_inline.len() });
    }

    fn cell_end(&mut self) {
        let Some(base) = self.cell.as_ref().map(|c| c.inline_base) else { return };
        self.close_html_inline(base);
        let Some(c) = self.cell.take() else { return };
        let rtf = std::mem::replace(&mut self.out, c.saved);
        if let Some(t) = &mut self.table {
            if t.rows.is_empty() {
                t.rows.push(Row { head: false, cells: Vec::new() });
            }
            if let Some(r) = t.rows.last_mut() {
                let header = c.header || r.head;
                r.cells.push(Cell { body: Body::Inline { rtf, chars: c.chars }, header, align: c.align });
            }
        }
    }

    fn table_end(&mut self) {
        if self.cell.is_some() {
            self.cell_end();
        }
        if let Some(t) = self.table.take() {
            let left = self.indent();
            let avail = self.width() - left;
            self.write_table(&t, self.itap + 1, left, avail);
            self.gap = Some(GAP);
        }
    }

    /// Write a table at depth `d`, `left` twips in, at most `avail` wide.
    fn write_table(&mut self, t: &TableModel, d: usize, left: i32, avail: i32) {
        let widths = fit_columns(t, avail);
        let saved = self.itap;
        let border = format!("\\brdrs\\brdrw15\\brdrcf{BORDER}");
        // GitHub shades every second body row. Not here: RichEdit draws all text
        // in a shaded cell in one color of its own, so links and code lose theirs.
        for row in &t.rows {
            let mut props = format!("\\trowd\\trgaph{CELL_X}\\trleft{left}");
            let mut x = left;
            for w in &widths {
                x += w;
                props.push_str(&format!("\\clbrdrt{border}\\clbrdrl{border}\\clbrdrb{border}\\clbrdrr{border}\\cellx{x}"));
            }
            if d == 1 {
                self.out.push_str(&props);
                self.out.push('\n');
            }
            for (k, w) in widths.iter().enumerate() {
                self.itap = d;
                self.spacer(CELL_Y);
                match row.cells.get(k) {
                    Some(Cell { body: Body::Inline { rtf, .. }, header, align }) => {
                        let align = match (align, t.align.get(k)) {
                            (Alignment::Center, _) | (Alignment::None, Some(Alignment::Center)) => "\\qc",
                            (Alignment::Right, _) | (Alignment::None, Some(Alignment::Right)) => "\\qr",
                            _ => "",
                        };
                        let bold = if *header { "\\b" } else { "" };
                        let tbl = self.tbl();
                        self.out.push_str(&format!("\\pard{tbl}{align}\\sl{LINE}\\slmult0\\plain\\f0\\fs{BODY}\\cf{TEXT}{bold} {rtf}"));
                        self.par_end();
                    }
                    Some(Cell { body: Body::Table(inner), .. }) => self.write_table(inner, d + 1, 0, w - 2 * CELL_X),
                    None => {}
                }
                self.itap = d;
                self.spacer(CELL_Y);
                self.end_cell(d);
            }
            if d == 1 {
                self.out.push_str("\\row\n");
            } else {
                self.out.push_str(&format!("{{\\*\\nesttableprops{props}\\nestrow}}{{\\nonesttables\\par}}\n"));
            }
        }
        self.itap = saved;
    }

    // ---- front matter and footnotes ----

    fn front_matter(&mut self, v: &Value) {
        let t = fm_table(v, true);
        let avail = self.width();
        self.write_table(&t, self.itap + 1, 0, avail);
        self.gap = Some(GAP);
    }

    fn note_start(&mut self, label: &str) {
        self.end();
        let saved = NoteSaved {
            label: label.to_string(),
            out: std::mem::take(&mut self.out),
            frames: std::mem::take(&mut self.frames),
            lists: std::mem::take(&mut self.lists),
            gap: self.gap.take(),
            itap: self.itap,
            quote: self.quote,
        };
        self.note_saved = Some(saved);
        self.frames.push(Frame { props: String::new(), width: self.opts.width.max(2000), lists_base: 0, kind: Kind::Other });
        self.itap = 1;
        self.quote = 0;
        self.small = true;
        self.lists.push(List { next: None, items: 0, footnote: true });
        self.item_start();
    }

    fn note_end(&mut self) {
        self.end();
        let Some(s) = self.note_saved.take() else { return };
        // GitHub ends a footnote with a link back to its reference.
        if self.out.ends_with("\\par\n") {
            let at = self.out.len() - 5;
            let mut back = format!(" {{\\cf{LINK} ");
            escape(&mut back, "\u{21A9}");
            back.push('}');
            self.out.insert_str(at, &back);
        }
        let rtf = std::mem::replace(&mut self.out, s.out);
        self.notes.push((s.label, rtf));
        self.frames = s.frames;
        self.lists = s.lists;
        self.gap = s.gap;
        self.itap = s.itap;
        self.quote = s.quote;
        self.small = false;
    }

    /// The footnotes, at the end in the order of their first reference, as GitHub shows them.
    fn footnotes(&mut self) {
        let items: Vec<String> = self
            .note_refs
            .iter()
            .enumerate()
            .filter_map(|(i, label)| self.notes.iter().find(|(l, _)| l == label).map(|(_, rtf)| rtf.replace('\u{1}', &format!("{}.", i + 1))))
            .collect();
        if items.is_empty() || self.itap != 0 {
            return;
        }
        self.end();
        self.open_box(&format!("\\clbrdrt\\brdrs\\brdrw15\\brdrcf{BORDER}"), 0, GAP, Kind::Other);
        self.spacer(GAP);
        for it in items {
            self.out.push_str(&it);
        }
        self.last_par_itap = 1;
        self.close_box(0, 0);
    }

    // ---- HTML ----

    fn html(&mut self, s: &str) {
        self.html_buf.push_str(s);
        let buf = std::mem::take(&mut self.html_buf);
        let (toks, used) = mdhtml::tokens(&buf);
        self.html_buf = buf[used..].to_string();
        for t in toks {
            self.html_token(t);
        }
    }

    /// The end of an HTML block: a tag that never closed is text.
    fn html_flush(&mut self) {
        if !self.html_buf.is_empty() {
            let b = std::mem::take(&mut self.html_buf);
            self.html_text(&b, false);
        }
    }

    fn html_text(&mut self, s: &str, raw: bool) {
        if let Some((code, _)) = &mut self.code {
            code.push_str(s);
            return;
        }
        // Whitespace in HTML collapses to one space.
        let mut t = String::with_capacity(s.len());
        for ch in s.chars() {
            if ch.is_whitespace() && !raw {
                if !t.ends_with(' ') {
                    t.push(' ');
                }
            } else {
                t.push(ch);
            }
        }
        if t.trim().is_empty() {
            if !t.is_empty() && (self.open || self.cell.is_some()) {
                self.text(" ");
            }
            return;
        }
        let start = !self.open && self.cell.is_none();
        self.ensure_para();
        self.text(if start { t.trim_start() } else { &t });
    }

    fn html_token(&mut self, t: Token) {
        match &t {
            Token::Text(s) => self.html_text(s, false),
            Token::Open { name, raw, .. } | Token::Close { name, raw } if FILTERED.contains(&name.as_str()) => {
                let r = raw.clone();
                self.html_text(&r, true);
            }
            Token::Open { name, .. } => {
                if self.html_pre {
                    return; // tags in <pre> are dropped
                }
                let name = name.clone();
                self.html_open(&name, &t);
            }
            Token::Close { name, .. } => {
                if self.html_pre && name != "pre" {
                    return;
                }
                let name = name.clone();
                self.html_close(&name);
            }
        }
    }

    fn html_group(&mut self, name: &str, open: String) {
        self.ensure_para();
        self.flush();
        self.out.push_str(&open);
        self.html_inline.push(HtmlInline { name: name.to_string(), closer: "}", open: true });
    }

    /// Close the HTML style groups from `base` on, so that none spans a paragraph or cell end.
    fn close_html_inline(&mut self, base: usize) {
        for i in (base..self.html_inline.len()).rev() {
            let g = &mut self.html_inline[i];
            if g.open {
                g.open = false;
                self.out.push_str(g.closer);
                if g.closer == "}}}" {
                    self.links = self.links.saturating_sub(1);
                }
            }
        }
    }

    fn html_open(&mut self, name: &str, t: &Token) {
        let in_cell = self.cell.is_some();
        let align = match t.attr("align").map(str::to_ascii_lowercase).as_deref() {
            Some("center") => Some("\\qc"),
            Some("right") => Some("\\qr"),
            Some("justify") => Some("\\qj"),
            Some("left") => Some("\\ql"),
            _ => None,
        };
        let px = |k: &str| t.attr(k).and_then(|v| v.trim().trim_end_matches("px").parse::<i32>().ok()).filter(|v| *v > 0);
        match name {
            "p" | "div" if !in_cell => {
                self.end();
                self.align.push(align);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" if !in_cell => {
                self.align.push(align);
                self.heading_start(name.as_bytes()[1] - b'0');
            }
            "br" => {
                self.ensure_para();
                self.flush();
                self.out.push_str("\\line ");
            }
            "hr" if !in_cell => {
                self.end();
                self.rule();
            }
            "img" => {
                self.ensure_para();
                self.flush();
                let (src, alt) = (t.attr("src").unwrap_or("").to_string(), t.attr("alt").unwrap_or("").to_string());
                self.picture(&src, &alt, px("width"), px("height"));
            }
            "a" => {
                if let Some(href) = t.attr("href").filter(|h| !h.is_empty()) {
                    self.ensure_para();
                    self.flush();
                    let href = href.to_string();
                    self.link_open(&href, LINK);
                    self.links += 1;
                    self.html_inline.push(HtmlInline { name: "a".into(), closer: "}}}", open: true });
                }
            }
            "b" | "strong" => self.html_group(name, "{\\b ".into()),
            "i" | "em" | "cite" | "dfn" | "var" => self.html_group(name, "{\\i ".into()),
            "s" | "strike" | "del" => self.html_group(name, "{\\strike ".into()),
            "ins" => self.html_group(name, "{\\ul ".into()),
            "sub" => self.html_group(name, "{\\sub ".into()),
            "sup" => self.html_group(name, "{\\super ".into()),
            "code" | "tt" | "samp" => self.html_group(name, format!("{{\\f1\\fs{CODE}\\highlight{CODE_BG} ")),
            "kbd" => self.html_group(name, format!("{{\\f1\\fs17\\highlight{PRE_BG} ")),
            "mark" => self.html_group(name, format!("{{\\highlight{MARK} ")),
            "small" => self.html_group(name, format!("{{\\fs{} ", self.size() * 9 / 10)),
            "abbr" if t.attr("title").is_some() => self.html_group(name, "{\\uld ".into()),
            "q" => {
                self.ensure_para();
                self.text("\u{201C}");
            }
            "ul" if !in_cell => self.list_start(None),
            "ol" if !in_cell => self.list_start(Some(t.attr("start").and_then(|s| s.parse().ok()).unwrap_or(1))),
            "li" if !in_cell => self.item_start(),
            "blockquote" if !in_cell => self.quote_start(None),
            "pre" if !in_cell => {
                self.end();
                self.code = Some((String::new(), String::new()));
                self.html_pre = true;
            }
            "table" if !in_cell && self.table.is_none() => {
                self.end();
                self.block(0);
                self.table = Some(TableModel::default());
            }
            "thead" => self.html_head = true,
            "tbody" | "tfoot" => self.html_head = false,
            "tr" => {
                if self.cell.is_some() {
                    self.cell_end();
                }
                let head = self.html_head;
                if let Some(tb) = &mut self.table {
                    tb.rows.push(Row { head, cells: Vec::new() });
                }
            }
            "td" | "th" if self.table.is_some() => {
                if self.cell.is_some() {
                    self.cell_end();
                }
                let a = match align {
                    Some("\\qc") => Alignment::Center,
                    Some("\\qr") => Alignment::Right,
                    _ => Alignment::None,
                };
                self.cell_start(name == "th", a);
            }
            "details" if !in_cell => self.details_open(t.attr("open").is_some()),
            "summary" if !in_cell => self.summary_open(),
            "dt" if !in_cell => {
                self.para();
                self.html_group("dt", "{\\b ".into());
            }
            "dd" if !in_cell => {
                self.end();
                self.block(0);
                self.pending = Some(self.props(self.size(), self.color(), LINE, &format!("\\li{}", self.indent() + PAD), ""));
            }
            // Other tags are removed and their content kept, as GitHub's sanitizer does.
            _ => {}
        }
    }

    fn html_close(&mut self, name: &str) {
        let in_cell = self.cell.is_some();
        match name {
            "p" if !in_cell => {
                self.end();
                self.align.pop();
                self.gap = Some(GAP);
            }
            "div" if !in_cell => {
                self.end();
                self.align.pop();
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" if !in_cell && self.heading.is_some() => {
                self.heading_end();
                self.align.pop();
            }
            "ul" | "ol" if !in_cell => self.list_end(),
            "li" if !in_cell => self.end(),
            "blockquote" if !in_cell && self.frames.last().is_some_and(|f| f.kind == Kind::Quote) => self.quote_end(),
            "pre" => {
                if let Some((code, _)) = self.code.take() {
                    self.html_pre = false;
                    let code = code.strip_prefix('\n').unwrap_or(&code).to_string();
                    self.code_block(&code, "");
                }
            }
            "table" => self.table_end(),
            "td" | "th" => self.cell_end(),
            "thead" => self.html_head = false,
            // Gated as in html_open: a cell's `</details>` must not close a block outside the table.
            "details" if !in_cell => self.details_close(),
            "summary" if !in_cell => self.summary_close(),
            "q" => self.text("\u{201D}"),
            "dt" => {
                self.html_inline_close("dt");
                self.end();
            }
            "dd" => {
                self.end();
                self.gap = Some(GAP);
            }
            _ => self.html_inline_close(name),
        }
    }

    fn html_inline_close(&mut self, name: &str) {
        if let Some(i) = self.html_inline.iter().rposition(|g| g.name == name) {
            let g = self.html_inline.remove(i);
            if g.open {
                self.out.push_str(g.closer);
                if g.closer == "}}}" {
                    self.links = self.links.saturating_sub(1);
                }
            }
        }
    }

    /// `open`: the tag has the `open` attribute, which GitHub keeps.
    fn details_open(&mut self, open: bool) {
        self.end();
        let id = self.details_next;
        self.details_next += 1;
        let open = open != self.opts.toggled_details.contains(&id);
        self.details.push(Details { id, open, hide: None });
    }

    /// A summary is a link that opens or closes its block (the preview renders again).
    fn summary_open(&mut self) {
        let Some((id, open)) = self.details.last().map(|d| (d.id, d.open)) else { return self.para() };
        self.para();
        self.flush();
        self.link_open(&format!("{DETAILS}{id}"), self.color());
        escape(&mut self.out, if open { "\u{25BC} " } else { "\u{25B6} " });
        self.links += 1;
        self.html_inline.push(HtmlInline { name: "summary".into(), closer: "}}}", open: true });
    }

    fn summary_close(&mut self) {
        self.html_inline_close("summary");
        self.end();
        self.gap = Some(0);
        let snapshot = Hide { at: 0, gap: self.gap, lists: self.lists.len(), frames: self.frames.len(), itap: self.itap, quote: self.quote };
        let at = self.out.len();
        if let Some(d) = self.details.last_mut() {
            if !d.open && d.hide.is_none() {
                d.hide = Some(Hide { at, ..snapshot });
            }
        }
    }

    fn details_close(&mut self) {
        self.end();
        if let Some(d) = self.details.pop() {
            if let Some(h) = d.hide {
                self.out.truncate(h.at);
                self.lists.truncate(h.lists);
                self.frames.truncate(h.frames);
                self.itap = h.itap;
                self.quote = h.quote;
                self.gap = h.gap;
                self.pending = None;
                self.marker = None;
                self.open = false;
            }
        }
        self.gap = Some(self.gap.unwrap_or(0).max(GAP));
    }

    // ---- pulldown-cmark events ----

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                if self.pending.is_none() && self.cell.is_none() {
                    self.para();
                }
            }
            Tag::Heading { level, .. } => self.heading_start(match level {
                HeadingLevel::H1 => 1,
                HeadingLevel::H2 => 2,
                HeadingLevel::H3 => 3,
                HeadingLevel::H4 => 4,
                HeadingLevel::H5 => 5,
                HeadingLevel::H6 => 6,
            }),
            Tag::BlockQuote(kind) => self.quote_start(kind),
            Tag::CodeBlock(kind) => {
                self.end();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split([' ', ',', '{']).next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((String::new(), lang));
            }
            Tag::List(first) => self.list_start(first),
            Tag::Item => self.item_start(),
            Tag::FootnoteDefinition(label) => self.note_start(&label),
            Tag::Table(align) => {
                self.end();
                self.block(0);
                self.table = Some(TableModel { align, rows: Vec::new() });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.rows.push(Row { head: matches!(tag, Tag::TableHead), cells: Vec::new() });
                }
            }
            Tag::TableCell => self.cell_start(false, Alignment::None),
            Tag::Emphasis => self.group("{\\i "),
            Tag::Strong => self.group("{\\b "),
            Tag::Strikethrough => self.group("{\\strike "),
            Tag::Link { dest_url, .. } => {
                if self.image.is_none() {
                    self.flush();
                    self.link_open(&dest_url, LINK);
                    self.links += 1;
                }
            }
            Tag::Image { dest_url, .. } if self.image.is_none() => self.image = Some((dest_url.to_string(), String::new())),
            _ => {}
        }
    }

    fn finish(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                if self.cell.is_none() {
                    self.end();
                    self.gap = Some(GAP);
                }
            }
            TagEnd::Heading(_) => self.heading_end(),
            TagEnd::BlockQuote(_) => self.quote_end(),
            TagEnd::CodeBlock => {
                if let Some((code, lang)) = self.code.take() {
                    self.code_block(&code, &lang);
                }
            }
            TagEnd::HtmlBlock => self.html_flush(),
            TagEnd::List(_) => self.list_end(),
            TagEnd::Item => self.end(),
            TagEnd::FootnoteDefinition => self.note_end(),
            TagEnd::Table => self.table_end(),
            TagEnd::TableCell => self.cell_end(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.group_end("}"),
            TagEnd::Link => {
                if self.image.is_none() && self.links > 0 {
                    self.out.push_str("}}}");
                    self.links -= 1;
                }
            }
            TagEnd::Image => self.image_end(),
            _ => {}
        }
    }
}

/// The table GitHub makes from front matter: at the top, one row per key
/// (key, value); a nested mapping, its keys over its values; a list, one row.
fn fm_table(v: &Value, top: bool) -> TableModel {
    let cell = |v: &Value| match v {
        Value::Str(s) => {
            let mut rtf = String::new();
            for (i, line) in s.lines().enumerate() {
                if i > 0 {
                    rtf.push_str("\\line ");
                }
                escape(&mut rtf, line);
            }
            let chars = s.lines().map(|l| l.chars().count()).max().unwrap_or(0);
            Cell { body: Body::Inline { rtf, chars }, header: false, align: Alignment::None }
        }
        other => Cell { body: Body::Table(fm_table(other, false)), header: false, align: Alignment::None },
    };
    let key = |k: &str| {
        let mut rtf = String::new();
        escape(&mut rtf, k);
        Cell { body: Body::Inline { rtf, chars: k.chars().count() }, header: true, align: Alignment::None }
    };
    let rows = match v {
        Value::Map(entries) if top => entries.iter().map(|(k, v)| Row { head: false, cells: vec![key(k), cell(v)] }).collect(),
        Value::Map(entries) => vec![
            Row { head: true, cells: entries.iter().map(|(k, _)| key(k)).collect() },
            Row { head: false, cells: entries.iter().map(|(_, v)| cell(v)).collect() },
        ],
        Value::Seq(items) => vec![Row { head: false, cells: items.iter().map(cell).collect() }],
        Value::Str(_) => vec![Row { head: false, cells: vec![cell(v)] }],
    };
    TableModel { align: Vec::new(), rows }
}

fn cell_natural(c: &Cell) -> i32 {
    2 * CELL_X
        + match &c.body {
            Body::Inline { chars, .. } => (*chars as i32 * CHAR_W * if c.header { 11 } else { 10 } / 10).max(CHAR_W),
            Body::Table(t) => col_naturals(t).iter().sum(),
        }
}

fn col_naturals(t: &TableModel) -> Vec<i32> {
    let cols = t.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0).max(t.align.len()).max(1);
    let mut w = vec![2 * CELL_X + CHAR_W; cols];
    for r in &t.rows {
        for (k, c) in r.cells.iter().enumerate() {
            w[k] = w[k].max(cell_natural(c));
        }
    }
    w
}

/// Column widths: each column as wide as its widest cell, as a browser does.
/// A table wider than `avail` shrinks: a column narrower than an equal share
/// of the free width keeps its width, and the others share the rest by their widths.
fn fit_columns(t: &TableModel, avail: i32) -> Vec<i32> {
    let nat = col_naturals(t);
    if nat.iter().sum::<i32>() <= avail {
        return nat;
    }
    let mut fixed = vec![false; nat.len()];
    loop {
        let free = (avail - nat.iter().zip(&fixed).filter(|(_, f)| **f).map(|(w, _)| w).sum::<i32>()).max(0);
        let flex: i32 = nat.iter().zip(&fixed).filter(|(_, f)| !**f).map(|(w, _)| w).sum();
        let count = fixed.iter().filter(|f| !**f).count() as i32;
        if count == 0 {
            return nat;
        }
        let mut changed = false;
        for (k, w) in nat.iter().enumerate() {
            if !fixed[k] && w * count <= free {
                fixed[k] = true;
                changed = true;
            }
        }
        if !changed {
            let min = 2 * CELL_X + 3 * CHAR_W;
            return nat.iter().zip(&fixed).map(|(w, f)| if *f { *w } else { (i64::from(free) * i64::from(*w) / i64::from(flex.max(1))) as i32 }.max(min.min(*w))).collect();
        }
    }
}

enum Seg<'t> {
    Text(&'t str),
    Link(String, &'t str),
}

/// GitHub's extended autolinks: `https://`, `http://` and `www.` URLs and
/// email addresses in text become links. Trailing punctuation and an unmatched
/// closing parenthesis stay outside the link.
fn autolinks(t: &str) -> Vec<Seg<'_>> {
    if !t.contains(['.', '@']) {
        return vec![Seg::Text(t)];
    }
    let b = t.as_bytes();
    let mut spans: Vec<(usize, usize, String)> = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let boundary = i == 0 || matches!(b[i - 1], b' ' | b'\t' | b'\n' | b'*' | b'_' | b'~' | b'(');
        if boundary {
            let rest = &t[i..];
            let prefix = ["https://", "http://", "www."].into_iter().find(|p| rest.len() > p.len() && rest.get(..p.len()).is_some_and(|r| r.eq_ignore_ascii_case(p)));
            if let Some(p) = prefix {
                let end = rest.find(|c: char| c.is_whitespace() || c == '<').unwrap_or(rest.len());
                let cand = trim_url(&rest[..end]);
                let host_from = if p == "www." { 0 } else { p.len() };
                let host = cand.get(host_from..).unwrap_or("").split(['/', '?', '#']).next().unwrap_or("");
                if cand.len() > p.len() && valid_domain(host) {
                    let url = if p == "www." { format!("http://{cand}") } else { cand.to_string() };
                    spans.push((i, i + cand.len(), url));
                    i += cand.len();
                    continue;
                }
            }
        }
        i += t[i..].chars().next().map_or(1, char::len_utf8);
    }
    let email_char = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'-' | b'_');
    for (at, _) in t.match_indices('@') {
        if spans.iter().any(|(s, e, _)| (*s..*e).contains(&at)) {
            continue;
        }
        let mut s = at;
        while s > 0 && email_char(b[s - 1]) {
            s -= 1;
        }
        let mut e = at + 1;
        while e < b.len() && (b[e].is_ascii_alphanumeric() || matches!(b[e], b'.' | b'-' | b'_')) {
            e += 1;
        }
        while e > at + 1 && b[e - 1] == b'.' {
            e -= 1;
        }
        let domain = &t[at + 1..e];
        let ok_end = domain.as_bytes().last().is_some_and(|c| c.is_ascii_alphanumeric());
        if s < at && domain.contains('.') && ok_end && (s == 0 || !b[s - 1].is_ascii_alphanumeric() && b[s - 1] != b'/') {
            spans.push((s, e, format!("mailto:{}", &t[s..e])));
        }
    }
    if spans.is_empty() {
        return vec![Seg::Text(t)];
    }
    spans.sort_by_key(|s| s.0);
    let mut out = Vec::new();
    let mut at = 0;
    for (s, e, url) in spans {
        if s < at {
            continue;
        }
        if s > at {
            out.push(Seg::Text(&t[at..s]));
        }
        out.push(Seg::Link(url, &t[s..e]));
        at = e;
    }
    if at < t.len() {
        out.push(Seg::Text(&t[at..]));
    }
    out
}

fn trim_url(mut s: &str) -> &str {
    while let Some(c) = s.chars().last() {
        let unmatched = c == ')' && s.matches('(').count() < s.matches(')').count();
        if "?!.,:*_~'\"".contains(c) || unmatched {
            s = &s[..s.len() - 1];
        } else {
            break;
        }
    }
    s
}

fn valid_domain(host: &str) -> bool {
    let host = host.split(':').next().unwrap_or("");
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_'))
        && parts.iter().rev().take(2).all(|p| !p.contains('_'))
}

/// `:name:` emoji codes as the emoji, from GitHub's list (assets/emoji.txt).
fn emoji(t: &str) -> Cow<'_, str> {
    if !t.contains(':') {
        return Cow::Borrowed(t);
    }
    static TABLE: std::sync::OnceLock<Vec<(&'static str, String)>> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        include_str!("../assets/emoji.txt")
            .lines()
            .filter_map(|l| {
                let (name, cps) = l.trim().split_once(' ')?;
                let s: Option<String> = cps.split('-').map(|h| u32::from_str_radix(h, 16).ok().and_then(char::from_u32)).collect();
                Some((name, s?))
            })
            .collect()
    });
    let mut out = String::new();
    let mut rest = t;
    let mut changed = false;
    while let Some(i) = rest.find(':') {
        let after = &rest[i + 1..];
        let len = after.bytes().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'+' | b'-')).count();
        if len > 0 && after.as_bytes().get(len) == Some(&b':') {
            if let Ok(k) = table.binary_search_by(|(n, _)| (*n).cmp(&after[..len])) {
                out.push_str(&rest[..i]);
                out.push_str(&table[k].1);
                rest = &after[len + 1..];
                changed = true;
                continue;
            }
        }
        out.push_str(&rest[..=i]);
        rest = after;
    }
    if !changed {
        return Cow::Borrowed(t);
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// CSS lower-roman: 1 to 3999, else decimal (a huge `<ol start>` must not build a huge string).
fn roman(mut n: u64) -> String {
    if n == 0 || n > 3999 {
        return n.to_string();
    }
    let mut s = String::new();
    for (v, r) in [(1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")] {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// CSS lower-alpha: 1 is a, 27 is aa; 0 is decimal.
fn alpha(mut n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push(b'a' + (n % 26) as u8);
        n /= 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// RTF for a Markdown document. Pure ASCII: everything else is a \u escape.
pub fn to_rtf(md: &str, c: &Colors, opts: &Options) -> String {
    let mut w = Writer::new(opts);
    let rgb = |v: u32| format!("\\red{}\\green{}\\blue{};", v & 0xFF, (v >> 8) & 0xFF, (v >> 16) & 0xFF);
    let mut table = String::from(";");
    for v in [c.text, c.muted, c.link, c.code_bg, c.pre_bg, c.border, c.border_muted, c.mark] {
        table.push_str(&rgb(v));
    }
    for (bar, _) in c.alerts {
        table.push_str(&rgb(bar));
    }
    for (_, title) in c.alerts {
        table.push_str(&rgb(title));
    }
    for v in c.syntax {
        table.push_str(&rgb(v));
    }
    w.out.push_str("{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0\\fswiss Segoe UI;}{\\f1\\fmodern Consolas;}}");
    w.out.push_str(&format!("{{\\colortbl{table}}}\n"));
    let (meta, body) = front_matter(md);
    if let Some((text, yaml)) = meta.filter(|(t, _)| !t.is_empty()) {
        match yaml.then(|| frontmatter::parse(text)).flatten() {
            Some(v) => w.front_matter(&v),
            // TOML, or YAML that the subset cannot read: its text.
            None => w.code_block(&text.replace('\r', ""), if yaml { "yaml" } else { "toml" }),
        }
    }
    let po = ParseOptions::ENABLE_TABLES | ParseOptions::ENABLE_TASKLISTS | ParseOptions::ENABLE_STRIKETHROUGH | ParseOptions::ENABLE_FOOTNOTES | ParseOptions::ENABLE_GFM;
    for ev in TextMergeStream::new(Parser::new_ext(body, po)) {
        match ev {
            Event::Start(tag) => w.start(tag),
            Event::End(tag) => w.finish(tag),
            Event::Text(t) => w.text(&t),
            Event::Code(t) => w.inline_code(&t),
            Event::Html(t) | Event::InlineHtml(t) => w.html(&t),
            Event::SoftBreak => w.text(" "),
            Event::HardBreak => {
                w.flush();
                w.out.push_str("\\line ");
            }
            Event::Rule => {
                w.end();
                w.rule();
            }
            Event::TaskListMarker(done) => {
                let mut m = String::from("\\tab ");
                escape(&mut m, if done { "\u{2611}" } else { "\u{2610}" });
                m.push_str("\\tab ");
                w.marker = Some(m);
            }
            Event::FootnoteReference(label) => w.note_ref(&label),
            _ => {}
        }
    }
    w.html_flush();
    // Close what the document left open. A `<pre>` with no `</pre>` takes the rest of the file.
    if w.html_pre {
        w.html_close("pre");
    }
    w.end();
    while w.details.last().is_some() {
        w.details_close();
    }
    while !w.frames.is_empty() {
        w.close_box(0, 0);
    }
    w.footnotes();
    w.out.push('}');
    w.out
}

/// Split YAML (`---` … `---` or `...`) or TOML (`+++` … `+++`) front matter off
/// the top of the file: ((its lines, is YAML), the rest). Only at the very top,
/// and the line after the opener must not be blank, so a file that starts with
/// a rule keeps its body. pulldown-cmark's metadata option would also take a
/// `---` rule pair later in the file, so it stays off.
fn front_matter(md: &str) -> (Option<(&str, bool)>, &str) {
    let src = md.strip_prefix('\u{FEFF}').unwrap_or(md);
    let mut lines = src.split_inclusive('\n');
    let open = lines.next().map(|l| l.trim_end()).unwrap_or("");
    let (close, yaml): (&[&str], bool) = match open {
        "---" => (&["---", "..."], true),
        "+++" => (&["+++"], false),
        _ => return (None, md),
    };
    let start = src.find('\n').map_or(src.len(), |i| i + 1);
    let mut at = start;
    for (i, line) in lines.enumerate() {
        let t = line.trim_end();
        if i == 0 && t.is_empty() {
            break;
        }
        if close.contains(&t) {
            return (Some((src[start..at].trim_end(), yaml)), &src[at + line.len()..]);
        }
        at += line.len();
    }
    (None, md)
}

fn escape(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '\\' | '{' | '}' => {
                out.push('\\');
                out.push(ch);
            }
            '\t' => out.push_str("\\tab "),
            '\n' | '\r' => out.push(' '),
            ' '..='~' => out.push(ch),
            _ => {
                let mut units = [0u16; 2];
                for u in ch.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{}?", *u as i16));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const C: Colors = Colors {
        text: 0xFCF6F0,
        muted: 0xA19891,
        link: 0xF89344,
        code_bg: 0x2A231F,
        pre_bg: 0x231B15,
        border: 0x4D443D,
        border_muted: 0x3D352F,
        mark: 0x152227,
        alerts: [(1, 2), (3, 4), (5, 6), (7, 8), (9, 10)],
        syntax: [11, 12, 13, 14, 15, 16, 17, 18, 19],
    };

    fn no_picture(_: &str) -> Option<Picture> {
        None
    }

    fn rtf(md: &str) -> String {
        to_rtf(md, &C, &Options { width: 9000, toggled_details: &[], picture: &no_picture, code_char: 110.0 })
    }

    #[test]
    fn markdown_extensions_are_recognised_in_any_case() {
        assert!(is_markdown(r"C:\notes\README.md"));
        assert!(is_markdown("a.MARKDOWN"));
        assert!(!is_markdown("a.txt"));
        assert!(!is_markdown("md"));
    }

    #[test]
    fn output_is_one_ascii_rtf_group_with_balanced_braces() {
        let r = rtf("# H\u{e9}llo \u{1F600}\n\n> [!NOTE]\n> x\n\n| a |\n|---|\n| b |\n\n```rust\nfn x() {}\n```\n\nText[^1]\n\n[^1]: note");
        assert!(r.starts_with("{\\rtf1") && r.ends_with('}'));
        assert!(r.is_ascii());
        assert!(r.contains("H\\u233?llo"));
        assert!(r.contains("\\u-10179?\\u-8704?"), "a surrogate pair is two escapes");
        let mut depth = 0i32;
        let b = r.as_bytes();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'\\' => i += 1,
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            assert!(depth > 0 || i + 1 >= b.len(), "the group closed early at {i}");
            i += 1;
        }
        assert_eq!(depth, 0);
    }

    #[test]
    fn rtf_control_characters_in_text_are_escaped() {
        assert!(rtf("a \\{b} c").contains("a \\{b\\} c"));
        assert!(rtf("`{x}`").contains("\\{x\\}"));
    }

    #[test]
    fn body_text_has_githubs_size_and_line_height() {
        assert!(rtf("text").contains("\\sl360\\slmult0\\plain\\f0\\fs24\\cf1 text\\par"));
    }

    #[test]
    fn h1_and_h2_are_underlined_rows_and_h3_is_not() {
        let r = rtf("# One\n\n### Three");
        assert!(r.contains("\\clbrdrb\\brdrs\\brdrw15\\brdrcf7\\cellx9000"), "{r}");
        assert!(r.contains("\\fs48\\cf1\\b One"), "{r}");
        assert!(r.contains("\\fs30\\cf1\\b Three\\par"), "{r}");
        assert_eq!(r.matches("\\row").count(), 1);
    }

    #[test]
    fn paragraphs_are_apart_by_one_spacer_of_16px() {
        let r = rtf("a\n\nb");
        assert!(r.contains("a\\par\n\\pard\\plain\\fs2\\sl-240\\slmult0\\par\n"), "{r}");
    }

    #[test]
    fn inline_styles_nest_as_groups() {
        assert!(rtf("**bold *both***").contains("{\\b bold {\\i both}}"));
        assert!(rtf("~gone~").contains("{\\strike gone}"), "one tilde, as on GitHub");
    }

    #[test]
    fn links_are_colored_fields_without_underline() {
        let r = rtf("[the spec](docs/SPEC.md)");
        assert!(r.contains("{\\field{\\*\\fldinst{HYPERLINK \"docs/SPEC.md\"}}{\\fldrslt{\\cf3 the spec}}}"), "{r}");
    }

    #[test]
    fn a_quote_in_a_link_url_cannot_end_the_field() {
        assert!(rtf("[x](<a\"b.md>)").contains("HYPERLINK \"a%22b.md\""));
    }

    #[test]
    fn bare_urls_and_emails_become_links() {
        let r = rtf("see www.example.com/a, https://x.org/b_(c)). or me@mail.example.com.");
        assert!(r.contains("HYPERLINK \"http://www.example.com/a\"}}{\\fldrslt{\\cf3 www.example.com/a}}},"), "{r}");
        assert!(r.contains("HYPERLINK \"https://x.org/b_(c)\""), "{r}");
        assert!(r.contains("HYPERLINK \"mailto:me@mail.example.com\""), "{r}");
        assert!(!rtf("[www.a.com](x.md)").contains("http://www"), "no link in a link");
    }

    #[test]
    fn emoji_codes_become_emoji() {
        assert!(rtf("go :rocket: now").contains("go \\u-10179?\\u-8576? now"));
        assert!(rtf("time 10:30:00 and :nope:").contains("10:30:00 and :nope:"));
    }

    #[test]
    fn raw_html_keeps_what_githubs_sanitizer_keeps() {
        let r = rtf("a <b>bold</b> <kbd>K</kbd> <span>x</span> <u>u</u>");
        assert!(r.contains("{\\b bold}"), "{r}");
        assert!(r.contains("{\\f1\\fs17\\highlight5 K}"), "{r}");
        assert!(r.contains(" x "), "an unknown tag goes, its text stays: {r}");
        assert!(!r.contains("<span>"), "{r}");
    }

    #[test]
    fn filtered_tags_show_as_text_and_comments_go() {
        let r = rtf("<script>alert(1)</script>\n\nx <!-- hidden --> y");
        assert!(r.contains("<script>alert(1)</script>"), "{r}");
        assert!(!r.contains("hidden"), "{r}");
    }

    #[test]
    fn html_align_centers_the_paragraph() {
        let r = rtf("<p align=\"center\">\nMiddle\n</p>");
        assert!(r.contains("\\qc\\li0"), "{r}");
        assert!(r.contains("Middle"), "{r}");
    }

    #[test]
    fn closed_details_show_only_the_summary_as_a_toggle() {
        let md = "<details>\n<summary>More</summary>\n\nHidden text\n\n</details>\n\nAfter";
        let r = rtf(md);
        assert!(r.contains("HYPERLINK \"details:0\""), "{r}");
        assert!(r.contains("More"), "{r}");
        assert!(!r.contains("Hidden"), "{r}");
        assert!(r.contains("After"), "{r}");
        let open = to_rtf(md, &C, &Options { width: 9000, toggled_details: &[0], picture: &no_picture, code_char: 110.0 });
        assert!(open.contains("Hidden text"), "{open}");
        assert_eq!(resolve("details:0", None), Target::Details(0));
    }

    #[test]
    fn details_in_a_table_cell_do_not_close_the_block_around_the_table() {
        let md = "<details>
<summary>More</summary>

| a |
|---|
| <details><summary>x</summary>y</details> |

Hidden after

</details>

After";
        let r = rtf(md);
        assert!(!r.contains("Hidden"), "{r}");
        assert!(r.contains("After"), "{r}");
    }

    #[test]
    fn details_with_the_open_attribute_start_open_and_toggle_closed() {
        let md = "<details open>
<summary>More</summary>

Shown text

</details>";
        assert!(rtf(md).contains("Shown text"));
        let toggled = to_rtf(md, &C, &Options { width: 9000, toggled_details: &[0], picture: &no_picture, code_char: 110.0 });
        assert!(!toggled.contains("Shown"), "{toggled}");
    }

    #[test]
    fn missing_images_show_their_alt_text() {
        assert!(rtf("![Logo](https://x.y/a.png)").contains(" Logo}"));
        assert!(rtf("![`icon` *big*](a.png)").contains(" icon big}"));
    }

    #[test]
    fn a_loaded_picture_is_scaled_to_fit() {
        let pic = |_: &str| Some(Picture { png: true, width: 2000, height: 1000, hex: "00".into() });
        let r = to_rtf("![x](a.png)", &C, &Options { width: 9000, toggled_details: &[], picture: &pic, code_char: 110.0 });
        assert!(r.contains("{\\pict\\pngblip\\picw2000\\pich1000\\picwgoal9000\\pichgoal4500 00}"), "{r}");
    }

    #[test]
    fn picture_info_reads_png_and_jpeg_sizes() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&[0, 0, 1, 0, 0, 0, 0, 0x80]);
        assert_eq!(picture_info(&png), Some((true, 256, 128)));
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 11, 8, 0, 50, 0, 100, 3, 0, 0];
        assert_eq!(picture_info(&jpeg), Some((false, 100, 50)));
        assert_eq!(picture_info(b"GIF89a"), None);
    }

    #[test]
    fn bullet_markers_are_right_aligned_before_the_text() {
        let r = rtf("- one\n- two");
        assert!(r.contains("\\li480\\sl360\\slmult0\\fi-480\\tqr\\tx360\\tx480"), "{r}");
        assert!(r.contains("\\tab \\u8226?\\tab one\\par"), "{r}");
        assert!(r.contains("\\sl-60\\slmult0"), "4px between items: {r}");
    }

    #[test]
    fn nested_ordered_lists_go_roman_then_alpha() {
        let r = rtf("1. a\n   1. b\n      1. c");
        assert!(r.contains("\\tab 1.\\tab a"), "{r}");
        assert!(r.contains("\\tab i.\\tab b"), "{r}");
        assert!(r.contains("\\tab a.\\tab c"), "{r}");
    }

    #[test]
    fn a_loose_item_starts_one_paragraph_not_two() {
        let r = rtf("- one\n\n- two");
        assert!(r.contains("\\tab \\u8226?\\tab one\\par"), "{r}");
    }

    #[test]
    fn task_items_show_a_checkbox_instead_of_a_bullet() {
        let r = rtf("- [x] done\n- [ ] todo");
        assert!(r.contains("\\u9745?\\tab done"), "{r}");
        assert!(!r.contains("\\u8226?"), "{r}");
    }

    #[test]
    fn code_blocks_are_highlighted_boxes_with_syntax_colors() {
        let r = rtf("```rust\nfn main() {\n    x\n}\n```");
        assert!(r.contains("\\plain\\f0\\fs20\\cf1\\f1\\highlight5 "), "{r}");
        // 9000 twips at 110 a column: 80 columns, each line padded to them.
        let first = format!("\\line   {{\\cf20 fn}} {{\\cf23 main}}() \\{{{}\\u160?", " ".repeat(80 - 14));
        assert!(r.contains(&first), "{r}");
        assert!(r.contains(&format!("\\line       x{}\\u160?", " ".repeat(80 - 8))), "{r}");
    }

    #[test]
    fn a_long_code_line_wraps_inside_the_box() {
        let r = rtf(&format!("```\n{}\n```", "x".repeat(100)));
        assert!(r.contains(&format!("\\line   {} \\u160?\\line   {}", "x".repeat(76), "x".repeat(24))), "{r}");
    }

    #[test]
    fn quotes_have_a_bar_and_muted_text() {
        let r = rtf("> said");
        assert!(r.contains("\\clbrdrl\\brdrs\\brdrw60\\brdrcf6"), "{r}");
        assert!(r.contains("\\cf2 said\\cell"), "{r}");
    }

    #[test]
    fn alerts_have_a_colored_bar_and_title() {
        let r = rtf("> [!WARNING]\n> Careful");
        assert!(r.contains("\\brdrcf12\\cellx"), "{r}");
        assert!(r.contains("\\cf17\\b \\u9888?\\u-498?  Warning\\par"), "{r}");
        assert!(r.contains("Careful"), "{r}");
    }

    #[test]
    fn a_box_in_a_box_is_a_nested_table() {
        let r = rtf("> outer\n> > inner");
        assert!(r.contains("\\intbl\\itap2"), "{r}");
        assert!(r.contains("\\nestcell"), "{r}");
        assert!(r.contains("\\nestrow}{\\nonesttables\\par}"), "{r}");
    }

    #[test]
    fn tables_have_bordered_cells_and_a_bold_header() {
        let r = rtf("| A | Bee |\n|:-:|--:|\n| 1 | 2 |\n| 3 | 4 |");
        assert_eq!(r.matches("\\row").count(), 3);
        assert!(r.contains("\\qc\\sl360\\slmult0\\plain\\f0\\fs24\\cf1\\b A"), "{r}");
        assert!(r.contains("\\qr\\sl360\\slmult0\\plain\\f0\\fs24\\cf1 2"), "{r}");
        assert!(r.contains("\\clbrdrr\\brdrs\\brdrw15\\brdrcf6\\cellx"), "{r}");
        assert!(!r.contains("\\clcfpat"), "no shaded cell: RichEdit would drop link and code colors: {r}");
    }

    #[test]
    fn a_wide_table_shrinks_to_the_width() {
        let long = "word ".repeat(80);
        let r = rtf(&format!("| a | b |\n|---|---|\n| x | {long} |"));
        let last = r.match_indices("\\cellx").map(|(i, _)| r[i + 6..].split(|c: char| !c.is_ascii_digit()).next().unwrap().parse::<i32>().unwrap()).max().unwrap();
        assert!(last <= 9000, "{last}");
    }

    #[test]
    fn front_matter_shows_as_a_table_of_keys_and_values() {
        let r = rtf("---\ntitle: Notes\ntags: [a, b]\nauthor:\n  name: Ann\n---\n\n# Body");
        assert!(r.contains("\\b title\\par"), "{r}");
        assert!(r.contains("Notes\\par"), "{r}");
        assert!(r.contains("\\itap2"), "a list or map is a table in its cell: {r}");
        assert!(r.contains("\\itap2\\sl360\\slmult0\\plain\\f0\\fs24\\cf1\\b name\\par"), "{r}");
        assert!(r.contains("\\b Body"), "{r}");
    }

    #[test]
    fn toml_or_unreadable_front_matter_shows_as_code() {
        let r = rtf("+++\r\ntitle = \"Notes\"\r\n+++\r\n\r\n# Body");
        assert!(r.contains("\\highlight5"), "{r}");
        assert!(r.contains("title"), "{r}");
        assert!(rtf("---\na: &x 1\n---\n").contains("\\f1"), "an anchor: code");
    }

    #[test]
    fn empty_front_matter_shows_nothing() {
        let r = rtf("---\n---\n# Body");
        assert_eq!(r.matches("\\row").count(), 1, "the h1 only: {r}");
    }

    #[test]
    fn a_non_ascii_word_before_a_dot_does_not_panic() {
        assert!(rtf("Caf\u{e9}. x").contains("Caf\\u233?. x"));
        assert!(rtf("\u{4e2d}\u{6587}. www.example.com").contains("HYPERLINK \"http://www.example.com\""));
    }

    #[test]
    fn nested_ordered_lists_that_start_at_0_or_past_3999_use_numbers() {
        let r0 = rtf("1. a\n\n   0. b");
        assert!(r0.contains("\\tab 0.\\tab "), "{r0}");
        let r = rtf("<ol><li>a<ol start=\"18446744073709551615\"><li>b</li><li>c</li></ol></li></ol>");
        assert!(r.contains("\\tab 18446744073709551615.\\tab "), "{r}");
    }

    #[test]
    fn a_pre_with_no_end_tag_shows_the_rest_of_the_file() {
        assert!(rtf("<pre>\nline one\n\nline two").contains("line two"));
    }

    #[test]
    fn huge_picture_sizes_do_not_overflow() {
        let pic = |_: &str| Some(Picture { png: true, width: u32::MAX, height: u32::MAX, hex: "00".into() });
        let opts = Options { width: 9000, toggled_details: &[], picture: &pic, code_char: 110.0 };
        let r = to_rtf("<img src=a.png height=2147483647>", &C, &opts);
        assert!(r.contains("\\picwgoal9000"), "{r}");
    }

    #[test]
    fn dashes_that_are_not_front_matter_stay_rules() {
        for md in ["text\n\n---\nnot: meta\n---\n", "---\ntitle: open", "---\n\ntitle: x\n---\n"] {
            assert!(rtf(md).contains("\\brdrw60\\brdrcf6"), "{md}");
        }
    }

    #[test]
    fn footnotes_go_to_the_end_in_reference_order() {
        let r = rtf("A[^b] B[^a]\n\n[^a]: first defined\n[^b]: first used");
        assert!(r.contains("{\\super\\cf3 1}"), "{r}");
        let used = r.find("first used").unwrap();
        let defined = r.find("first defined").unwrap();
        assert!(used < defined, "{r}");
        assert!(r.contains("\\tab 1.\\tab first used {\\cf3 \\u8617?}"), "{r}");
        assert!(r.find("\\clbrdrt").unwrap() < used, "{r}");
    }

    #[test]
    fn link_target_reads_the_url_from_a_field_range() {
        assert_eq!(link_target("HYPERLINK \"https://example.com/a?b=1\"the spec"), Some("https://example.com/a?b=1"));
        assert_eq!(link_target("docs/a.md"), Some("docs/a.md"));
        assert_eq!(link_target(""), None);
    }

    #[test]
    fn web_links_open_in_the_browser() {
        assert_eq!(resolve("https://x.y/z", None), Target::Web("https://x.y/z".into()));
        assert_eq!(resolve("HTTP://x.y", None), Target::Web("HTTP://x.y".into()));
    }

    #[test]
    fn other_schemes_do_nothing() {
        for url in ["mailto:a@b.c", "file:///C:/x.md", "javascript:alert(1)", "ms-settings:display", "#intro"] {
            assert_eq!(resolve(url, Some(Path::new(r"C:\d"))), Target::Nothing, "{url}");
        }
    }

    #[test]
    fn relative_markdown_links_resolve_against_the_file() {
        assert_eq!(resolve("docs/My%20Notes.md#top", Some(Path::new(r"C:\repo"))), Target::File(PathBuf::from(r"C:\repo\docs/My Notes.md")));
        assert_eq!(resolve("C:/x/y.md", None), Target::File(PathBuf::from("C:/x/y.md")));
    }

    #[test]
    fn unc_links_do_nothing() {
        for url in ["//host/share/a.md", r"\\host\share\a.md", "%5C%5Chost/a.md", r"/\host/a.md"] {
            assert_eq!(resolve(url, Some(Path::new(r"C:\d"))), Target::Nothing, "{url}");
        }
        assert_eq!(resolve("a.md", Some(Path::new(r"\\host\share"))), Target::File(PathBuf::from(r"\\host\share\a.md")));
    }

    #[test]
    fn non_markdown_or_unsaved_relative_links_do_nothing() {
        assert_eq!(resolve("setup.exe", Some(Path::new(r"C:\d"))), Target::Nothing);
        assert_eq!(resolve("a.md", None), Target::Nothing);
    }
}

#[cfg(test)]
mod dump {
    /// `RTF_DUMP=in.md cargo test dump -- --ignored`: writes in.md.rtf, for a look in RichEdit.
    #[test]
    #[ignore]
    fn dump() {
        let Ok(p) = std::env::var("RTF_DUMP") else { return };
        let md = std::fs::read_to_string(&p).unwrap();
        let none = |_: &str| None;
        let r = super::to_rtf(&md, &super::tests::C, &super::Options { width: 9000, toggled_details: &[], picture: &none, code_char: 110.0 });
        std::fs::write(format!("{p}.rtf"), r).unwrap();
    }
}
