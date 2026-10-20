//! Tokens of the expression language.

/// Where a token starts, counted from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Pos {
    pub(super) line: u32,
    pub(super) column: u32,
}

impl std::fmt::Display for Pos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}, column {}", self.line, self.column)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Punct(&'static str),
    End,
}

#[derive(Debug, Clone)]
pub(super) struct Token {
    pub(super) tok: Tok,
    pub(super) pos: Pos,
}

/// Longest first, so `===` is not read as `==` and `=`.
const PUNCTS: [&str; 34] = [
    "===", "!==", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/=", "++", "--", "(", ")",
    "[", "]", "{", "}", ",", ";", ".", "?", ":", "+", "-", "*", "/", "%", "!", "=", "<", ">",
];

pub(super) fn lex(source: &str) -> Result<Vec<Token>, String> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;
    let mut line = 1u32;
    let mut line_start = 0usize;
    // The column is counted forward from the last place it was known, so a
    // long line costs one pass.
    let (mut counted_to, mut column) = (0usize, 1u32);
    while at < bytes.len() {
        if counted_to < line_start {
            (counted_to, column) = (line_start, 1);
        }
        column += source[counted_to..at].chars().count() as u32;
        counted_to = at;
        let pos = Pos { line, column };
        let rest = &source[at..];
        let byte = bytes[at];
        if byte == b'\n' {
            at += 1;
            line += 1;
            line_start = at;
            continue;
        }
        if byte.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if rest.starts_with("//") {
            at += rest.find('\n').unwrap_or(rest.len());
            continue;
        }
        if let Some(body) = rest.strip_prefix("/*") {
            let Some(end) = body.find("*/") else {
                return Err(format!("{pos}: a comment is not closed with */"));
            };
            let comment = &rest[..end + 4];
            for (offset, _) in comment.match_indices('\n') {
                line += 1;
                line_start = at + offset + 1;
            }
            at += comment.len();
            continue;
        }
        if byte.is_ascii_digit()
            || (byte == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit))
        {
            let len = number_len(rest);
            let text = &rest[..len];
            let value = text
                .parse::<f64>()
                .map_err(|_| format!("{pos}: {text} is not a number"))?;
            tokens.push(Token {
                tok: Tok::Num(value),
                pos,
            });
            at += len;
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            let (text, len) = string(rest, pos)?;
            tokens.push(Token {
                tok: Tok::Str(text),
                pos,
            });
            at += len;
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' {
            let len = rest
                .bytes()
                .position(|b| !(b.is_ascii_alphanumeric() || b == b'_' || b == b'$'))
                .unwrap_or(rest.len());
            tokens.push(Token {
                tok: Tok::Ident(rest[..len].to_string()),
                pos,
            });
            at += len;
            continue;
        }
        let Some(punct) = PUNCTS.iter().find(|punct| rest.starts_with(**punct)) else {
            let shown = rest.chars().next().unwrap_or(' ');
            return Err(format!("{pos}: {shown:?} is not part of the language"));
        };
        tokens.push(Token {
            tok: Tok::Punct(punct),
            pos,
        });
        at += punct.len();
    }
    if counted_to < line_start {
        (counted_to, column) = (line_start, 1);
    }
    column += source[counted_to..].chars().count() as u32;
    tokens.push(Token {
        tok: Tok::End,
        pos: Pos { line, column },
    });
    Ok(tokens)
}

/// Bytes of the number at the start of `text`: digits, a fraction, and an
/// exponent.
fn number_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .position(|b| !b.is_ascii_digit())
            .map_or(bytes.len(), |len| from + len)
    };
    let mut end = digits(0);
    if bytes.get(end) == Some(&b'.') {
        end = digits(end + 1);
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut exponent = end + 1;
        if matches!(bytes.get(exponent), Some(b'+' | b'-')) {
            exponent += 1;
        }
        if bytes.get(exponent).is_some_and(u8::is_ascii_digit) {
            end = digits(exponent);
        }
    }
    end
}

/// The string literal at the start of `text`, and its length in bytes.
fn string(text: &str, pos: Pos) -> Result<(String, usize), String> {
    let mut chars = text.char_indices();
    let Some((_, quote)) = chars.next() else {
        return Err(format!("{pos}: a string is empty"));
    };
    let mut out = String::new();
    while let Some((at, c)) = chars.next() {
        match c {
            '\n' => break,
            '\\' => {
                let Some((_, escaped)) = chars.next() else {
                    break;
                };
                out.push(match escaped {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    '0' => '\0',
                    other => other,
                });
            }
            c if c == quote => return Ok((out, at + c.len_utf8())),
            c => out.push(c),
        }
    }
    Err(format!("{pos}: a string is not closed with {quote}"))
}
