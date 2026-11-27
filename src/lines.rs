//! Line commands without windows: Tab and Shift+Tab on lines, Ctrl+D, and the
//! bracket that matches the one at the caret. Lines end at CR LF, LF or CR,
//! because the editor keeps each file's own line breaks.

/// Replace `start..end` with `with`, then select `sel`.
#[derive(Debug, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub with: Vec<u16>,
    pub sel: (usize, usize),
}

const TAB: u16 = 9;
const SPACE: u16 = 32;

fn is_break(c: u16) -> bool {
    c == 10 || c == 13
}

fn line_start(t: &[u16], i: usize) -> usize {
    t[..i].iter().rposition(|&c| is_break(c)).map_or(0, |k| k + 1)
}

fn line_end(t: &[u16], i: usize) -> usize {
    t[i..].iter().position(|&c| is_break(c)).map_or(t.len(), |k| i + k)
}

/// The whole lines that the selection `s..e` touches, without the last line
/// break. A selection that ends at the start of a line leaves that line out.
fn block(t: &[u16], s: usize, e: usize) -> (usize, usize) {
    let start = line_start(t, s);
    let mut j = e;
    if e > s && line_start(t, e) == e {
        if t[j - 1] == 10 {
            j -= 1;
        }
        if j > 0 && t[j - 1] == 13 {
            j -= 1;
        }
    }
    let j = j.max(start);
    (start, line_end(t, line_start(t, j)))
}

/// The indent of this text: a tab, or spaces as wide as the most common step
/// between the indents of two lines next to each other (like VS Code). A text
/// with no indented lines gets a tab, like Notepad.
/// Ceiling: reads the first 10,000 lines only.
pub fn indent_unit(t: &[u16]) -> Vec<u16> {
    let (mut tabs, mut spaced) = (0usize, 0usize);
    let mut steps = [0usize; 9];
    let mut prev: Option<usize> = None;
    for line in t.split(|&c| c == 10).take(10_000) {
        let line = line.strip_suffix(&[13]).unwrap_or(line);
        if line.iter().all(|&c| c == SPACE || c == TAB) {
            continue; // blank lines say nothing
        }
        match line[0] {
            TAB => {
                tabs += 1;
                prev = None;
            }
            _ => {
                let n = line.iter().take_while(|&&c| c == SPACE).count();
                if n > 0 {
                    spaced += 1;
                }
                if let Some(p) = prev {
                    let step = n.abs_diff(p);
                    if (2..=8).contains(&step) {
                        steps[step] += 1;
                    }
                }
                prev = Some(n);
            }
        }
    }
    if spaced > tabs {
        // On a tie the smaller width wins.
        let width = (2..=8).rev().max_by_key(|&k| steps[k]).filter(|&k| steps[k] > 0).unwrap_or(4);
        vec![SPACE; width]
    } else {
        vec![TAB]
    }
}

/// How many spaces a Tab inserts at `col` with this unit (to the next stop),
/// or None for a tab character.
pub fn spaces_for_tab(unit: &[u16], col: usize) -> Option<usize> {
    (unit.first() == Some(&SPACE)).then(|| unit.len() - col % unit.len())
}

/// Tab (indent) or Shift+Tab (outdent) on each line that `s..e` touches.
/// Indent skips empty lines. Outdent takes off one tab, or up to one unit of
/// spaces (4 when the unit is a tab). None when no line changes.
pub fn indent(t: &[u16], s: usize, e: usize, unit: &[u16], outdent: bool) -> Option<Edit> {
    let (start, end) = block(t, s, e);
    let width = if unit.first() == Some(&SPACE) { unit.len() } else { 4 };
    // Per line: its start in `t`, and what goes in or out there.
    let mut lines: Vec<(usize, usize)> = Vec::new();
    let mut with = Vec::with_capacity(end - start + unit.len());
    let mut i = start;
    loop {
        let le = line_end(t, i);
        let line = &t[i..le];
        let n = if outdent {
            match line.first() {
                Some(&TAB) => 1,
                _ => line.iter().take(width).take_while(|&&c| c == SPACE).count(),
            }
        } else if line.is_empty() {
            0
        } else {
            unit.len()
        };
        if outdent {
            with.extend_from_slice(&line[n..]);
        } else {
            if n > 0 {
                with.extend_from_slice(unit);
            }
            with.extend_from_slice(line);
        }
        lines.push((i, n));
        if le >= end {
            break;
        }
        // The line break: CR LF, LF or CR.
        let next = if t[le] == 13 && t.get(le + 1) == Some(&10) { le + 2 } else { le + 1 };
        with.extend_from_slice(&t[le..next]);
        i = next;
    }
    if lines.iter().all(|&(_, n)| n == 0) {
        return None;
    }
    let map = |p: usize| -> usize {
        let mut shift: isize = 0;
        for &(ls, n) in &lines {
            if p < ls {
                break;
            }
            let own = if outdent { -((n.min(p - ls)) as isize) } else if p > ls { n as isize } else { 0 };
            let full = if outdent { -(n as isize) } else { n as isize };
            // Lines that end before p move it by their full change.
            let le = line_end(t, ls);
            shift += if p > le { full } else { own };
        }
        (p as isize + shift) as usize
    };
    Some(Edit { start, end, sel: (map(s), map(e)), with })
}

/// Ctrl+D: a copy of the lines that `s..e` touches goes below them, and the
/// selection moves into the copy. The copy uses the line break of the text.
pub fn duplicate(t: &[u16], s: usize, e: usize) -> Edit {
    let (start, end) = block(t, s, e);
    let at = if end < t.len() { end } else { t.iter().position(|&c| is_break(c)).unwrap_or(t.len()) };
    let brk: Vec<u16> = match t.get(at) {
        Some(13) if t.get(at + 1) == Some(&10) => vec![13, 10],
        Some(&c) => vec![c],
        None => vec![13, 10],
    };
    let shift = brk.len() + end - start;
    let mut with = brk;
    with.extend_from_slice(&t[start..end]);
    Edit { start: end, end, with, sel: (s + shift, e + shift) }
}

/// The bracket beside the caret and its partner: the one before the caret
/// first, then the one after it. Brackets in strings count too.
/// Ceiling: looks 1M characters far at most, so an unmatched bracket in a big
/// file costs about 1 ms.
pub fn match_bracket(t: &[u16], caret: usize) -> Option<(usize, usize)> {
    const LIMIT: usize = 1 << 20;
    let pair = |c: u16| -> Option<(u16, u16, bool)> {
        Some(match c {
            0x28 => (0x28, 0x29, true),
            0x5B => (0x5B, 0x5D, true),
            0x7B => (0x7B, 0x7D, true),
            0x29 => (0x28, 0x29, false),
            0x5D => (0x5B, 0x5D, false),
            0x7D => (0x7B, 0x7D, false),
            _ => return None,
        })
    };
    for i in [caret.checked_sub(1), Some(caret)].into_iter().flatten() {
        let Some((open, close, forward)) = t.get(i).and_then(|&c| pair(c)) else { continue };
        let mut depth = 0usize;
        let found = if forward {
            t[i + 1..t.len().min(i + 1 + LIMIT)].iter().position(|&c| {
                if c == open {
                    depth += 1;
                } else if c == close {
                    if depth == 0 {
                        return true;
                    }
                    depth -= 1;
                }
                false
            }).map(|k| i + 1 + k)
        } else {
            t[i.saturating_sub(LIMIT)..i].iter().rposition(|&c| {
                if c == close {
                    depth += 1;
                } else if c == open {
                    if depth == 0 {
                        return true;
                    }
                    depth -= 1;
                }
                false
            }).map(|k| i.saturating_sub(LIMIT) + k)
        };
        if let Some(m) = found {
            return Some((i, m));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn run(t: &str, e: &Edit) -> String {
        let mut v = w(t);
        v.splice(e.start..e.end, e.with.iter().copied());
        String::from_utf16(&v).unwrap()
    }

    #[test]
    fn indent_unit_follows_the_text() {
        assert_eq!(indent_unit(&w("{\n  \"a\": {\n    \"b\": 1\n  }\n}")), w("  "));
        assert_eq!(indent_unit(&w("{\r\n    \"a\": 1,\r\n    \"b\": 2\r\n}")), w("    "));
        assert_eq!(indent_unit(&w("{\n\t\"a\": 1\n}")), w("\t"));
        assert_eq!(indent_unit(&w("KEY=1\nOTHER=2")), w("\t"));
    }

    #[test]
    fn tab_goes_to_the_next_stop() {
        assert_eq!(spaces_for_tab(&w("    "), 0), Some(4));
        assert_eq!(spaces_for_tab(&w("    "), 5), Some(3));
        assert_eq!(spaces_for_tab(&w("\t"), 3), None);
    }

    #[test]
    fn indent_each_selected_line_but_empty_ones() {
        let t = "a\r\n\r\nb\r\nc";
        // From inside "a" to the end of "b".
        let e = indent(&w(t), 1, 6, &w("  "), false).unwrap();
        assert_eq!(run(t, &e), "  a\r\n\r\n  b\r\nc");
        assert_eq!(e.sel, (3, 10));
    }

    #[test]
    fn a_selection_that_ends_at_a_line_start_leaves_that_line() {
        let t = "a\nb\nc";
        let e = indent(&w(t), 0, 4, &w("\t"), false).unwrap();
        assert_eq!(run(t, &e), "\ta\n\tb\nc");
        // The start at column 0 stays at column 0; the end moves with the text.
        assert_eq!(e.sel, (0, 6));
    }

    #[test]
    fn outdent_takes_a_tab_or_one_unit_of_spaces() {
        let t = "\ta\n      b\n c\nd";
        let e = indent(&w(t), 0, t.len(), &w("    "), true).unwrap();
        assert_eq!(run(t, &e), "a\n  b\nc\nd");
        assert_eq!(e.sel, (0, 9));
        assert_eq!(indent(&w("a\nb"), 0, 3, &w("  "), true), None);
    }

    #[test]
    fn outdent_moves_a_caret_inside_the_removed_spaces_to_the_line_start() {
        let t = "x\n    y";
        let e = indent(&w(t), 4, 4, &w("    "), true).unwrap();
        assert_eq!(run(t, &e), "x\ny");
        assert_eq!(e.sel, (2, 2));
    }

    #[test]
    fn duplicate_copies_the_line_below_and_moves_the_caret() {
        let t = "one\r\ntwo\r\nthree";
        let e = duplicate(&w(t), 6, 6); // in "two"
        assert_eq!(run(t, &e), "one\r\ntwo\r\ntwo\r\nthree");
        assert_eq!(e.sel, (11, 11));
    }

    #[test]
    fn duplicate_the_last_line_uses_the_text_line_break() {
        let t = "a\nlast";
        let e = duplicate(&w(t), 6, 6);
        assert_eq!(run(t, &e), "a\nlast\nlast");
        assert_eq!(e.sel, (11, 11));
        let e = duplicate(&w("solo"), 0, 0);
        assert_eq!(run("solo", &e), "solo\r\nsolo");
    }

    #[test]
    fn duplicate_a_selection_of_lines() {
        let t = "a\nb\nc";
        let e = duplicate(&w(t), 0, 3); // "a\nb"
        assert_eq!(run(t, &e), "a\nb\na\nb\nc");
        assert_eq!(e.sel, (4, 7));
    }

    #[test]
    fn match_bracket_finds_the_partner() {
        let t = w("{\"a\": [1, (2)], \"b\": {}}");
        assert_eq!(match_bracket(&t, 0), Some((0, 23)));
        assert_eq!(match_bracket(&t, 24), Some((23, 0)));
        assert_eq!(match_bracket(&t, 6), Some((6, 13))); // before "["
        assert_eq!(match_bracket(&t, 14), Some((13, 6))); // after "]"
        assert_eq!(match_bracket(&t, 3), None);
        assert_eq!(match_bracket(&w("(()"), 0), None);
    }
}
