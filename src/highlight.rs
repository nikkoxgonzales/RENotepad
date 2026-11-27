//! Syntax colors for fenced code blocks in the Markdown preview, in the roles
//! of GitHub's `pl-*` classes. Ceiling: a token scanner per language family
//! (comments, strings, numbers, keywords, calls), not a grammar, so it can
//! color a word that GitHub's grammars would not.

pub const COMMENT: u8 = 0;
pub const KEYWORD: u8 = 1;
pub const STRING: u8 = 2;
pub const CONSTANT: u8 = 3;
pub const ENTITY: u8 = 4;
pub const VARIABLE: u8 = 5;
pub const TAG: u8 = 6;
pub const INSERTED: u8 = 7;
pub const DELETED: u8 = 8;

pub type Span<'t> = (Option<u8>, &'t str);

struct Lang {
    line: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    quotes: &'static str,
    keywords: &'static [&'static str],
    constants: &'static [&'static str],
    /// `$name` is a variable (shells).
    dollar: bool,
    /// Keywords match in any case (SQL).
    nocase: bool,
    /// A string or word before `:` is a key (JSON, YAML).
    keys: bool,
}

const C_LIKE: &[&str] = &[
    "auto", "break", "case", "catch", "char", "class", "const", "continue", "default", "delete", "do", "double", "else", "enum", "explicit", "extern", "float", "for", "friend", "goto", "if",
    "inline", "int", "long", "namespace", "new", "operator", "private", "protected", "public", "register", "return", "short", "signed", "sizeof", "static", "struct", "switch", "template",
    "this", "throw", "try", "typedef", "typename", "union", "unsigned", "using", "virtual", "void", "volatile", "while", "abstract", "as", "async", "await", "base", "bool", "byte",
    "decimal", "event", "extends", "final", "finally", "fixed", "foreach", "implements", "import", "in", "instanceof", "interface", "internal", "is", "let", "lock", "object", "out",
    "override", "package", "params", "readonly", "ref", "sealed", "string", "super", "synchronized", "throws", "transient", "typeof", "var", "function", "export", "from", "of", "yield",
    "type", "keyof", "declare", "module", "get", "set", "func", "go", "chan", "defer", "map", "range", "select", "fallthrough", "val", "fun", "when", "where", "elif",
];
const C_CONST: &[&str] = &["true", "false", "null", "nullptr", "NULL", "undefined", "nil", "NaN", "Infinity", "iota"];
const RUST: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "type", "unsafe", "use", "where", "while",
];
const RUST_CONST: &[&str] = &["true", "false", "None", "Some", "Ok", "Err"];
const PYTHON: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda",
    "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield", "match", "case", "self",
];
const PY_CONST: &[&str] = &["True", "False", "None"];
const SHELL: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in", "function", "return", "export", "local", "readonly", "set", "unset", "echo", "cd",
    "source", "exit", "sudo",
];
const POWERSHELL: &[&str] = &[
    "if", "else", "elseif", "foreach", "for", "while", "do", "until", "switch", "function", "filter", "param", "return", "try", "catch", "finally", "throw", "begin", "process", "end",
    "break", "continue", "class", "enum", "in", "exit", "trap",
];
const SQL: &[&str] = &[
    "select", "from", "where", "insert", "into", "values", "update", "set", "delete", "create", "table", "drop", "alter", "add", "join", "inner", "left", "right", "outer", "full", "on",
    "and", "or", "not", "as", "order", "by", "group", "having", "limit", "offset", "primary", "key", "foreign", "references", "index", "view", "distinct", "union", "all", "case",
    "when", "then", "else", "end", "is", "in", "like", "between", "exists", "default", "unique", "begin", "commit", "rollback",
];
const SQL_CONST: &[&str] = &["null", "true", "false"];
const YAML_CONST: &[&str] = &["true", "false", "null", "yes", "no", "on", "off", "~"];

fn spec(lang: &str) -> Option<Lang> {
    let base = Lang { line: &["//"], block: Some(("/*", "*/")), quotes: "\"'`", keywords: C_LIKE, constants: C_CONST, dollar: false, nocase: false, keys: false };
    Some(match lang {
        "rust" | "rs" => Lang { quotes: "\"", keywords: RUST, constants: RUST_CONST, ..base },
        "c" | "h" | "cpp" | "c++" | "cc" | "hpp" | "cxx" | "cs" | "csharp" | "c#" | "java" | "kotlin" | "kt" | "swift" | "go" | "golang" | "js" | "javascript" | "jsx" | "mjs" | "cjs"
        | "ts" | "typescript" | "tsx" | "dart" | "scala" | "groovy" | "php" | "zig" => base,
        "json" | "jsonc" | "json5" | "jsonl" | "ndjson" => Lang { line: &["//"], quotes: "\"", keywords: &[], keys: true, ..base },
        "python" | "py" | "python3" => Lang { line: &["#"], block: None, quotes: "\"'", keywords: PYTHON, constants: PY_CONST, ..base },
        "ruby" | "rb" => Lang { line: &["#"], block: None, quotes: "\"'", keywords: PYTHON, constants: &["true", "false", "nil"], ..base },
        "sh" | "bash" | "shell" | "zsh" | "console" | "shellsession" | "fish" => {
            Lang { line: &["#"], block: None, quotes: "\"'", keywords: SHELL, constants: &["true", "false"], dollar: true, ..base }
        }
        "powershell" | "ps1" | "pwsh" | "ps" => {
            Lang { line: &["#"], block: Some(("<#", "#>")), quotes: "\"'", keywords: POWERSHELL, constants: &["$true", "$false", "$null"], dollar: true, nocase: true, ..base }
        }
        "bat" | "cmd" | "batch" => Lang { line: &["REM ", "rem ", "::"], block: None, quotes: "\"", keywords: SHELL, dollar: false, nocase: true, ..base },
        "sql" | "mysql" | "postgresql" | "sqlite" | "tsql" => Lang { line: &["--"], quotes: "'\"", keywords: SQL, constants: SQL_CONST, nocase: true, ..base },
        "yaml" | "yml" => Lang { line: &["#"], block: None, quotes: "\"'", keywords: &[], constants: YAML_CONST, keys: true, ..base },
        "toml" | "ini" | "cfg" | "conf" | "properties" | "editorconfig" => {
            Lang { line: &["#", ";"], block: None, quotes: "\"'", keywords: &[], constants: &["true", "false"], keys: true, ..base }
        }
        "css" | "scss" | "less" => Lang { line: &["//"], quotes: "\"'", keywords: &["important", "media", "import", "keyframes"], constants: &[], keys: true, ..base },
        "lua" => Lang { line: &["--"], block: Some(("--[[", "]]")), quotes: "\"'", keywords: C_LIKE, constants: &["true", "false", "nil"], ..base },
        "makefile" | "make" | "dockerfile" | "docker" | "r" | "perl" | "pl" | "nim" | "elixir" | "ex" => {
            Lang { line: &["#"], block: None, quotes: "\"'", keywords: SHELL, dollar: true, ..base }
        }
        _ => return None,
    })
}

/// The code cut into spans, each with its color role (None: the plain text color).
pub fn spans<'t>(lang: &str, code: &'t str) -> Vec<Span<'t>> {
    let lang = lang.to_ascii_lowercase();
    match lang.as_str() {
        "diff" | "patch" => diff(code),
        "html" | "xml" | "svg" | "xhtml" | "htm" | "vue" | "xaml" | "plist" | "csproj" => markup(code),
        l => match spec(l) {
            Some(s) => scan(code, &s),
            None => vec![(None, code)],
        },
    }
}

fn push<'t>(out: &mut Vec<Span<'t>>, role: Option<u8>, s: &'t str, whole: &'t str, start: usize) {
    if s.is_empty() {
        return;
    }
    // Plain spans next to each other merge.
    if role.is_none() {
        if let Some((None, prev)) = out.last_mut() {
            let p = prev.as_ptr() as usize - whole.as_ptr() as usize;
            if p + prev.len() == start {
                *prev = &whole[p..start + s.len()];
                return;
            }
        }
    }
    out.push((role, s));
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

fn scan<'t>(code: &'t str, l: &Lang) -> Vec<Span<'t>> {
    let b = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut plain = 0; // start of the plain text not pushed yet
    macro_rules! emit {
        ($role:expr, $end:expr) => {{
            push(&mut out, None, &code[plain..i], code, plain);
            push(&mut out, Some($role), &code[i..$end], code, i);
            i = $end;
            plain = i;
            continue;
        }};
    }
    while i < b.len() {
        let rest = &code[i..];
        if let Some((open, close)) = l.block {
            if let Some(body) = rest.strip_prefix(open) {
                let end = body.find(close).map_or(code.len(), |n| i + open.len() + n + close.len());
                emit!(COMMENT, end);
            }
        }
        if l.line.iter().any(|p| rest.starts_with(p)) && (i == 0 || !is_ident(b[i - 1]) || l.line[0] != "#") {
            let end = rest.find('\n').map_or(code.len(), |n| i + n);
            emit!(COMMENT, end);
        }
        let c = b[i];
        if l.quotes.as_bytes().contains(&c) {
            let triple = rest.len() >= 3 && rest.as_bytes()[1] == c && rest.as_bytes()[2] == c && l.line[0] == "#";
            let end = string_end(code, i, c, triple);
            let after = code[end..].trim_start_matches([' ', '\t']);
            let role = if l.keys && after.starts_with(':') { TAG } else { STRING };
            emit!(role, end);
        }
        if c.is_ascii_digit() && (i == 0 || !is_ident(b[i - 1])) {
            let len = rest.bytes().take_while(|&d| d.is_ascii_alphanumeric() || d == b'.' || d == b'_').count();
            emit!(CONSTANT, i + len);
        }
        if l.dollar && c == b'$' && b.get(i + 1).is_some_and(|&n| is_ident(n) || n == b'{') {
            let len = 1 + rest[1..].bytes().take_while(|&d| is_ident(d) || d == b'{' || d == b'}' || d == b':').count();
            let word = &code[i..i + len];
            let role = if l.constants.iter().any(|k| k.eq_ignore_ascii_case(word)) { CONSTANT } else { VARIABLE };
            emit!(role, i + len);
        }
        if (c.is_ascii_alphabetic() || c == b'_' || c == b'@') && (i == 0 || !is_ident(b[i - 1])) {
            let len = 1 + rest[1..].bytes().take_while(|&d| is_ident(d) || d == b'-' && l.nocase && l.dollar).count();
            let word = &code[i..i + len];
            let eq = |k: &&str| if l.nocase { k.eq_ignore_ascii_case(word) } else { **k == *word };
            let next = code[i + len..].trim_start_matches([' ', '\t']).as_bytes().first().copied();
            let role = if l.keys && next == Some(b':') && !l.keywords.iter().any(eq) && (l.constants.is_empty() || l.line[0] == "#") {
                Some(TAG)
            } else if l.keys && next == Some(b'=') && l.line.contains(&";") {
                Some(VARIABLE)
            } else if l.keywords.iter().any(eq) {
                Some(KEYWORD)
            } else if l.constants.iter().any(eq) {
                Some(CONSTANT)
            } else if next == Some(b'(') || next == Some(b'!') && l.quotes == "\"" || word.contains('-') && word.as_bytes()[0].is_ascii_uppercase() {
                Some(ENTITY) // a call, a Rust macro, or a PowerShell Verb-Noun command
            } else {
                None
            };
            if let Some(r) = role {
                emit!(r, i + len);
            }
            i += len;
            continue;
        }
        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    push(&mut out, None, &code[plain..], code, plain);
    out
}

/// The end of a string that starts at `i` with quote `q`: the closing quote,
/// or the line end for an unclosed one (a backtick string may span lines).
fn string_end(code: &str, i: usize, q: u8, triple: bool) -> usize {
    let b = code.as_bytes();
    if triple {
        let pat = [q, q, q];
        let pat = std::str::from_utf8(&pat).unwrap_or("\"\"\"");
        return code[i + 3..].find(pat).map_or(code.len(), |n| i + 3 + n + 3);
    }
    let mut k = i + 1;
    while k < b.len() {
        match b[k] {
            b'\\' => k += 2,
            c if c == q => return k + 1,
            b'\n' if q != b'`' => return k,
            _ => k += 1,
        }
    }
    code.len()
}

fn diff(code: &str) -> Vec<Span<'_>> {
    let mut out = Vec::new();
    for line in code.split_inclusive('\n') {
        let role = match line.as_bytes().first() {
            Some(b'+') if !line.starts_with("+++") => Some(INSERTED),
            Some(b'-') if !line.starts_with("---") => Some(DELETED),
            Some(b'@') => Some(ENTITY),
            _ if line.starts_with("diff ") || line.starts_with("index ") || line.starts_with("+++") || line.starts_with("---") => Some(KEYWORD),
            _ => None,
        };
        let start = line.as_ptr() as usize - code.as_ptr() as usize;
        push(&mut out, role, line, code, start);
    }
    out
}

fn markup(code: &str) -> Vec<Span<'_>> {
    fn flush<'t>(out: &mut Vec<Span<'t>>, code: &'t str, plain: usize, i: usize) {
        push(out, None, &code[plain..i], code, plain);
    }
    let b = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut plain = 0;
    let mut in_tag = false;
    while i < b.len() {
        let rest = &code[i..];
        if !in_tag && rest.starts_with("<!--") {
            flush(&mut out, code, plain, i);
            let end = rest.find("-->").map_or(code.len(), |n| i + n + 3);
            push(&mut out, Some(COMMENT), &code[i..end], code, i);
            i = end;
            plain = i;
            continue;
        }
        if !in_tag && b[i] == b'<' && b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'/' || *c == b'!' || *c == b'?') {
            let start = i + 1 + usize::from(matches!(b[i + 1], b'/' | b'!' | b'?'));
            let len = code[start..].bytes().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b':' | b'_' | b'.')).count();
            flush(&mut out, code, plain, start);
            push(&mut out, Some(TAG), &code[start..start + len], code, start);
            i = start + len;
            plain = i;
            in_tag = true;
            continue;
        }
        if in_tag {
            match b[i] {
                b'>' => in_tag = false,
                q @ (b'"' | b'\'') => {
                    flush(&mut out, code, plain, i);
                    let end = code[i + 1..].find(q as char).map_or(code.len(), |n| i + 1 + n + 1);
                    push(&mut out, Some(STRING), &code[i..end], code, i);
                    i = end;
                    plain = i;
                    continue;
                }
                c if c.is_ascii_alphabetic() => {
                    flush(&mut out, code, plain, i);
                    let len = rest.bytes().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b':' | b'_')).count();
                    push(&mut out, Some(ENTITY), &code[i..i + len], code, i);
                    i += len;
                    plain = i;
                    continue;
                }
                _ => {}
            }
        }
        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    flush(&mut out, code, plain, code.len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles(lang: &str, code: &str) -> Vec<(Option<u8>, String)> {
        spans(lang, code).into_iter().map(|(r, s)| (r, s.to_string())).collect()
    }

    fn role_of(lang: &str, code: &str, word: &str) -> Option<u8> {
        spans(lang, code).into_iter().find(|(_, s)| *s == word).and_then(|(r, _)| r)
    }

    #[test]
    fn spans_cover_the_code_exactly() {
        for (lang, code) in [("rust", "fn main() { let x = \"hi\"; // c\n}"), ("python", "def f():\n    return 'a' # x"), ("html", "<a href=\"x\">t</a>"), ("diff", "+a\n-b\n c")] {
            let joined: String = spans(lang, code).iter().map(|(_, s)| *s).collect();
            assert_eq!(joined, code, "{lang}");
        }
    }

    #[test]
    fn rust_keywords_strings_comments_and_calls_get_roles() {
        let code = "fn main() { let n = 42; println!(\"hi\"); } // done";
        assert_eq!(role_of("rust", code, "fn"), Some(KEYWORD));
        assert_eq!(role_of("rust", code, "main"), Some(ENTITY));
        assert_eq!(role_of("rust", code, "42"), Some(CONSTANT));
        assert_eq!(role_of("rust", code, "println"), Some(ENTITY));
        assert_eq!(role_of("rust", code, "\"hi\""), Some(STRING));
        assert_eq!(role_of("rust", code, "// done"), Some(COMMENT));
    }

    #[test]
    fn json_keys_differ_from_string_values() {
        let code = "{\"name\": \"x\", \"on\": true}";
        assert_eq!(role_of("json", code, "\"name\""), Some(TAG));
        assert_eq!(role_of("json", code, "\"x\""), Some(STRING));
        assert_eq!(role_of("json", code, "true"), Some(CONSTANT));
    }

    #[test]
    fn shell_variables_and_powershell_commands_get_roles() {
        assert_eq!(role_of("bash", "echo $HOME", "$HOME"), Some(VARIABLE));
        assert_eq!(role_of("powershell", "Get-Item $p", "Get-Item"), Some(ENTITY));
        assert_eq!(role_of("powershell", "if ($x -eq $true) {}", "$true"), Some(CONSTANT));
    }

    #[test]
    fn markup_tags_attributes_and_values_get_roles() {
        let code = "<!-- c --><a href=\"x\">t</a>";
        assert_eq!(role_of("html", code, "<!-- c -->"), Some(COMMENT));
        assert_eq!(role_of("html", code, "a"), Some(TAG));
        assert_eq!(role_of("html", code, "href"), Some(ENTITY));
        assert_eq!(role_of("html", code, "\"x\""), Some(STRING));
    }

    #[test]
    fn diff_lines_are_inserted_or_deleted() {
        assert_eq!(roles("diff", "+a\n-b\n"), vec![(Some(INSERTED), "+a\n".into()), (Some(DELETED), "-b\n".into())]);
    }

    #[test]
    fn an_unknown_language_stays_plain() {
        assert_eq!(roles("klingon", "fn x"), vec![(None, "fn x".into())]);
    }
}
