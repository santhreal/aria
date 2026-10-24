//! Source text with its test-only items blanked, for the censuses that read
//! the code a release build compiles, and the string literals of a source, for
//! the censuses that read the text it writes. [`production_sources`] reads
//! every production source in the crate that way.
//!
//! A test-only item is an item, statement, field, or arm under `#[cfg(test)]`,
//! `#[test]`, or `#[gpui::test]`. It can sit anywhere in a file: a
//! `#[cfg(test)]` re-export above the draw code, a recording statement inside a
//! paint, an inline test module at the end. Cutting a file at its first
//! `#[cfg(test)]` drops every production line after that point from the
//! census, so each test-only item is blanked on its own.
//!
//! The scan is line-wise over code with comments and literal contents removed,
//! so a bracket in a string, a character literal, or a comment does not move
//! the item's end. An item ends on the first line where its brackets balance
//! and the line ends in `;` or `}`, or in `,` for a field, variant, or arm.

/// The attributes that make an item build only under `cfg(test)`.
const TEST_ATTRIBUTES: [&str; 4] = ["#[cfg(test)]", "#[test]", "#[gpui::test]", "#[gpui::test("];

/// `text` with each test-only item replaced by empty lines, so its line
/// numbers still match the file.
pub(crate) fn without_test_items(text: &str) -> String {
    let code = code_lines(text);
    let mut keep = vec![true; code.len()];
    let mut line = 0;
    while line < code.len() {
        let start = code[line].trim_start();
        if !TEST_ATTRIBUTES
            .iter()
            .any(|attribute| start.starts_with(attribute))
        {
            line += 1;
            continue;
        }
        let mut depth = 0i32;
        let mut declares_fn = false;
        let mut has_where = false;
        while line < code.len() {
            let current = code[line].as_str();
            keep[line] = false;
            for c in current.chars() {
                match c {
                    '{' | '(' | '[' => depth += 1,
                    '}' | ')' | ']' => depth -= 1,
                    _ => {}
                }
            }
            let trimmed = current.trim();
            declares_fn |= current.contains("fn ");
            has_where |= trimmed == "where" || trimmed.starts_with("where ");
            line += 1;
            let ends = match trimmed.chars().last() {
                Some(';' | '}') => true,
                // A `where` bound or a parameter list ends a line in `,` inside
                // a function's signature; only a field, variant, or arm ends
                // its item there.
                Some(',') => !declares_fn && !has_where,
                _ => false,
            };
            if ends && depth <= 0 {
                break;
            }
        }
    }
    let mut out = String::with_capacity(text.len());
    for (source, keep) in text.lines().zip(keep) {
        if keep {
            out.push_str(source);
        }
        out.push('\n');
    }
    out
}

/// Every production source in the crate: its path relative to `src` with `/`
/// between its parts, and its text with test-only items and comments blanked.
/// The test files and the test support tree are left out.
pub(crate) fn production_sources() -> Vec<(String, String)> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    super::collect_rs(&src, &mut files);
    assert!(
        files.len() > 200,
        "found {} files under {src:?}, which cannot be this crate",
        files.len()
    );
    files
        .iter()
        .filter_map(|file| {
            let rel = file
                .strip_prefix(&src)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/");
            if rel.starts_with("test_support/") || rel.ends_with("_tests.rs") {
                return None;
            }
            let raw = std::fs::read_to_string(file)
                .unwrap_or_else(|err| panic!("failed to read {}: {err}", file.display()));
            Some((
                rel,
                super::control_sites::strip_comments(&without_test_items(&raw)),
            ))
        })
        .collect()
}

/// Each line of `text`, as `str::lines` splits it, with comments and the
/// contents of string and character literals replaced by spaces.
fn code_lines(text: &str) -> Vec<String> {
    lex(text, None)
}

/// Each string literal in `text`, raw and byte strings included: the line it
/// opens on, counted from 1, and its contents between the quotes with escapes
/// as written. A character literal is not a string literal, and neither is a
/// quote in a comment.
pub(crate) fn string_literals(text: &str) -> Vec<(usize, String)> {
    let mut literals = Vec::new();
    lex(text, Some(&mut literals));
    literals
}

/// Appends `c` to the literal being read, when literals are collected.
fn extend_literal(literals: &mut Option<&mut Vec<(usize, String)>>, c: char) {
    if let Some((_, contents)) = literals.as_deref_mut().and_then(|all| all.last_mut()) {
        contents.push(c);
    }
}

/// The lexer behind [`code_lines`] and [`string_literals`]: the blanked lines,
/// and each string literal pushed onto `literals` when they are collected.
fn lex(text: &str, mut literals: Option<&mut Vec<(usize, String)>>) -> Vec<String> {
    enum Lexed {
        Code,
        LineComment,
        BlockComment(u32),
        Str,
        RawStr(usize),
    }
    let chars: Vec<char> = text.chars().collect();
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut state = Lexed::Code;
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        let next = chars.get(at + 1).copied();
        if c == '\n' {
            lines.push(std::mem::take(&mut line));
            match state {
                Lexed::LineComment => state = Lexed::Code,
                Lexed::Str | Lexed::RawStr(_) => extend_literal(&mut literals, c),
                Lexed::Code | Lexed::BlockComment(_) => {}
            }
            at += 1;
            continue;
        }
        match state {
            Lexed::Code => match c {
                '/' if next == Some('/') => {
                    state = Lexed::LineComment;
                    line.push(' ');
                }
                '/' if next == Some('*') => {
                    state = Lexed::BlockComment(1);
                    line.push_str("  ");
                    at += 1;
                }
                '"' => {
                    state = Lexed::Str;
                    line.push('"');
                    if let Some(all) = literals.as_deref_mut() {
                        all.push((lines.len() + 1, String::new()));
                    }
                }
                'r' if at == 0
                    || !is_ident(chars[at - 1])
                    || (chars[at - 1] == 'b' && (at < 2 || !is_ident(chars[at - 2]))) =>
                {
                    let hashes = chars[at + 1..].iter().take_while(|&&c| c == '#').count();
                    if chars.get(at + 1 + hashes) == Some(&'"') {
                        line.push('r');
                        line.push('"');
                        state = Lexed::RawStr(hashes);
                        if let Some(all) = literals.as_deref_mut() {
                            all.push((lines.len() + 1, String::new()));
                        }
                        at += 2 + hashes;
                        continue;
                    }
                    line.push(c);
                }
                '\'' if next == Some('\\') => {
                    // An escaped character: the closing quote comes after the
                    // escaped character, which may itself be a quote.
                    let from = (at + 3).min(chars.len());
                    match chars[from..].iter().position(|&c| c == '\'' || c == '\n') {
                        Some(offset) if chars[from + offset] == '\'' => {
                            line.push_str("' '");
                            at = from + offset + 1;
                            continue;
                        }
                        _ => line.push(c),
                    }
                }
                '\'' if next.is_some_and(|c| c != '\n') && chars.get(at + 2) == Some(&'\'') => {
                    line.push_str("' '");
                    at += 3;
                    continue;
                }
                _ => line.push(c),
            },
            Lexed::LineComment => line.push(' '),
            Lexed::BlockComment(depth) => {
                if c == '*' && next == Some('/') {
                    state = if depth == 1 {
                        Lexed::Code
                    } else {
                        Lexed::BlockComment(depth - 1)
                    };
                    line.push_str("  ");
                    at += 1;
                } else if c == '/' && next == Some('*') {
                    state = Lexed::BlockComment(depth + 1);
                    line.push_str("  ");
                    at += 1;
                } else {
                    line.push(' ');
                }
            }
            Lexed::Str => {
                if c == '\\' {
                    line.push(' ');
                    extend_literal(&mut literals, c);
                    if let Some(escaped) = next.filter(|&c| c != '\n') {
                        line.push(' ');
                        extend_literal(&mut literals, escaped);
                        at += 1;
                    }
                } else if c == '"' {
                    state = Lexed::Code;
                    line.push('"');
                } else {
                    line.push(' ');
                    extend_literal(&mut literals, c);
                }
            }
            Lexed::RawStr(hashes) => {
                let closes = c == '"'
                    && chars.len() > at + hashes
                    && chars[at + 1..=at + hashes].iter().all(|&c| c == '#');
                if closes {
                    line.push('"');
                    state = Lexed::Code;
                    at += hashes;
                } else {
                    line.push(' ');
                    extend_literal(&mut literals, c);
                }
            }
        }
        at += 1;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Each literal form the lexer reads, with the line it opens on and its
/// contents. A quote in a comment or a character literal opens none, and a
/// lifetime is not a character literal.
#[test]
fn each_string_literal_is_read_with_its_line_and_contents() {
    let text = r##"let a = "one"; // "not a literal"
let b = '"'; let c = '\''; fn f<'a>(x: &'a str) {}
let d = r#"raw "quoted" text"#;
let e = b"bytes\"";
let g = "first
second";
/* "block" */ let h = br"x";
"##;
    assert_eq!(
        string_literals(text),
        [
            (1, "one".to_owned()),
            (3, r#"raw "quoted" text"#.to_owned()),
            (4, r#"bytes\""#.to_owned()),
            (5, "first\nsecond".to_owned()),
            (7, "x".to_owned()),
        ]
    );
}

/// Each case marks the lines it expects blanked with a trailing `//~`.
const CASES: &[(&str, &str)] = &[
    (
        "a test-only re-export above the draw code",
        "use a::b;\n#[cfg(test)] //~\npub(crate) use menu::labels; //~\n\nfn draw() {\n    paint();\n}\n",
    ),
    (
        "a test-only function whose brackets sit in literals and comments",
        r##"#[cfg(test)] //~
fn helper() { //~
    let s = "}"; //~
    let c = '{'; //~
    let q = '\''; //~
    let b = '\\'; //~
    // } //~
    /* } */ //~
} //~
fn kept() {}
"##,
    ),
    (
        "a test-only statement inside a paint",
        "fn paint() {\n    #[cfg(test)] //~\n    record(); //~\n    draw();\n}\n",
    ),
    (
        "a test-only statement over several lines",
        "fn wraps() {\n    #[cfg(test)] //~\n    let row = row.child( //~\n        canvas(|b| record(b)), //~\n    ); //~\n    row\n}\n",
    ),
    (
        "a test-only if block",
        "fn paint() {\n    #[cfg(test)] //~\n    if fade.fades() { //~\n        painted.push(fade); //~\n    } //~\n    draw();\n}\n",
    ),
    (
        "a test-only macro block",
        "#[cfg(test)] //~\nthread_local! { //~\n    static R: u8 = 0; //~\n} //~\nconst H: f32 = 1.0;\n",
    ),
    (
        "a test module mounted from another file",
        "#[cfg(test)] //~\n#[path = \"x_tests.rs\"] //~\nmod tests; //~\nfn kept() {}\n",
    ),
    (
        "a test-only field",
        "struct S {\n    #[cfg(test)] //~\n    seen: u8, //~\n    kept: u8,\n}\n",
    ),
    (
        "a test-only arm",
        "match x {\n    #[cfg(test)] //~\n    A => 1, //~\n    B => 2,\n}\n",
    ),
    (
        "a test-only function with generics on their own lines",
        "#[cfg(test)] //~\nfn f< //~\n    A, //~\n>() {} //~\nfn kept() {}\n",
    ),
    (
        "a test-only impl with a where clause",
        "#[cfg(test)] //~\nimpl<T> X for Y<T> //~\nwhere //~\n    T: Clone, //~\n{ //~\n} //~\nfn kept() {}\n",
    ),
    (
        "test functions",
        "#[test] //~\nfn a() {} //~\n#[gpui::test] //~\nfn b() {} //~\n#[gpui::test(iterations = 2)] //~\nfn c() {} //~\nfn kept() {}\n",
    ),
    (
        "a test-only constant whose raw string holds a bracket and a quote",
        "#[cfg(test)] //~\nconst S: &str = r#\"}\" {\"#; //~\nfn kept() {}\n",
    ),
    (
        "a test-only function with lifetimes",
        "#[cfg(test)] //~\nfn f<'a>(x: &'a str) -> &'a str { //~\n    x //~\n} //~\nfn kept() {}\n",
    ),
    (
        "a test module at the end of the file",
        "fn kept() {}\n#[cfg(test)] //~\nmod tests { //~\n    #[test] //~\n    fn t() { //~\n        assert!(true); //~\n    } //~\n} //~\n",
    ),
    (
        "a test-only function whose closing bracket hides in a nested block comment",
        "#[cfg(test)] //~\nfn helper() { //~\n    /* /* */ //~\n    } //~\n    */ //~\n    x(); //~\n} //~\nfn kept() {}\n",
    ),
    (
        "production code under another cfg, after a comment, after a block comment, and after a string",
        "#[cfg(not(test))]\nfn a() {}\n// #[cfg(test)]\nfn b() {}\n/*\n#[cfg(test)]\n*/\nfn d() {}\nconst S: &str = \"\n#[cfg(test)]\n\";\nfn c() {}\n",
    ),
];

#[test]
fn each_test_only_form_is_blanked_and_every_other_line_kept() {
    for (what, source) in CASES {
        let mut expected = String::new();
        for line in source.lines() {
            if !line.ends_with("//~") {
                expected.push_str(line);
            }
            expected.push('\n');
        }
        assert_eq!(without_test_items(source), expected, "{what}");
    }
}

/// Fewest crate sources with a production line after a test-only item. A file
/// counted here is one a cut at the first test attribute reads only in part.
const RESUMED_FLOOR: usize = 150;

/// Every source in the crate keeps its line count and balanced brackets once
/// its test-only items are blanked, and keeps no test attribute. A scan that
/// stops inside an item or runs past its end breaks one of the three in the
/// file where it happens.
#[test]
fn every_crate_source_keeps_its_lines_and_balances_without_its_test_items() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    super::collect_rs(&src, &mut files);
    let mut faults = Vec::new();
    let mut resumed = 0;
    for file in &files {
        let name = file.strip_prefix(&src).unwrap_or(file).display();
        let text = std::fs::read_to_string(file)
            .unwrap_or_else(|err| panic!("{name} is listed and must read: {err}"));
        let stripped = without_test_items(&text);
        if stripped.lines().count() != text.lines().count() {
            faults.push(format!("{name}: the line count changed"));
            continue;
        }
        let mut depth = 0i64;
        let mut below = false;
        for (index, code) in code_lines(&stripped).iter().enumerate() {
            let start = code.trim_start();
            if TEST_ATTRIBUTES
                .iter()
                .any(|attribute| start.starts_with(attribute))
            {
                faults.push(format!("{name}:{}: keeps `{}`", index + 1, code.trim()));
            }
            for c in code.chars() {
                match c {
                    '{' | '(' | '[' => depth += 1,
                    '}' | ')' | ']' => depth -= 1,
                    _ => {}
                }
                below |= depth < 0;
            }
        }
        if depth != 0 || below {
            faults.push(format!("{name}: the brackets left do not balance"));
        }
        let mut blanked = false;
        for (source, kept) in text.lines().zip(stripped.lines()) {
            if kept.is_empty() && !source.is_empty() {
                blanked = true;
            } else if blanked && !kept.trim().is_empty() {
                resumed += 1;
                break;
            }
        }
    }
    assert!(
        faults.is_empty(),
        "{} faults:\n  {}",
        faults.len(),
        faults.join("\n  ")
    );
    assert!(
        resumed >= RESUMED_FLOOR,
        "{resumed} of {} sources keep a production line after a test-only item, fewer than \
         the {RESUMED_FLOOR} this check was written against: it is now checking almost nothing",
        files.len()
    );
}
