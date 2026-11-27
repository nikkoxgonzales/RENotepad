//! The raw HTML in a Markdown file, cut into tags, comments and text for the
//! preview. Only a tokenizer: markdown.rs decides what each tag does, as
//! GitHub's sanitizer would. Nothing here loads anything.

#[derive(Debug, PartialEq)]
pub enum Token {
    /// `<name attr="v">` (or `<name/>`). The name is lower case.
    Open { name: String, attrs: Vec<(String, String)>, raw: String },
    /// `</name>`, lower case.
    Close { name: String, raw: String },
    /// Text between tags, with entities decoded.
    Text(String),
}

impl Token {
    /// The value of an attribute of an opening tag.
    pub fn attr(&self, key: &str) -> Option<&str> {
        match self {
            Token::Open { attrs, .. } => attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()),
            _ => None,
        }
    }
}

/// Cut `s` into tokens. Returns the tokens and the length of `s` that they
/// use: a tag or comment that is not closed yet (it continues in the next
/// line of an HTML block) stays for the next call. Comments are dropped, as
/// GitHub drops them. A `<` that does not start a tag is text.
pub fn tokens(s: &str) -> (Vec<Token>, usize) {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    let b = s.as_bytes();
    while i < b.len() {
        if b[i] != b'<' {
            let next = s[i..].find('<').map_or(s.len(), |n| i + n);
            text.push_str(&s[i..next]);
            i = next;
            continue;
        }
        let rest = &s[i..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            match comment.find("-->") {
                Some(end) => {
                    i += 4 + end + 3;
                    continue;
                }
                None => break, // the comment goes on in the next line
            }
        }
        match tag(rest) {
            Some(Some((tok, len))) => {
                flush_text(&mut out, &mut text);
                out.push(tok);
                i += len;
            }
            Some(None) => break, // a tag that goes on in the next line
            None => {
                text.push('<');
                i += 1;
            }
        }
    }
    flush_text(&mut out, &mut text);
    (out, i)
}

fn flush_text(out: &mut Vec<Token>, text: &mut String) {
    if !text.is_empty() {
        out.push(Token::Text(decode_entities(text)));
        text.clear();
    }
}

/// A tag at the start of `s`: Some(Some(token, length)), Some(None) when it
/// is not closed yet, or None when `s` does not start a tag.
fn tag(s: &str) -> Option<Option<(Token, usize)>> {
    let b = s.as_bytes();
    let close = b.get(1) == Some(&b'/');
    let start = if close { 2 } else { 1 };
    let name_len = s[start..].bytes().take_while(|c| c.is_ascii_alphanumeric() || *c == b'-').count();
    if name_len == 0 || !b[start].is_ascii_alphabetic() {
        return None;
    }
    let name = s[start..start + name_len].to_ascii_lowercase();
    // Find the closing '>' outside quotes.
    let mut quote = None;
    let mut end = None;
    for (k, c) in s.char_indices().skip(start + name_len) {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => {
                end = Some(k);
                break;
            }
            (None, '<') => return None, // `a < b<c`: not a tag
            _ => {}
        }
    }
    let Some(end) = end else { return Some(None) };
    let raw = s[..end + 1].to_string();
    if close {
        return Some(Some((Token::Close { name, raw }, end + 1)));
    }
    let attrs = attributes(&s[start + name_len..end]);
    Some(Some((Token::Open { name, attrs, raw }, end + 1)))
}

fn attributes(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s.trim_start_matches([' ', '\t', '\r', '\n', '/']);
    while !rest.is_empty() {
        let klen = rest.find(|c: char| c.is_whitespace() || c == '=' || c == '/').unwrap_or(rest.len());
        let key = rest[..klen].to_ascii_lowercase();
        rest = rest[klen..].trim_start();
        let mut value = String::new();
        if let Some(r) = rest.strip_prefix('=') {
            let r = r.trim_start();
            let (v, after) = match r.chars().next() {
                Some(q @ ('"' | '\'')) => match r[1..].find(q) {
                    Some(e) => (&r[1..1 + e], &r[2 + e..]),
                    None => (&r[1..], ""),
                },
                _ => {
                    let e = r.find(char::is_whitespace).unwrap_or(r.len());
                    // `<img width=80/>`: the slash ends the tag, not the value.
                    let v = if e == r.len() { r.trim_end_matches('/') } else { &r[..e] };
                    (v, &r[e..])
                }
            };
            value = decode_entities(v);
            rest = after;
        }
        if !key.is_empty() {
            out.push((key, value));
        }
        rest = rest.trim_start_matches([' ', '\t', '\r', '\n', '/']);
    }
    out
}

/// `&amp;`, `&lt;`, `&#169;`, `&#x1F600;` and a few named entities. Others stay as written.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        // Only the next 10 bytes: a search to the end for each `&` is quadratic.
        let semi = rest.as_bytes()[1..].iter().take(10).position(|&c| c == b';').map(|n| n + 1);
        let decoded = semi.and_then(|n| {
            let ent = &rest[1..n];
            let ch = if let Some(num) = ent.strip_prefix('#') {
                let v = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => num.parse().ok(),
                };
                v.and_then(char::from_u32)
            } else {
                Some(match ent {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    "nbsp" => '\u{A0}',
                    "copy" => '\u{A9}',
                    "reg" => '\u{AE}',
                    "trade" => '\u{2122}',
                    "mdash" => '\u{2014}',
                    "ndash" => '\u{2013}',
                    "hellip" => '\u{2026}',
                    "middot" => '\u{B7}',
                    "times" => '\u{D7}',
                    "rarr" => '\u{2192}',
                    "larr" => '\u{2190}',
                    _ => return None,
                })
            };
            ch.map(|c| (c, n + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(s: &str) -> Vec<String> {
        tokens(s)
            .0
            .iter()
            .map(|t| match t {
                Token::Open { name, .. } => format!("<{name}>"),
                Token::Close { name, .. } => format!("</{name}>"),
                Token::Text(t) => t.clone(),
            })
            .collect()
    }

    #[test]
    fn tags_text_and_attributes_are_split() {
        let s = r#"<p align="center">Hi <b>there</b><img src='a.png' alt=Logo width=80/></p>"#;
        let (t, used) = tokens(s);
        assert_eq!(used, s.len());
        assert_eq!(t[0].attr("align"), Some("center"));
        assert_eq!(t[5].attr("src"), Some("a.png"));
        assert_eq!(t[5].attr("alt"), Some("Logo"));
        assert_eq!(t[5].attr("width"), Some("80"));
        assert_eq!(names(r#"<P ALIGN="center">Hi <b>there</b></p>"#), ["<p>", "Hi ", "<b>", "there", "</b>", "</p>"]);
    }

    #[test]
    fn comments_are_dropped() {
        assert_eq!(names("a<!-- hidden -->b"), ["ab"]);
    }

    #[test]
    fn an_unclosed_tag_or_comment_waits_for_the_next_line() {
        let s = "text <img src=\"a.png\"";
        let (t, used) = tokens(s);
        assert_eq!(used, 5);
        assert_eq!(t, vec![Token::Text("text ".into())]);
        assert_eq!(tokens("<!-- open").1, 0);
    }

    #[test]
    fn a_less_than_sign_that_starts_no_tag_is_text() {
        assert_eq!(names("1 < 2 and <3"), ["1 < 2 and <3"]);
    }

    #[test]
    fn a_quoted_greater_than_sign_does_not_end_the_tag() {
        let (t, _) = tokens(r#"<a href="x>y">z</a>"#);
        assert_eq!(t[0].attr("href"), Some("x>y"));
    }

    #[test]
    fn many_ampersands_with_no_semicolon_decode_in_linear_time() {
        let s = "&".repeat(1_000_000);
        assert_eq!(decode_entities(&s), s);
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(decode_entities("a &amp; b &lt;c&gt; &#169; &#x1F600; &nope; &"), "a & b <c> \u{A9} \u{1F600} &nope; &");
    }
}
