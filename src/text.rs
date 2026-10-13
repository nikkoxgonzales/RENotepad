//! Text rules that don't touch windows: .LOG, find, replace-all, command line.

use windows_sys::Win32::Globalization::{
    FindNLSString, GetDateFormatW, GetTimeFormatW, FIND_FROMEND, FIND_FROMSTART, NORM_IGNORECASE,
};

const LOCALE_USER_DEFAULT: u32 = 0x400;
const DATE_SHORTDATE: u32 = 1;
const TIME_NOSECONDS: u32 = 2;

/// Documents whose first four characters are exactly ".LOG" (case-sensitive)
/// get a timestamp appended every time they are opened.
pub fn is_log_file(text: &[u16]) -> bool {
    text.starts_with(&[b'.' as u16, b'L' as u16, b'O' as u16, b'G' as u16])
}

/// "h:mm tt M/d/yyyy" in the user's locale: time without seconds, short date.
/// F5 inserts it bare; the .LOG feature wraps it in CRLFs.
pub fn timestamp(for_log: bool) -> Vec<u16> {
    let mut time = [0u16; 80];
    let mut date = [0u16; 80];
    unsafe {
        let t = GetTimeFormatW(LOCALE_USER_DEFAULT, TIME_NOSECONDS, std::ptr::null(), std::ptr::null(), time.as_mut_ptr(), 80);
        let d = GetDateFormatW(LOCALE_USER_DEFAULT, DATE_SHORTDATE, std::ptr::null(), std::ptr::null(), date.as_mut_ptr(), 80);
        let crlf = [13u16, 10];
        let mut out = Vec::new();
        if for_log {
            out.extend_from_slice(&crlf);
        }
        out.extend_from_slice(&time[..(t.max(1) - 1) as usize]);
        out.push(b' ' as u16);
        out.extend_from_slice(&date[..(d.max(1) - 1) as usize]);
        if for_log {
            out.extend_from_slice(&crlf);
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub start: usize,
    pub len: usize,
    /// The search ran off one end and continued from the other.
    pub wrapped: bool,
}

/// Locale-aware search (FindNLSString, as Notepad uses), from `sel` = current
/// selection. Down searches after the selection start + 1 so repeated Find Next
/// moves forward; up searches strictly before the selection start.
pub fn find(text: &[u16], pat: &[u16], sel: (usize, usize), down: bool, match_case: bool, wrap: bool) -> Option<Hit> {
    if pat.is_empty() || text.is_empty() {
        return None;
    }
    let (sel_start, sel_end) = sel;
    if down {
        // Win10: when there's a selection, search starts at its end; else at the caret.
        let from = sel_end.min(text.len());
        if let Some((s, l)) = nls_find(&text[from..], pat, false, match_case) {
            return Some(Hit { start: from + s, len: l, wrapped: false });
        }
        if wrap {
            // Search the head up to the end of the selection so a match straddling it is found.
            let end = (sel_end + pat.len()).min(text.len());
            if let Some((s, l)) = nls_find(&text[..end], pat, false, match_case) {
                return Some(Hit { start: s, len: l, wrapped: true });
            }
        }
    } else {
        let to = sel_start.min(text.len());
        if let Some((s, l)) = nls_find(&text[..to], pat, true, match_case) {
            return Some(Hit { start: s, len: l, wrapped: false });
        }
        if wrap {
            let from = sel_start.saturating_sub(pat.len());
            if let Some((s, l)) = nls_find(&text[from..], pat, true, match_case) {
                return Some(Hit { start: from + s, len: l, wrapped: true });
            }
        }
    }
    None
}

fn nls_find(hay: &[u16], pat: &[u16], from_end: bool, match_case: bool) -> Option<(usize, usize)> {
    if hay.is_empty() {
        return None;
    }
    let mut flags = if from_end { FIND_FROMEND } else { FIND_FROMSTART };
    if !match_case {
        flags |= NORM_IGNORECASE;
    }
    let mut found_len: i32 = 0;
    let idx = unsafe {
        FindNLSString(LOCALE_USER_DEFAULT, flags, hay.as_ptr(), hay.len() as i32, pat.as_ptr(), pat.len() as i32, &mut found_len)
    };
    (idx >= 0 && found_len > 0).then_some((idx as usize, found_len as usize))
}

/// Every non-overlapping match replaced, start to end. Returns (new text, count).
pub fn replace_all(text: &[u16], pat: &[u16], with: &[u16], match_case: bool) -> (Vec<u16>, usize) {
    let mut out = Vec::with_capacity(text.len());
    let mut pos = 0;
    let mut count = 0;
    while pos < text.len() {
        match nls_find(&text[pos..], pat, false, match_case) {
            Some((s, l)) => {
                out.extend_from_slice(&text[pos..pos + s]);
                out.extend_from_slice(with);
                pos += s + l;
                count += 1;
            }
            None => break,
        }
    }
    out.extend_from_slice(&text[pos.min(text.len())..]);
    (out, count)
}

/// Does the selected text equal the search string (for Replace)?
pub fn selection_matches(sel: &[u16], pat: &[u16], match_case: bool) -> bool {
    !sel.is_empty() && nls_find(sel, pat, false, match_case) == Some((0, sel.len()))
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// /A or /W: force ANSI / UTF-16 LE when opening.
    pub force_ansi: bool,
    pub force_unicode: bool,
    /// /P: print the file to the default printer and exit.
    pub print: bool,
    pub file: Option<String>,
}

/// Notepad takes the rest of the command line as one filename, quoted or not
/// ("notepad my notes.txt" works), after optional /A, /W or /P switches.
pub fn parse_args(cmdline: &str) -> Args {
    let mut rest = skip_program_name(cmdline).trim_start();
    let mut args = Args::default();
    loop {
        let lower = rest.get(..2).map(str::to_ascii_lowercase);
        let switch_end = rest.as_bytes().get(2).is_none_or(|c| *c == b' ' || *c == b'\t');
        match lower.as_deref() {
            Some("/a") if switch_end => args.force_ansi = true,
            Some("/w") if switch_end => args.force_unicode = true,
            Some("/p") if switch_end => args.print = true,
            _ => break,
        }
        rest = rest[2..].trim_start();
    }
    let file = rest.trim();
    let file = file.strip_prefix('"').map_or(file, |f| f.strip_suffix('"').unwrap_or(f));
    if !file.is_empty() {
        args.file = Some(file.to_string());
    }
    args
}

fn skip_program_name(cmdline: &str) -> &str {
    let s = cmdline.trim_start();
    if let Some(r) = s.strip_prefix('"') {
        r.find('"').map_or("", |i| &r[i + 1..])
    } else {
        s.find([' ', '\t']).map_or("", |i| &s[i..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn log_needs_exact_prefix() {
        assert!(is_log_file(&w(".LOG\r\nfirst")));
        assert!(!is_log_file(&w(".log\r\n")));
        assert!(!is_log_file(&w(" .LOG")));
        assert!(!is_log_file(&w(".LO")));
    }

    #[test]
    fn timestamp_shapes() {
        let bare = String::from_utf16(&timestamp(false)).unwrap();
        assert!(!bare.contains('\n') && bare.contains(' '), "{bare:?}");
        let log = String::from_utf16(&timestamp(true)).unwrap();
        assert!(log.starts_with("\r\n") && log.ends_with("\r\n"));
    }

    #[test]
    fn find_down_and_up() {
        let t = w("one two one two");
        let p = w("two");
        assert_eq!(find(&t, &p, (0, 0), true, false, false).unwrap().start, 4);
        assert_eq!(find(&t, &p, (4, 7), true, false, false).unwrap().start, 12);
        assert_eq!(find(&t, &p, (12, 15), false, false, false).unwrap().start, 4);
    }

    #[test]
    fn find_wraps_only_when_asked() {
        let t = w("abc xyz abc");
        let p = w("xyz");
        assert_eq!(find(&t, &p, (8, 11), true, false, false), None);
        assert_eq!(find(&t, &p, (8, 11), true, false, true), Some(Hit { start: 4, len: 3, wrapped: true }));
        assert_eq!(find(&t, &p, (0, 0), false, false, true), Some(Hit { start: 4, len: 3, wrapped: true }));
    }

    #[test]
    fn find_case() {
        let t = w("Hello hello");
        assert_eq!(find(&t, &w("hello"), (0, 0), true, true, false).unwrap().start, 6);
        assert_eq!(find(&t, &w("hello"), (0, 0), true, false, false).unwrap().start, 0);
    }

    #[test]
    fn replace_all_counts() {
        let (out, n) = replace_all(&w("a-b-c"), &w("-"), &w("--"), true);
        assert_eq!((String::from_utf16(&out).unwrap(), n), ("a--b--c".into(), 2));
        let (out, n) = replace_all(&w("Cat cat"), &w("cat"), &w("dog"), false);
        assert_eq!((String::from_utf16(&out).unwrap(), n), ("dog dog".into(), 2));
    }

    #[test]
    fn args_quoted_and_unquoted() {
        assert_eq!(parse_args(r#""C:\x\np.exe" "C:\my notes.txt""#).file.as_deref(), Some(r"C:\my notes.txt"));
        assert_eq!(parse_args(r#"np.exe my notes.txt"#).file.as_deref(), Some("my notes.txt"));
        assert_eq!(parse_args(r#"np.exe"#).file, None);
        let a = parse_args(r#"np.exe /A /p "f.txt""#);
        assert!(a.force_ansi && a.print && a.file.as_deref() == Some("f.txt"));
        // a path starting with /a... is not a switch
        assert_eq!(parse_args("np.exe /apple.txt").file.as_deref(), Some("/apple.txt"));
    }
}
