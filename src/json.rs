//! Format JSON for reading: two-space indent, one member per line, and
//! `"key": value`. Strings and numbers are copied as they are, so the format
//! changes only white space. No windows here: the editor calls it.

use std::path::Path;

const INDENT: usize = 2;

/// A `.json` file. JSON Lines (`.jsonl`, `.ndjson`) keeps one record on each
/// line, so it is never formatted.
pub fn is_json(path: &str) -> bool {
    let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
    matches!(ext.as_deref(), Some("json"))
}

/// The text has no line break, or only at the end: a minified file.
pub fn is_one_line(t: &[u16]) -> bool {
    let end = t.iter().rposition(|&c| !is_ws(c)).map_or(0, |k| k + 1);
    !t[..end].iter().any(|&c| c == 10 || c == 13)
}

/// The line break that the text uses first. CRLF when it has none.
pub fn eol_of(t: &[u16]) -> &'static [u16] {
    match t.iter().position(|&c| c == 10 || c == 13) {
        Some(k) if t[k] == 10 => &[10],
        Some(k) if t.get(k + 1) == Some(&10) => &[13, 10],
        Some(_) => &[13],
        None => &[13, 10],
    }
}

#[derive(Debug, PartialEq)]
pub enum Error {
    /// The offset where the text stops being JSON.
    At(usize),
    /// The indent of deep nesting grows the text past `max_len`.
    TooBig,
}

/// The longest result: the indent grows with the square of the depth, so a
/// small file nested 100,000 deep would need gigabytes.
fn max_len(len: usize) -> usize {
    len.saturating_mul(16).saturating_add(1 << 20).min(1 << 28)
}

/// `t` formatted with `eol` between lines. A line break at the end stays.
/// A loop with a stack of open brackets, not recursion, so deep nesting
/// cannot overflow the stack.
pub fn format(t: &[u16], eol: &[u16]) -> Result<Vec<u16>, Error> {
    let max = max_len(t.len());
    let mut out = Vec::with_capacity(t.len() + t.len() / 2);
    // The close bracket of each open object or array.
    let mut open: Vec<u16> = Vec::new();
    let newline = |out: &mut Vec<u16>, depth: usize| {
        if out.len() + eol.len() + depth * INDENT > max {
            return Err(Error::TooBig);
        }
        out.extend_from_slice(eol);
        out.resize(out.len() + depth * INDENT, b' ' as u16);
        Ok(())
    };
    let mut i = 0;
    let mut want_value = true;
    loop {
        i = skip_ws(t, i);
        if want_value {
            match t.get(i).copied() {
                Some(c @ (0x7B | 0x5B)) => {
                    let close = c + 2; // '{' -> '}', '[' -> ']'
                    out.push(c);
                    let j = skip_ws(t, i + 1);
                    if t.get(j) == Some(&close) {
                        out.push(close);
                        i = j + 1;
                    } else {
                        open.push(close);
                        newline(&mut out, open.len())?;
                        i = if close == 0x7D { key(t, j, &mut out).map_err(Error::At)? } else { j };
                        continue;
                    }
                }
                Some(0x22) => i = string(t, i, &mut out).map_err(Error::At)?,
                Some(_) => i = scalar(t, i, &mut out).map_err(Error::At)?,
                None => return Err(Error::At(i)),
            }
            want_value = false;
            continue;
        }
        let Some(&close) = open.last() else {
            if i < t.len() {
                return Err(Error::At(i));
            }
            break;
        };
        match t.get(i).copied() {
            Some(0x2C) => {
                out.push(0x2C);
                newline(&mut out, open.len())?;
                i = skip_ws(t, i + 1);
                if close == 0x7D {
                    i = key(t, i, &mut out).map_err(Error::At)?;
                }
                want_value = true;
            }
            Some(c) if c == close => {
                open.pop();
                newline(&mut out, open.len())?;
                out.push(c);
                i += 1;
            }
            _ => return Err(Error::At(i)),
        }
    }
    if t.last().is_some_and(|&c| c == 10 || c == 13) {
        out.extend_from_slice(eol);
    }
    Ok(out)
}

/// The 1-based line and column of offset `at`, for a message.
pub fn line_col(t: &[u16], at: usize) -> (usize, usize) {
    let at = at.min(t.len());
    let mut line = 1;
    let mut start = 0;
    for (k, &c) in t[..at].iter().enumerate() {
        if c == 10 || (c == 13 && t.get(k + 1) != Some(&10)) {
            line += 1;
            start = k + 1;
        }
    }
    (line, at - start + 1)
}

fn is_ws(c: u16) -> bool {
    matches!(c, 9 | 10 | 13 | 32)
}

fn skip_ws(t: &[u16], mut i: usize) -> usize {
    while t.get(i).is_some_and(|&c| is_ws(c)) {
        i += 1;
    }
    i
}

/// An object key at `i`, then `:`. Writes `"key": ` and returns the offset after the colon.
fn key(t: &[u16], i: usize, out: &mut Vec<u16>) -> Result<usize, usize> {
    if t.get(i) != Some(&0x22) {
        return Err(i);
    }
    let j = skip_ws(t, string(t, i, out)?);
    if t.get(j) != Some(&0x3A) {
        return Err(j);
    }
    out.extend_from_slice(&[0x3A, 0x20]);
    Ok(j + 1)
}

/// The string that starts at `i`, copied as it is. Returns the offset after it.
fn string(t: &[u16], i: usize, out: &mut Vec<u16>) -> Result<usize, usize> {
    let hex = |h: u16| h < 128 && (h as u8).is_ascii_hexdigit();
    let mut k = i + 1;
    loop {
        match t.get(k).copied() {
            Some(0x22) => break,
            Some(0x5C) => match t.get(k + 1).copied() {
                Some(0x22 | 0x5C | 0x2F | 0x62 | 0x66 | 0x6E | 0x72 | 0x74) => k += 2,
                Some(0x75) if t.len() >= k + 6 && t[k + 2..k + 6].iter().all(|&h| hex(h)) => k += 6,
                _ => return Err(k),
            },
            Some(c) if c >= 0x20 => k += 1,
            _ => return Err(k),
        }
    }
    out.extend_from_slice(&t[i..=k]);
    Ok(k + 1)
}

/// A number, `true`, `false` or `null` at `i`, copied as it is.
fn scalar(t: &[u16], i: usize, out: &mut Vec<u16>) -> Result<usize, usize> {
    for word in ["true", "false", "null"] {
        let w: Vec<u16> = word.encode_utf16().collect();
        if t[i..].starts_with(&w) {
            out.extend_from_slice(&w);
            return Ok(i + w.len());
        }
    }
    let digit = |k: usize| t.get(k).is_some_and(|&c| (0x30..=0x39).contains(&c));
    let digits = |mut k: usize| -> Result<usize, usize> {
        if !digit(k) {
            return Err(k);
        }
        while digit(k) {
            k += 1;
        }
        Ok(k)
    };
    let mut k = i;
    if t.get(k) == Some(&0x2D) {
        k += 1;
    }
    k = if t.get(k) == Some(&0x30) { k + 1 } else { digits(k)? };
    if t.get(k) == Some(&0x2E) {
        k = digits(k + 1)?;
    }
    if matches!(t.get(k), Some(0x65 | 0x45)) {
        k += 1;
        if matches!(t.get(k), Some(0x2B | 0x2D)) {
            k += 1;
        }
        k = digits(k)?;
    }
    out.extend_from_slice(&t[i..k]);
    Ok(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn fmt(s: &str) -> Result<String, usize> {
        match format(&w(s), &[10]) {
            Ok(v) => Ok(String::from_utf16(&v).unwrap()),
            Err(Error::At(i)) => Err(i),
            Err(Error::TooBig) => panic!("too big"),
        }
    }

    #[test]
    fn formats_nested_objects_and_arrays() {
        let got = fmt(r#"{"a":1,"b":[true,null,{"c":"x"}],"d":{}}"#).unwrap();
        assert_eq!(got, "{\n  \"a\": 1,\n  \"b\": [\n    true,\n    null,\n    {\n      \"c\": \"x\"\n    }\n  ],\n  \"d\": {}\n}");
    }

    #[test]
    fn keeps_strings_and_numbers_exactly() {
        let src = r#"["a \"q\" \\ \u00e9 {,}:", -0.5e+10, 12345678901234567890, 1E3]"#;
        assert_eq!(fmt(src).unwrap(), "[\n  \"a \\\"q\\\" \\\\ \\u00e9 {,}:\",\n  -0.5e+10,\n  12345678901234567890,\n  1E3\n]");
    }

    #[test]
    fn already_formatted_text_comes_out_the_same() {
        let once = fmt(r#"{"a":[1,2],"b":{"c":[]}}"#).unwrap();
        assert_eq!(fmt(&once).unwrap(), once);
    }

    #[test]
    fn empty_containers_and_scalars_stay_on_one_line() {
        assert_eq!(fmt(" [ ] ").unwrap(), "[]");
        assert_eq!(fmt("{ }").unwrap(), "{}");
        assert_eq!(fmt("\"x\"").unwrap(), "\"x\"");
        assert_eq!(fmt("42").unwrap(), "42");
    }

    #[test]
    fn keeps_a_final_line_break_with_the_eol_given() {
        assert_eq!(format(&w("[1]\r\n"), &[13, 10]).unwrap(), w("[\r\n  1\r\n]\r\n"));
    }

    #[test]
    fn refuses_text_that_is_not_json() {
        assert_eq!(fmt(r#"{"a":1,}"#), Err(7)); // trailing comma
        assert_eq!(fmt(r#"{"a" 1}"#), Err(5));
        assert_eq!(fmt("[1 2]"), Err(3));
        assert_eq!(fmt("{a:1}"), Err(1));
        assert_eq!(fmt("[01]"), Err(2));
        assert_eq!(fmt("[1.]"), Err(3));
        assert_eq!(fmt("[\"a\tb\"]"), Err(3)); // raw tab in a string
        assert_eq!(fmt("[\"\\x\"]"), Err(2));
        assert_eq!(fmt("[\"\\u12g4\"]"), Err(2));
        assert_eq!(fmt("[\"open"), Err(6));
        assert_eq!(fmt("[1]]"), Err(3));
        assert_eq!(fmt("// c\n{}"), Err(0));
        assert_eq!(fmt("{\"a\":1}\n{\"b\":2}"), Err(8)); // JSON Lines
        assert_eq!(fmt(""), Err(0));
        assert_eq!(fmt("[tru]"), Err(1));
    }

    #[test]
    fn deep_nesting_formats_without_recursion() {
        let n = 100;
        let src = "[".repeat(n) + &"]".repeat(n);
        assert!(fmt(&src).is_ok());
    }

    #[test]
    fn very_deep_nesting_is_refused_not_grown_to_gigabytes() {
        let n = 200_000;
        let src = "[".repeat(n) + &"]".repeat(n);
        assert_eq!(format(&w(&src), &[10]), Err(Error::TooBig));
    }

    #[test]
    fn one_line_and_eol_detection() {
        assert!(is_one_line(&w(r#"{"a":1}"#)));
        assert!(is_one_line(&w("{\"a\":1}\r\n")));
        assert!(!is_one_line(&w("{\n\"a\":1}")));
        assert_eq!(eol_of(&w("a\nb")), &[10]);
        assert_eq!(eol_of(&w("a\r\nb")), &[13, 10]);
        assert_eq!(eol_of(&w("a\rb")), &[13]);
        assert_eq!(eol_of(&w("ab")), &[13, 10]);
    }

    #[test]
    fn only_dot_json_is_json() {
        assert!(is_json(r"C:\a\b.json"));
        assert!(is_json("x.JSON"));
        assert!(!is_json("x.jsonl"));
        assert!(!is_json("x.ndjson"));
        assert!(!is_json("x.txt"));
    }

    #[test]
    fn line_col_counts_from_one() {
        let t = w("ab\r\ncd\nef");
        assert_eq!(line_col(&t, 0), (1, 1));
        assert_eq!(line_col(&t, 5), (2, 2));
        assert_eq!(line_col(&t, 8), (3, 2));
    }
}
