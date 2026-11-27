//! Markdown preview, the pure part: CommonMark plus GitHub's tables, task lists
//! and strikethrough, turned into RTF for a read-only RichEdit control
//! (preview.rs). Raw HTML shows as text, images as a labelled placeholder, and
//! nothing here reads a file or the network.

use pulldown_cmark::{Alignment, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::path::{Path, PathBuf};

/// Preview colors as COLORREF (0x00BBGGRR).
pub struct Colors {
    pub text: u32,
    pub muted: u32,
    pub link: u32,
    /// Behind inline code, code blocks and table headers.
    pub code_bg: u32,
    /// Heading underlines, the horizontal rule, quote bars and table borders.
    pub rule: u32,
}

// Color table entries, in the order `to_rtf` writes them.
const TEXT: u8 = 1;
const MUTED: u8 = 2;
const LINK: u8 = 3;
const CODE_BG: u8 = 4;
const RULE: u8 = 5;

// Sizes in half-points, spacing in twips (1/20 pt).
const BODY: u32 = 22;
const CODE: u32 = 19;
const GAP: u32 = 160;
const INDENT: i32 = 360;

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
    /// Anything else (mailto:, file:, #anchors, other files) does nothing.
    Nothing,
}

pub fn resolve(url: &str, doc_dir: Option<&Path>) -> Target {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return Target::Web(url.to_string());
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
    if path.chars().take(2).filter(|&c| c == '/' || c == '\\').count() == 2 {
        return Target::Nothing;
    }
    let p = Path::new(&path);
    match doc_dir {
        _ if p.is_absolute() => Target::File(p.to_path_buf()),
        Some(dir) => Target::File(dir.join(p)),
        None => Target::Nothing,
    }
}

fn percent_decode(s: &str) -> String {
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

struct Cell {
    rtf: String,
    /// Shown characters, for the column width.
    chars: usize,
}

#[derive(Default)]
struct Table {
    align: Vec<Alignment>,
    rows: Vec<(bool, Vec<Cell>)>,
}

#[derive(Default)]
struct Writer {
    out: String,
    /// One entry per open list: the next number, or None for bullets.
    lists: Vec<Option<u64>>,
    quote: usize,
    /// Paragraph properties not written yet. They are written with the first
    /// text, so a loose list item's own paragraph does not start a second one.
    pending: Option<String>,
    /// A paragraph has been started (its properties are written).
    open: bool,
    /// Where the open paragraph starts in `out`.
    para_at: usize,
    /// List marker waiting for the item's first text.
    marker: Option<String>,
    /// Inside a code block or raw HTML block: its text, written at the end.
    code: Option<String>,
    table: Option<Table>,
    /// Inside a table cell: the output that the cell interrupted, and the cell's shown length.
    cell: Option<(String, usize)>,
    /// Inside an image: whether its alt text has started.
    image: Option<bool>,
    /// Inside a heading: its font size, for inline code in it.
    heading: Option<u32>,
}

impl Writer {
    fn left(&self) -> i32 {
        (self.quote + self.lists.len()) as i32 * INDENT
    }

    fn color(&self) -> u8 {
        if self.quote > 0 { MUTED } else { TEXT }
    }

    /// Start a paragraph; its properties are written with its first text.
    fn begin(&mut self, props: String) {
        self.end();
        self.pending = Some(props);
    }

    // RichEdit ignores paragraph borders and shading (\brdr*, \cbpat), and
    // percent-width tables stop it painting at all. So quotes get no bar, code
    // blocks get a highlight behind the text only, and the rule is a fixed-width table row.
    fn para(&mut self) {
        let (left, color) = (self.left(), self.color());
        self.begin(format!("\\pard\\sa{GAP}\\li{left}\\plain\\f0\\fs{BODY}\\cf{color} "));
    }

    fn flush(&mut self) {
        if let Some(p) = self.pending.take() {
            self.para_at = self.out.len();
            self.out.push_str(&p);
            self.open = true;
        }
        if let Some(m) = self.marker.take() {
            self.out.push_str(&m);
        }
        // Before the alt text's first text or style group, so the group does not take it.
        if self.image == Some(false) {
            self.image = Some(true);
            self.out.push_str(": ");
        }
    }

    fn end(&mut self) {
        if self.pending.is_some() || self.marker.is_some() {
            self.flush();
        }
        if self.open {
            self.out.push_str("\\par\n");
            self.open = false;
            // RichEdit stretches a highlight on the last line down through the
            // space after the paragraph, and on the first line up through the
            // space before it (headings). Move that space to 1-point paragraphs.
            if self.out[self.para_at..].contains("\\highlight") {
                // Only in the properties (up to the first space), never in the text.
                // \sa first: it comes after \sb, so its position stays right.
                if let Some(sa) = self.zero_space("\\sa") {
                    self.out.push_str(&format!("\\pard\\plain\\fs2\\sa{}\\par\n", sa - 20));
                }
                if let Some(sb) = self.zero_space("\\sb") {
                    self.out.insert_str(self.para_at, &format!("\\pard\\plain\\fs2\\sa{}\\par\n", sb - 20));
                }
            }
        }
    }

    /// Set the open paragraph's `word` space (\sa or \sb) to 0 when it is over
    /// 1 point. Returns the old value in twips.
    fn zero_space(&mut self, word: &str) -> Option<u32> {
        let para = &self.out[self.para_at..];
        let props = &para[..para.find(' ').unwrap_or(para.len())];
        let i = props.find(word)? + word.len();
        let digits = props[i..].bytes().take_while(u8::is_ascii_digit).count();
        let v: u32 = props[i..i + digits].parse().ok().filter(|&v| v > 20)?;
        let at = self.para_at + i;
        self.out.replace_range(at..at + digits, "0");
        Some(v)
    }

    fn text(&mut self, t: &str) {
        if let Some(code) = &mut self.code {
            code.push_str(t);
            return;
        }
        self.flush();
        if let Some((_, chars)) = &mut self.cell {
            *chars += t.chars().count();
        }
        escape(&mut self.out, t);
    }

    fn raw(&mut self, s: &str) {
        self.flush();
        self.out.push_str(s);
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph if self.pending.is_none() => self.para(),
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                let (size, before) = match level {
                    HeadingLevel::H1 => (40, 120),
                    HeadingLevel::H2 => (32, 240),
                    HeadingLevel::H3 => (28, 200),
                    HeadingLevel::H4 => (24, 160),
                    _ => (BODY, 160),
                };
                let color = if level == HeadingLevel::H6 { MUTED } else { self.color() };
                let left = self.left();
                // The first block of a list item (`- # Title`): it takes the item's
                // marker and hanging indent, so the bullet does not stand alone.
                let marker = self.marker.take();
                let hang = if marker.is_some() {
                    self.pending = None;
                    format!("\\fi-{INDENT}\\tx{left}")
                } else {
                    String::new()
                };
                self.begin(format!("\\pard\\sb{before}\\sa120\\li{left}{hang}\\plain\\f0\\fs{size}\\cf{color}\\b "));
                self.marker = marker;
                self.heading = Some(size);
            }
            Tag::BlockQuote(_) => {
                self.end();
                self.quote += 1;
            }
            Tag::CodeBlock(_) | Tag::HtmlBlock => {
                self.end();
                self.code = Some(String::new());
            }
            Tag::List(first) => {
                self.end();
                self.lists.push(first);
            }
            Tag::Item => {
                let depth = self.lists.len();
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        format!("{}.", *n - 1)
                    }
                    _ => ["\u{2022}", "\u{25E6}", "\u{25AA}"][(depth.max(1) - 1) % 3].to_string(),
                };
                self.para();
                let left = self.left();
                if let Some(p) = &mut self.pending {
                    *p = p.replacen(&format!("\\li{left}"), &format!("\\li{left}\\fi-{INDENT}\\tx{left}"), 1);
                }
                let mut m = String::new();
                escape(&mut m, &marker);
                self.marker = Some(m + "\\tab ");
            }
            Tag::Emphasis => self.raw("{\\i "),
            Tag::Strong => self.raw("{\\b "),
            Tag::Strikethrough => self.raw("{\\strike "),
            Tag::Link { dest_url, .. } => {
                // A quote would end the field instruction; a backslash would start a switch.
                let url = dest_url.replace('"', "%22").replace('\\', "/");
                let mut f = String::from("{\\field{\\*\\fldinst{HYPERLINK \"");
                escape(&mut f, &url);
                f.push_str(&format!("\"}}}}{{\\fldrslt{{\\cf{LINK}\\ul "));
                self.raw(&f);
            }
            Tag::Image { .. } => {
                self.raw(&format!("{{\\cf{MUTED} [Image"));
                self.image = Some(false);
            }
            Tag::Table(align) => {
                self.end();
                self.table = Some(Table { align, rows: Vec::new() });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.rows.push((matches!(tag, Tag::TableHead), Vec::new()));
                }
            }
            Tag::TableCell => self.cell = Some((std::mem::take(&mut self.out), 0)),
            _ => {}
        }
    }

    fn finish(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Item => self.end(),
            TagEnd::Heading(_) => {
                self.heading = None;
                self.end();
            }
            TagEnd::BlockQuote(_) => {
                self.end();
                self.quote -= 1;
            }
            TagEnd::CodeBlock | TagEnd::HtmlBlock => {
                let code = self.code.take().unwrap_or_default();
                let left = self.left() + INDENT / 2;
                self.begin(format!("\\pard\\sa{GAP}\\li{left}\\ri{}\\plain\\f1\\fs{CODE}\\cf{TEXT}\\highlight{CODE_BG} ", INDENT / 2));
                self.flush();
                for (i, line) in code.trim_end_matches('\n').split('\n').enumerate() {
                    if i > 0 {
                        self.out.push_str("\\line ");
                    }
                    escape(&mut self.out, line);
                }
                self.end();
            }
            TagEnd::List(_) => {
                self.end();
                self.lists.pop();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.out.push('}'),
            TagEnd::Link => self.out.push_str("}}}"),
            TagEnd::Image => {
                self.image = None;
                self.out.push_str("]}");
            }
            TagEnd::TableCell => {
                if let Some((before, chars)) = self.cell.take() {
                    let rtf = std::mem::replace(&mut self.out, before);
                    if let Some((_, row)) = self.table.as_mut().and_then(|t| t.rows.last_mut()) {
                        row.push(Cell { rtf, chars });
                    }
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.write_table(t);
                }
            }
            _ => {}
        }
    }

    /// Ceiling: columns get fixed widths from their longest cell, so a table
    /// wider than the window runs past its right edge (RichEdit tables don't reflow).
    fn write_table(&mut self, t: Table) {
        let cols = t.align.len().max(t.rows.iter().map(|r| r.1.len()).max().unwrap_or(0));
        let mut widths = vec![0usize; cols];
        for (_, row) in &t.rows {
            for (w, c) in widths.iter_mut().zip(row) {
                *w = (*w).max(c.chars);
            }
        }
        let left = self.left();
        let border = format!("\\brdrs\\brdrw10\\brdrcf{RULE}");
        for (head, row) in &t.rows {
            let mut x = left;
            self.out.push_str(&format!("\\trowd\\trgaph100\\trleft{left}"));
            for w in &widths {
                x += (*w as i32 * 115 + 300).clamp(800, 4800);
                let shade = if *head { format!("\\clcbpat{CODE_BG}") } else { String::new() };
                self.out.push_str(&format!("\\clbrdrt{border}\\clbrdrl{border}\\clbrdrb{border}\\clbrdrr{border}{shade}\\cellx{x}"));
            }
            self.out.push('\n');
            for k in 0..cols {
                let align = match t.align.get(k) {
                    Some(Alignment::Center) => "\\qc",
                    Some(Alignment::Right) => "\\qr",
                    _ => "\\ql",
                };
                let bold = if *head { "\\b" } else { "" };
                let text = row.get(k).map_or("", |c| c.rtf.as_str());
                self.out.push_str(&format!("\\pard\\intbl{align}\\plain\\f0\\fs{BODY}\\cf{TEXT}{bold} {text}\\cell"));
            }
            self.out.push_str("\\row\n");
        }
        // Space after the table, as after a paragraph.
        self.out.push_str(&format!("\\pard\\plain\\fs2\\sa{GAP}\\par\n"));
    }
}

/// RTF for a Markdown document. Pure ASCII: everything else is a \u escape.
pub fn to_rtf(md: &str, c: &Colors) -> String {
    let mut w = Writer::default();
    let rgb = |v: u32| format!("\\red{}\\green{}\\blue{};", v & 0xFF, (v >> 8) & 0xFF, (v >> 16) & 0xFF);
    w.out.push_str("{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0\\fswiss Segoe UI;}{\\f1\\fmodern Consolas;}}");
    w.out.push_str(&format!("{{\\colortbl;{}{}{}{}{}}}\n", rgb(c.text), rgb(c.muted), rgb(c.link), rgb(c.code_bg), rgb(c.rule)));
    let (meta, md) = front_matter(md);
    if let Some(meta) = meta.filter(|m| !m.is_empty()) {
        w.start(Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Indented));
        w.text(&meta.replace('\r', ""));
        w.finish(TagEnd::CodeBlock);
    }
    let opts = Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    for ev in Parser::new_ext(md, opts) {
        match ev {
            Event::Start(tag) => w.start(tag),
            Event::End(tag) => w.finish(tag),
            Event::Text(t) => w.text(&t),
            Event::Code(t) => {
                // In a heading, a body-size highlight box looks tall beside the big letters.
                let size = w.heading.map_or(CODE + 1, |s| s * 9 / 10);
                w.raw(&format!("{{\\f1\\fs{size}\\highlight{CODE_BG} "));
                w.text(&t);
                w.out.push('}');
            }
            Event::Html(t) | Event::InlineHtml(t) => w.text(&t),
            Event::SoftBreak => w.text(" "),
            Event::HardBreak => w.raw("\\line "),
            Event::Rule => {
                // A centered 3-inch row with a bottom border: fits any sensible window.
                w.end();
                w.out.push_str(&format!("\\trowd\\trqc\\trgaph0\\clbrdrb\\brdrs\\brdrw10\\brdrcf{RULE}\\cellx4320\n\\pard\\intbl\\plain\\fs8\\cell\\row\n"));
                w.out.push_str(&format!("\\pard\\plain\\fs2\\sa{GAP}\\par\n"));
            }
            Event::TaskListMarker(done) => w.marker = Some(if done { "\\u9745?\\tab " } else { "\\u9744?\\tab " }.to_string()),
            _ => {}
        }
    }
    w.end();
    w.out.push('}');
    w.out
}

/// Split YAML (`---` … `---` or `...`) or TOML (`+++` … `+++`) front matter off
/// the top of the file: (its lines, the rest). Only at the very top, and the
/// line after the opener must not be blank, so a file that starts with a rule
/// keeps its body. pulldown-cmark's metadata option would also take a `---`
/// rule pair later in the file, so it stays off.
fn front_matter(md: &str) -> (Option<&str>, &str) {
    let src = md.strip_prefix('\u{FEFF}').unwrap_or(md);
    let mut lines = src.split_inclusive('\n');
    let open = lines.next().map(|l| l.trim_end()).unwrap_or("");
    let close: &[&str] = match open {
        "---" => &["---", "..."],
        "+++" => &["+++"],
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
            return (Some(src[start..at].trim_end()), &src[at + line.len()..]);
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

    const C: Colors = Colors { text: 0xE6E6E6, muted: 0xA0A0A0, link: 0xFFC24C, code_bg: 0x323232, rule: 0x464646 };

    fn rtf(md: &str) -> String {
        to_rtf(md, &C)
    }

    #[test]
    fn markdown_extensions_are_recognised_in_any_case() {
        assert!(is_markdown(r"C:\notes\README.md"));
        assert!(is_markdown("a.MARKDOWN"));
        assert!(is_markdown("a.mdown"));
        assert!(!is_markdown("a.txt"));
        assert!(!is_markdown("md"));
    }

    #[test]
    fn output_is_one_ascii_rtf_group() {
        let r = rtf("# Héllo 😀\n\nText");
        assert!(r.starts_with("{\\rtf1"));
        assert!(r.ends_with('}'));
        assert!(r.is_ascii());
        assert!(r.contains("H\\u233?llo"));
        assert!(r.contains("\\u-10179?\\u-8704?"), "a surrogate pair is two \\u escapes");
    }

    #[test]
    fn color_table_is_in_colorref_order() {
        assert!(rtf("x").contains("{\\colortbl;\\red230\\green230\\blue230;\\red160\\green160\\blue160;\\red76\\green194\\blue255;"));
    }

    #[test]
    fn rtf_control_characters_in_text_are_escaped() {
        assert!(rtf("a \\{b} c").contains("a \\{b\\} c"));
        assert!(rtf("`{x}`").contains("\\{x\\}"));
    }

    #[test]
    fn headings_are_bold_and_sized_by_level() {
        let r = rtf("# One\n\n### Three");
        assert!(r.contains("\\fs40\\cf1\\b One\\par"));
        assert!(r.contains("\\fs28\\cf1\\b Three\\par"));
    }

    #[test]
    fn inline_styles_nest_as_groups() {
        assert!(rtf("**bold *both***").contains("{\\b bold {\\i both}}"));
        assert!(rtf("~~gone~~").contains("{\\strike gone}"));
    }

    #[test]
    fn links_become_hyperlink_fields() {
        let r = rtf("[the spec](docs/SPEC.md)");
        assert!(r.contains("{\\field{\\*\\fldinst{HYPERLINK \"docs/SPEC.md\"}}{\\fldrslt{\\cf3\\ul the spec}}}"));
    }

    #[test]
    fn a_quote_in_a_link_url_cannot_end_the_field() {
        let r = rtf("[x](<a\"b.md>)");
        assert!(r.contains("HYPERLINK \"a%22b.md\""));
    }

    #[test]
    fn raw_html_shows_as_text() {
        let r = rtf("<script>alert(1)</script>\n\nand <b>inline</b>");
        assert!(r.contains("<script>alert(1)</script>"));
        assert!(r.contains("and <b>inline</b>"));
    }

    #[test]
    fn images_show_a_placeholder_and_load_nothing() {
        assert!(rtf("![Logo](https://x.y/a.png)").contains("{\\cf2 [Image: Logo]}"));
        assert!(rtf("![](a.png)").contains("{\\cf2 [Image]}"));
    }

    #[test]
    fn bullet_items_get_a_hanging_marker() {
        let r = rtf("- one\n- two");
        assert!(r.contains("\\li360\\fi-360\\tx360\\plain\\f0\\fs22\\cf1 \\u8226?\\tab one\\par"));
        assert_eq!(r.matches("\\u8226?").count(), 2);
    }

    #[test]
    fn ordered_items_count_from_the_start_number() {
        let r = rtf("3. a\n4. b");
        assert!(r.contains("3.\\tab a"));
        assert!(r.contains("4.\\tab b"));
    }

    #[test]
    fn a_loose_item_starts_one_paragraph_not_two() {
        let r = rtf("- one\n\n- two");
        assert!(r.contains("\\u8226?\\tab one\\par"));
        assert!(!r.contains("\\cf1 \\pard"));
    }

    #[test]
    fn nested_lists_indent_and_change_marker() {
        let r = rtf("- a\n  - b");
        assert!(r.contains("\\li720\\fi-360\\tx720"));
        assert!(r.contains("\\u9702?\\tab b"));
    }

    #[test]
    fn task_items_show_a_checkbox_instead_of_a_bullet() {
        let r = rtf("- [x] done\n- [ ] todo");
        assert!(r.contains("\\u9745?\\tab done"));
        assert!(r.contains("\\u9744?\\tab todo"));
        assert!(!r.contains("\\u8226?"));
    }

    #[test]
    fn code_blocks_keep_their_lines_and_drop_the_last_newline() {
        let r = rtf("```\nfn main() {\n    x\n}\n```");
        assert!(r.contains("\\f1\\fs19\\cf1\\highlight4 fn main() \\{\\line     x\\line \\}\\par"));
    }

    #[test]
    fn a_highlighted_paragraph_moves_its_space_after_to_a_spacer() {
        for md in ["```\nx\n```", "a `b`"] {
            let r = rtf(md);
            assert!(r.contains("\\pard\\sa0\\li"), "{md}");
            assert!(r.contains("\\par\n\\pard\\plain\\fs2\\sa140\\par"), "{md}");
        }
        assert!(!rtf("plain").contains("\\sa0"));
    }

    #[test]
    fn a_heading_as_a_list_item_keeps_its_bullet_on_its_line() {
        let r = rtf("- # Title\n- two");
        assert!(r.contains("\\b \\u8226?\\tab Title\\par"), "{r}");
        assert!(!r.contains("\\u8226?\\tab \\par"), "no bullet alone: {r}");
    }

    #[test]
    fn inline_code_in_a_heading_scales_with_the_heading() {
        assert!(rtf("## The `x` call").contains("{\\f1\\fs28\\highlight4 x}"));
        assert!(rtf("## h\n\nthe `x` call").contains("{\\f1\\fs20\\highlight4 x}"), "body size after the heading");
    }

    #[test]
    fn image_alt_text_that_starts_with_a_style_keeps_the_separator_outside() {
        assert!(rtf("![`icon`](a.png)").contains("{\\cf2 [Image: {\\f1\\fs20\\highlight4 icon}]}"));
        assert!(rtf("![*big* logo](a.png)").contains("{\\cf2 [Image: {\\i big} logo]}"));
    }

    #[test]
    fn a_heading_with_inline_code_moves_its_space_after_too() {
        let r = rtf("## The `x` call\n\nbody");
        assert!(r.contains("\\pard\\plain\\fs2\\sa220\\par\n\\pard\\sb0\\sa0\\li"), "space before in a spacer: {r}");
        assert!(r.contains("\\par\n\\pard\\plain\\fs2\\sa100\\par"), "space after in a spacer: {r}");
        assert!(rtf("## plain").contains("\\sb240\\sa120"), "a heading without code keeps its spacing");
    }

    #[test]
    fn quotes_are_indented_and_muted() {
        assert!(rtf("> said").contains("\\li360\\plain\\f0\\fs22\\cf2 said\\par"));
    }

    #[test]
    fn a_rule_is_a_centered_bordered_row() {
        let r = rtf("a\n\n---\n\nb");
        assert!(r.contains("\\trowd\\trqc"));
        assert!(r.contains("\\clbrdrb"));
    }

    #[test]
    fn tables_write_one_row_per_line_with_a_shaded_bold_header() {
        let r = rtf("| A | Bee |\n|:-:|--:|\n| 1 | 2 |");
        assert_eq!(r.matches("\\row").count(), 2);
        assert_eq!(r.matches("\\clcbpat4").count(), 2, "header cells only");
        assert!(r.contains("\\pard\\intbl\\qc\\plain\\f0\\fs22\\cf1\\b A\\cell"));
        assert!(r.contains("\\pard\\intbl\\qr\\plain\\f0\\fs22\\cf1 2\\cell"));
    }

    #[test]
    fn front_matter_shows_as_a_code_block_not_a_rule_and_heading() {
        for md in ["---\ntitle: Notes\ntags: [a, b]\n---\n\n# Body", "+++\r\ntitle = \"Notes\"\r\n+++\r\n\r\n# Body", "---\ntitle: x\n...\n# Body"] {
            let r = rtf(md);
            assert!(!r.contains("\\trowd"), "no rule: {md}");
            assert!(r.contains("\\f1\\fs19\\cf1\\highlight4 title"), "{r}");
            assert!(r.contains("\\fs40\\cf1\\b Body\\par"), "{md}");
        }
        let r = rtf("---\r\ntitle: Notes\r\ntags: [a, b]\r\n---\r\n");
        assert!(r.contains("title: Notes\\line tags: [a, b]\\par"), "{r}");
    }

    #[test]
    fn empty_front_matter_shows_nothing() {
        let r = rtf("---\n---\n# Body");
        assert!(!r.contains("\\highlight"), "{r}");
        assert!(!r.contains("\\trowd"), "{r}");
        assert!(r.contains("\\b Body\\par"), "{r}");
    }

    #[test]
    fn dashes_that_are_not_front_matter_stay_rules() {
        // A later rule pair, an unclosed one, and a blank line after the first `---`.
        for md in ["text\n\n---\nnot: meta\n---\n", "---\ntitle: open", "---\n\ntitle: x\n---\n"] {
            assert!(rtf(md).contains("\\trowd"), "{md}");
        }
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
    fn gap_text_in_a_highlighted_paragraph_stays_as_written() {
        // The source text \sa160\ is escaped to \\sa160\\ and must stay so.
        let r = rtf(r"# `x` \sa160\ y");
        assert!(r.contains(r"\\sa160\\ y"), "{r}");
    }

    #[test]
    fn non_markdown_or_unsaved_relative_links_do_nothing() {
        assert_eq!(resolve("setup.exe", Some(Path::new(r"C:\d"))), Target::Nothing);
        assert_eq!(resolve("a.md", None), Target::Nothing);
    }
}
