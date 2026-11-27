//! YAML front matter for the Markdown preview, which shows it as GitHub does:
//! a table of keys and values, with nested tables for nested values.
//!
//! Ceiling: a subset of YAML. Block mappings and sequences, flow `[...]` and
//! `{...}`, quoted and plain scalars, `|` and `>` blocks, comments. Anchors,
//! aliases, tags, tabs for indentation and multiple documents give None, and
//! the preview then shows the front matter as its text.

#[derive(Debug, PartialEq, Clone)]
pub enum Value {
    Str(String),
    Seq(Vec<Value>),
    Map(Vec<(String, Value)>),
}

struct Line<'a> {
    indent: usize,
    text: &'a str,
}

/// Parse front matter (the lines between the `---` lines).
pub fn parse(src: &str) -> Option<Value> {
    let mut lines = Vec::new();
    for raw in src.lines() {
        let raw = raw.trim_end_matches('\r');
        let body = raw.trim_start_matches(' ');
        if body.starts_with('\t') {
            return None;
        }
        lines.push(Line { indent: raw.len() - body.len(), text: body });
    }
    let mut p = Parser { lines, at: 0 };
    p.skip_blank();
    if p.at >= p.lines.len() {
        return None;
    }
    let indent = p.lines[p.at].indent;
    let v = p.block(indent)?;
    p.skip_blank();
    (p.at == p.lines.len()).then_some(v)
}

struct Parser<'a> {
    lines: Vec<Line<'a>>,
    at: usize,
}

fn is_blank(t: &str) -> bool {
    t.is_empty() || t.starts_with('#')
}

impl<'a> Parser<'a> {
    fn skip_blank(&mut self) {
        while self.at < self.lines.len() && is_blank(self.lines[self.at].text) {
            self.at += 1;
        }
    }

    /// The node whose first line is at the current line, with this indent.
    fn block(&mut self, indent: usize) -> Option<Value> {
        let t = self.lines[self.at].text;
        if t == "-" || t.starts_with("- ") {
            self.seq(indent)
        } else if split_key(t).is_some() {
            self.map(indent)
        } else {
            None
        }
    }

    fn seq(&mut self, indent: usize) -> Option<Value> {
        let mut items = Vec::new();
        loop {
            self.skip_blank();
            let Some(line) = self.lines.get(self.at) else { break };
            if line.indent != indent || !(line.text == "-" || line.text.starts_with("- ")) {
                break;
            }
            let rest = line.text[1..].trim_start();
            let item_indent = indent + (line.text.len() - rest.len());
            if is_blank(rest) {
                self.at += 1;
                items.push(self.nested(indent)?);
            } else if rest.starts_with("- ") || split_key(rest).is_some() && !rest.starts_with(['"', '\'', '[', '{']) {
                // `- key: v` or `- - x`: a node that starts on the dash's line.
                self.lines[self.at] = Line { indent: item_indent, text: rest };
                items.push(self.block(item_indent)?);
            } else {
                self.at += 1;
                items.push(self.scalar_or_flow(rest, indent)?);
            }
        }
        Some(Value::Seq(items))
    }

    fn map(&mut self, indent: usize) -> Option<Value> {
        let mut entries = Vec::new();
        loop {
            self.skip_blank();
            let Some(line) = self.lines.get(self.at) else { break };
            if line.indent != indent {
                if line.indent > indent {
                    return None;
                }
                break;
            }
            let (key, rest) = split_key(line.text)?;
            self.at += 1;
            let value = if is_blank(rest) {
                self.nested(indent)?
            } else {
                self.scalar_or_flow(rest, indent)?
            };
            entries.push((key, value));
        }
        Some(Value::Map(entries))
    }

    /// The value under a `key:` or `-` with nothing after it: a deeper block,
    /// a sequence at the same indent (YAML allows `key:` then `- x`), or empty.
    fn nested(&mut self, indent: usize) -> Option<Value> {
        self.skip_blank();
        match self.lines.get(self.at) {
            Some(l) if l.indent > indent => {
                let i = l.indent;
                self.block(i)
            }
            Some(l) if l.indent == indent && (l.text == "-" || l.text.starts_with("- ")) && self.parent_is_map(indent) => self.seq(indent),
            _ => Some(Value::Str(String::new())),
        }
    }

    /// The line before the current block is a `key:` line at this indent.
    fn parent_is_map(&self, indent: usize) -> bool {
        self.lines[..self.at].iter().rev().find(|l| !is_blank(l.text)).is_some_and(|l| l.indent == indent && split_key(l.text).is_some())
    }

    /// A value on the same line as its key or dash, plus any lines that continue it.
    fn scalar_or_flow(&mut self, rest: &str, indent: usize) -> Option<Value> {
        let rest = strip_comment(rest);
        if rest.starts_with(['&', '*', '!']) {
            return None;
        }
        if let Some(style) = rest.strip_prefix(['|', '>']) {
            let folded = rest.starts_with('>');
            return Some(Value::Str(self.block_scalar(indent, folded, style)));
        }
        // A value can go on over more lines: a flow collection until its
        // brackets balance, a quoted value until its quote closes, a plain
        // value while the next lines are more indented.
        let mut text = rest.to_string();
        let opener = text.chars().next();
        while let Some(l) = self.lines.get(self.at) {
            let more = match opener {
                Some('[' | '{') => !balanced(&text),
                Some(q @ ('"' | '\'')) => !closed_quote(&text, q),
                _ if l.text.is_empty() || l.indent <= indent => false,
                // `a: 1` then `  b: 2` is not YAML.
                _ if split_key(l.text).is_some() => return None,
                _ => true,
            };
            if !more {
                break;
            }
            if !l.text.is_empty() {
                text.push(' ');
                text.push_str(l.text);
            }
            self.at += 1;
        }
        flow(text.trim())
    }

    fn block_scalar(&mut self, indent: usize, folded: bool, style: &str) -> String {
        let keep = style.contains('+');
        let strip = style.contains('-');
        // Ceiling: lines indented deeper than the first lose that extra indent.
        let start = self.at;
        while self.lines.get(self.at).is_some_and(|l| l.text.is_empty() || l.indent > indent) {
            self.at += 1;
        }
        let mut body: Vec<&str> = self.lines[start..self.at].iter().map(|l| l.text).collect();
        while !keep && body.last() == Some(&"") {
            body.pop();
        }
        let mut s = if folded {
            let mut out = String::new();
            for (i, line) in body.iter().enumerate() {
                if i > 0 {
                    out.push(if line.is_empty() || body[i - 1].is_empty() { '\n' } else { ' ' });
                }
                out.push_str(line);
            }
            out
        } else {
            body.join("\n")
        };
        if !strip && !s.is_empty() {
            s.push('\n');
        }
        s.trim_end_matches('\n').to_string()
    }
}

/// `key: rest` → (key, rest). The key may be quoted.
fn split_key(t: &str) -> Option<(String, &str)> {
    let (key, after) = if let Some(q) = t.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let end = t[1..].find(q)? + 1;
        (t[1..end].to_string(), &t[end + 1..])
    } else {
        let i = t.find(": ").or_else(|| t.ends_with(':').then(|| t.len() - 1))?;
        if t[..i].contains(" #") || t.starts_with(['[', '{', '-', '#']) {
            return None;
        }
        (t[..i].trim().to_string(), &t[i..])
    };
    let rest = after.strip_prefix(':')?;
    if !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    Some((key, rest.trim()))
}

fn strip_comment(s: &str) -> &str {
    let mut quote = None;
    let b = s.as_bytes();
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') if i == 0 || b[i - 1] == b' ' || b[i - 1] == b'[' || b[i - 1] == b',' => quote = Some(c),
            (None, '#') if i > 0 && b[i - 1] == b' ' => return s[..i].trim_end(),
            _ => {}
        }
    }
    s
}

fn balanced(s: &str) -> bool {
    let mut depth = 0i32;
    let mut quote = None;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '[' | '{') => depth += 1,
            (None, ']' | '}') => depth -= 1,
            _ => {}
        }
    }
    depth <= 0
}

fn closed_quote(s: &str, q: char) -> bool {
    let body = &s[1..];
    if q == '\'' {
        body.replace("''", "").contains('\'')
    } else {
        let mut esc = false;
        for c in body.chars() {
            match c {
                '\\' if !esc => esc = true,
                c if c == q && !esc => return true,
                _ => esc = false,
            }
        }
        false
    }
}

/// A flow collection or a scalar.
fn flow(s: &str) -> Option<Value> {
    let mut chars = s.char_indices().peekable();
    let v = flow_node(s, &mut chars)?;
    match chars.peek() {
        None => Some(v),
        // A plain scalar with flow characters in it: `a [b]`.
        Some(_) if !s.starts_with(['[', '{']) => Some(Value::Str(s.to_string())),
        Some(_) => None,
    }
}

type Chars<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

fn skip_ws(it: &mut Chars) {
    while it.peek().is_some_and(|(_, c)| c.is_whitespace()) {
        it.next();
    }
}

fn flow_node(s: &str, it: &mut Chars) -> Option<Value> {
    skip_ws(it);
    match it.peek()?.1 {
        '[' => {
            it.next();
            let mut items = Vec::new();
            loop {
                skip_ws(it);
                if it.peek()?.1 == ']' {
                    it.next();
                    return Some(Value::Seq(items));
                }
                items.push(flow_item(s, it, &[',', ']'])?);
                skip_ws(it);
                match it.next()?.1 {
                    ',' => {}
                    ']' => return Some(Value::Seq(items)),
                    _ => return None,
                }
            }
        }
        '{' => {
            it.next();
            let mut entries = Vec::new();
            loop {
                skip_ws(it);
                if it.peek()?.1 == '}' {
                    it.next();
                    return Some(Value::Map(entries));
                }
                let key = match flow_item(s, it, &[':', ',', '}'])? {
                    Value::Str(k) => k,
                    _ => return None,
                };
                skip_ws(it);
                let value = if it.peek()?.1 == ':' {
                    it.next();
                    flow_item(s, it, &[',', '}'])?
                } else {
                    Value::Str(String::new())
                };
                entries.push((key, value));
                skip_ws(it);
                match it.next()?.1 {
                    ',' => {}
                    '}' => return Some(Value::Map(entries)),
                    _ => return None,
                }
            }
        }
        _ => {
            let start = it.peek()?.0;
            for _ in it.by_ref() {}
            Some(scalar(&s[start..]))
        }
    }
}

/// An item of a flow collection, up to one of `stops`.
fn flow_item(s: &str, it: &mut Chars, stops: &[char]) -> Option<Value> {
    skip_ws(it);
    let (start, c) = *it.peek()?;
    if c == '[' || c == '{' {
        return flow_node(s, it);
    }
    if c == '"' || c == '\'' {
        it.next();
        let mut esc = false;
        let mut end = None;
        while let Some((i, ch)) = it.next() {
            if c == '\'' && ch == '\'' {
                if it.peek().is_some_and(|(_, n)| *n == '\'') {
                    it.next();
                    continue;
                }
                end = Some(i);
                break;
            }
            if c == '"' {
                if esc {
                    esc = false;
                    continue;
                }
                if ch == '\\' {
                    esc = true;
                    continue;
                }
                if ch == '"' {
                    end = Some(i);
                    break;
                }
            }
        }
        return Some(scalar(&s[start..=end?]));
    }
    let mut end = s.len();
    while let Some(&(i, ch)) = it.peek() {
        if stops.contains(&ch) && (ch != ':' || s[i + 1..].starts_with([' ', ',', '}']) || i + 1 == s.len()) {
            end = i;
            break;
        }
        it.next();
    }
    Some(Value::Str(s[start..end].trim().to_string()))
}

/// A scalar as GitHub shows it: quotes removed, escapes decoded, `null` and `~` empty.
fn scalar(s: &str) -> Value {
    let s = s.trim();
    let text = if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        s[1..s.len() - 1].replace("''", "'")
    } else if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        let mut out = String::new();
        let mut it = s[1..s.len() - 1].chars();
        while let Some(c) = it.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => {}
            }
        }
        out
    } else if s == "~" || s == "null" {
        String::new()
    } else {
        s.to_string()
    };
    Value::Str(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Value {
        Value::Str(v.into())
    }

    fn map(e: &[(&str, Value)]) -> Value {
        Value::Map(e.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
    }

    #[test]
    fn a_flat_mapping_parses_in_order() {
        assert_eq!(parse("title: Notes\nlayout: post\n"), Some(map(&[("title", s("Notes")), ("layout", s("post"))])));
    }

    #[test]
    fn quoted_values_lose_their_quotes() {
        let v = parse("a: 'it''s'\nb: \"say \\\"hi\\\"\"\nc: x # comment").unwrap();
        assert_eq!(v, map(&[("a", s("it's")), ("b", s("say \"hi\"")), ("c", s("x"))]));
    }

    #[test]
    fn block_and_flow_sequences_parse() {
        let v = parse("tags: [rust, \"win 32\"]\nlist:\n  - one\n  - two\nsame:\n- x\n").unwrap();
        assert_eq!(
            v,
            map(&[("tags", Value::Seq(vec![s("rust"), s("win 32")])), ("list", Value::Seq(vec![s("one"), s("two")])), ("same", Value::Seq(vec![s("x")]))])
        );
    }

    #[test]
    fn nested_mappings_and_maps_in_sequences_parse() {
        let v = parse("versions:\n  fpt: '*'\n  ghes: '*'\nauthors:\n  - name: A\n    id: 1\n  - name: B\n").unwrap();
        assert_eq!(
            v,
            map(&[
                ("versions", map(&[("fpt", s("*")), ("ghes", s("*"))])),
                ("authors", Value::Seq(vec![map(&[("name", s("A")), ("id", s("1"))]), map(&[("name", s("B"))])])),
            ])
        );
    }

    #[test]
    fn block_scalars_keep_or_fold_their_lines() {
        let v = parse("a: |\n  one\n  two\nb: >\n  one\n  two\nc: 3\n").unwrap();
        assert_eq!(v, map(&[("a", s("one\ntwo")), ("b", s("one two")), ("c", s("3"))]));
    }

    #[test]
    fn a_plain_value_can_go_on_over_more_lines() {
        assert_eq!(parse("intro: one\n  two\nnext: x").unwrap(), map(&[("intro", s("one two")), ("next", s("x"))]));
    }

    #[test]
    fn empty_and_null_values_are_empty() {
        assert_eq!(parse("a:\nb: ~\nc: null").unwrap(), map(&[("a", s("")), ("b", s("")), ("c", s(""))]));
    }

    #[test]
    fn a_flow_mapping_parses() {
        assert_eq!(parse("m: {a: 1, b: [x, y]}").unwrap(), map(&[("m", map(&[("a", s("1")), ("b", Value::Seq(vec![s("x"), s("y")]))]))]));
    }

    #[test]
    fn urls_and_times_stay_one_value() {
        assert_eq!(parse("url: https://x.y/z\nat: 10:30").unwrap(), map(&[("url", s("https://x.y/z")), ("at", s("10:30"))]));
    }

    #[test]
    fn what_the_subset_cannot_read_gives_none() {
        for src in ["a: &x 1\nb: *x", "a:\n\t- tab", "just text", "a: 1\n  b: 2", "a: [1, 2"] {
            assert_eq!(parse(src), None, "{src:?}");
        }
    }
}
