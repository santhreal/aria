//! Sections and tables of the markdown in `docs/`, read for the tests that
//! hold a document to the code it describes.

use std::path::Path;

/// The text of `docs/<relative>`.
pub(crate) fn read_doc(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// A table cell without its surrounding backticks and whitespace.
pub(crate) fn strip_ticks(cell: &str) -> &str {
    cell.trim().trim_matches('`').trim()
}

/// The body of the `## {title}` section, up to the next `## ` heading.
pub(crate) fn section_slice<'a>(doc: &'a str, title: &str) -> &'a str {
    let marker = format!("## {title}\n");
    let start = doc
        .find(&marker)
        .unwrap_or_else(|| panic!("missing section `## {title}`"));
    let rest = &doc[start + marker.len()..];
    match rest.find("\n## ") {
        Some(end) => &rest[..end],
        None => rest,
    }
}

/// The body under `heading`, written with its `#`s, up to the next heading of
/// its level or above.
pub(crate) fn heading_body<'a>(doc: &'a str, heading: &str) -> &'a str {
    let level = heading.bytes().take_while(|&b| b == b'#').count();
    let start = doc
        .find(heading)
        .unwrap_or_else(|| panic!("missing heading {heading:?}"));
    let body = &doc[start + heading.len()..];
    let body = body.strip_prefix('\n').unwrap_or(body);
    let mut offset = 0usize;
    for line in body.lines() {
        let hashes = line.bytes().take_while(|&b| b == b'#').count();
        if (1..=level).contains(&hashes) && line.as_bytes().get(hashes) == Some(&b' ') {
            return &body[..offset];
        }
        offset += line.len() + 1;
    }
    body
}

/// The cells of a table row, trimmed.
fn cells(row: &str) -> Vec<&str> {
    row.trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

/// The rows of every table in `section` that are neither a separator row nor
/// a header row whose first cell is `ID`, `Variant`, `Key`, `Shape` or
/// `Shortcut`.
pub(crate) fn parse_table_rows(section: &str) -> Vec<Vec<&str>> {
    let mut rows = Vec::new();
    for line in section.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells = cells(trimmed);
        if cells.is_empty() {
            continue;
        }
        if cells
            .iter()
            .all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' '))
        {
            continue;
        }
        if matches!(cells[0], "ID" | "Variant" | "Key" | "Shape" | "Shortcut") {
            continue;
        }
        rows.push(cells);
    }
    rows
}

/// The body rows of every table in `text`: each table's first row is its
/// header and its second the separator.
pub(crate) fn table_rows(text: &str) -> Vec<Vec<&str>> {
    let mut rows = Vec::new();
    let mut in_table = false;
    let mut past_sep = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            in_table = false;
            past_sep = false;
            continue;
        }
        if !in_table {
            in_table = true;
            continue;
        }
        if !past_sep {
            past_sep = true;
            continue;
        }
        rows.push(cells(trimmed));
    }
    rows
}
