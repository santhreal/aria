//! Every call that draws a shared control, and the documented id each call
//! states.
//!
//! `keys::controls_doc_tests` reads this to check `docs/reference/controls.md`
//! against the source. The shared controls are the public functions of
//! `components.rs` that take a control id first: `components::button`,
//! `ghost_button`, `disclosure_button`, `danger_button`, `chip`, `list_chip`,
//! `text_box`, `field_frame` and the icon button. A call states its control
//! id in one of four forms, read from the call's own text:
//!
//! - a string literal, for a control drawn once;
//! - a tuple whose first element is a literal, `("speed-preset", id)`, for a
//!   control drawn once per row: the literal is the documented id and the
//!   second element tells the rows apart;
//! - `MotionKey::new("kind", ..)`, whose kind is the documented id;
//! - `if cond { "a" } else { "b" }`, both literals.
//!
//! A call in none of these forms is resolved by a record: a [`Forwarder`], a
//! function that passes one of its arguments to the widget, whose callers are
//! read at that argument instead; or a [`Table`], a call whose id is a field of
//! a static table, whose ids are enumerated at run time. Any other call fails
//! the census, so a new control whose id the census cannot read turns the suite
//! red until the call states its id or a record covers it.

use std::collections::BTreeSet;
use std::path::Path;

use super::collect_rs;
use super::sources::without_test_items;

/// The shared widgets. Spelled with `concat!` so that no needle appears whole
/// in this file: the tooltip census reads every non-test file for icon button
/// calls without stripping comments or strings.
const WIDGETS: [&str; 9] = [
    concat!("components::", "button("),
    concat!("ghost_", "button("),
    concat!("disclosure_", "button("),
    concat!("components::", "danger_button("),
    concat!("components::", "chip("),
    concat!("list_", "chip("),
    concat!("components::", "text_box("),
    concat!("components::", "field_frame("),
    concat!("icon_", "button("),
];

/// A function or closure that passes one of its arguments to a shared widget
/// as the control id. Its callers state the id at that argument.
struct Forwarder {
    /// The function or closure binding. Unique in the crate, so every call of
    /// it is a call of this one.
    name: &'static str,
    /// The zero-based argument that holds the id.
    arg: usize,
}

const FORWARDERS: &[Forwarder] = &[
    Forwarder {
        name: "lit_icon",
        arg: 0,
    },
    Forwarder {
        name: "tool_btn",
        arg: 0,
    },
    Forwarder {
        name: "display_btn",
        arg: 0,
    },
    Forwarder {
        name: "transport_btn",
        arg: 1,
    },
    Forwarder {
        name: "jump_btn",
        arg: 0,
    },
    Forwarder {
        name: "step_btn",
        arg: 0,
    },
    Forwarder {
        name: "align_button",
        arg: 1,
    },
    Forwarder {
        name: "pick_button",
        arg: 0,
    },
    Forwarder {
        name: "pick_chip",
        arg: 1,
    },
    Forwarder {
        name: "marker_action",
        arg: 1,
    },
    Forwarder {
        name: "on_off_row",
        arg: 1,
    },
    Forwarder {
        name: "choice_row",
        arg: 1,
    },
    Forwarder {
        name: "stepper",
        arg: 2,
    },
    Forwarder {
        name: "graph_mode_chip",
        arg: 0,
    },
    Forwarder {
        name: "graph_tool",
        arg: 0,
    },
    Forwarder {
        name: "card_action",
        arg: 0,
    },
];

/// A widget call whose id is a field of a static table.
struct Table {
    /// The calling file, relative to `src/`.
    file: &'static str,
    /// The id argument as written at the call.
    expr: &'static str,
    /// Every id the table holds.
    ids: fn() -> Vec<&'static str>,
}

fn tool_ids() -> Vec<&'static str> {
    crate::tool_catalog::tool_specs()
        .iter()
        .map(|spec| spec.id)
        .collect()
}

fn media_support_ids() -> Vec<&'static str> {
    crate::panels::media_support::action_ids().collect()
}

fn bin_action_ids() -> Vec<&'static str> {
    crate::surfaces::bin_strip::BIN_ACTIONS
        .iter()
        .map(|action| action.id)
        .collect()
}

const TABLES: &[Table] = &[
    Table {
        file: "chrome/tool_strip.rs",
        expr: "spec.id",
        ids: tool_ids,
    },
    Table {
        file: "panels/media_support.rs",
        expr: "action.id",
        ids: media_support_ids,
    },
    Table {
        file: "panels/sidebar_bins.rs",
        expr: "action.id",
        ids: bin_action_ids,
    },
];

/// Replaces line and block comments with spaces while preserving string
/// literals and newlines, so a scan matches no commented-out call and every
/// byte offset keeps its line.
pub(crate) fn strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(b' ');
                i += 1;
            }
        } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            out.push(b' ');
            out.push(b' ');
            i += 2;
            let mut depth = 1usize;
            while i < bytes.len() && depth > 0 {
                if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    depth += 1;
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                } else if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    depth -= 1;
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                } else {
                    out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
                    i += 1;
                }
            }
        } else if bytes[i] == b'"' {
            out.push(bytes[i]);
            i += 1;
            while i < bytes.len() {
                out.push(bytes[i]);
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 1;
                    out.push(bytes[i]);
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).expect("valid utf-8 after stripping comments")
}

/// One non-test source file of the crate, comments stripped.
struct Source {
    rel: String,
    text: String,
}

fn sources() -> Vec<Source> {
    let crate_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&crate_src, &mut files);
    files.sort();
    files
        .into_iter()
        .filter_map(|file| {
            let rel = file
                .strip_prefix(&crate_src)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            if rel == "components.rs"
                || rel.starts_with("test_support")
                || rel.ends_with("_tests.rs")
                || rel.ends_with("/tests.rs")
                || rel.contains("/tests/")
            {
                return None;
            }
            let raw = std::fs::read_to_string(&file)
                .unwrap_or_else(|err| panic!("failed to read {}: {err}", file.display()));
            Some(Source {
                rel,
                text: strip_comments(&without_test_items(&raw)),
            })
        })
        .collect()
}

/// The byte just past a string or char literal that opens at `at`, or `None`
/// when `at` opens neither. A lifetime such as `'static` is not a literal.
fn skip_literal(bytes: &[u8], at: usize) -> Option<usize> {
    match bytes[at] {
        b'"' => {
            let mut i = at + 1;
            while i < bytes.len() && bytes[i] != b'"' {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            Some(i + 1)
        }
        b'\'' if bytes.get(at + 2) == Some(&b'\'') => Some(at + 3),
        b'\'' if bytes.get(at + 1) == Some(&b'\\') => (at + 2..bytes.len().min(at + 12))
            .find(|&i| bytes[i] == b'\'')
            .map(|i| i + 1),
        _ => None,
    }
}

/// The top-level arguments of the call whose `(` is at `open`, trimmed.
fn call_args(text: &str, open: usize) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut start = open + 1;
    let mut i = open + 1;
    while i < bytes.len() {
        if let Some(next) = skip_literal(bytes, i) {
            i = next;
            continue;
        }
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth == 0 => {
                let last = text[start..i].trim();
                if !last.is_empty() {
                    args.push(last);
                }
                return args;
            }
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                args.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    panic!("unclosed call at byte {open}");
}

/// The whole of `text` as a usable id literal.
fn literal(text: &str) -> Option<&str> {
    let id = text.strip_prefix('"')?.strip_suffix('"')?;
    (!id.is_empty() && !id.contains(['"', '\\', '{', '$'])).then_some(id)
}

/// A usable id literal at the start of `text`, followed by a comma.
fn leading_literal(text: &str) -> Option<&str> {
    let rest = text.trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    let id = literal(&text.trim_start()[..end + 2])?;
    rest[end + 1..].trim_start().starts_with(',').then_some(id)
}

/// The documented ids an id argument states, in the forms the module
/// documentation lists.
fn stated_ids(arg: &str) -> Option<Vec<&str>> {
    if let Some(id) = literal(arg) {
        return Some(vec![id]);
    }
    if let Some(inner) = arg.strip_prefix('(') {
        return leading_literal(inner).map(|id| vec![id]);
    }
    for prefix in [
        "MotionKey::new(",
        "motion::MotionKey::new(",
        "crate::motion::MotionKey::new(",
    ] {
        if let Some(inner) = arg.strip_prefix(prefix) {
            return leading_literal(inner).map(|id| vec![id]);
        }
    }
    let rest = arg.strip_prefix("if ")?;
    let open = rest.find('{')?;
    let close = open + rest[open..].find('}')?;
    let first = literal(rest[open + 1..close].trim())?;
    let other = rest[close + 1..]
        .trim_start()
        .strip_prefix("else")?
        .trim_start()
        .strip_prefix('{')?
        .strip_suffix('}')?;
    Some(vec![first, literal(other.trim())?])
}

/// Whether the match at `at` is a whole call of `needle` rather than the tail
/// of a longer name, a method call, or the definition.
fn is_call(text: &str, at: usize, needle: &str) -> bool {
    let before = &text.as_bytes()[..at];
    if needle.contains("::") {
        return true;
    }
    let joined = before
        .last()
        .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.');
    !joined && !before.ends_with(b"fn ")
}

/// The byte range of the body of the function or closure defined at `at`.
fn body(text: &str, at: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let open = at + text[at..].find('{').expect("a definition has a body");
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        if let Some(next) = skip_literal(bytes, i) {
            i = next;
            continue;
        }
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return (open, i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    panic!("unclosed body at byte {at}");
}

/// Every definition of `name` as a function or a closure binding.
fn definitions(text: &str, name: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for (prefix, openers) in [
        ("fn ", &["(", "<"][..]),
        ("let ", &[" = |", " = move |"][..]),
    ] {
        let head = format!("{prefix}{name}");
        let mut cursor = 0;
        while let Some(pos) = text[cursor..].find(&head) {
            let at = cursor + pos;
            cursor = at + head.len();
            let starts_word = text.as_bytes()[..at]
                .last()
                .is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'_');
            if starts_word && openers.iter().any(|o| text[cursor..].starts_with(o)) {
                found.push(at);
            }
        }
    }
    found
}

/// Every documented control id the shared-widget calls state.
///
/// Panics listing each call whose id it cannot read, and each record that no
/// longer matches the source: a forwarder defined other than once, one that
/// draws no shared control or has no caller, and a table no call reads.
pub(crate) fn control_ids() -> BTreeSet<String> {
    let sources = sources();
    let mut stale = Vec::new();

    // Each forwarder's body, by file.
    let mut bodies: Vec<(&str, (usize, usize), &'static str)> = Vec::new();
    for forwarder in FORWARDERS {
        let found: Vec<_> = sources
            .iter()
            .flat_map(|source| {
                definitions(&source.text, forwarder.name)
                    .into_iter()
                    .map(move |at| (source, at))
            })
            .collect();
        match found.as_slice() {
            [(source, at)] => {
                bodies.push((source.rel.as_str(), body(&source.text, *at), forwarder.name))
            }
            _ => stale.push(format!(
                "forwarder `{}` is defined {} times, not once; rename it or drop its record",
                forwarder.name,
                found.len()
            )),
        }
    }

    // Each needle, the argument that holds the id, and the forwarder it calls.
    let mut needles: Vec<(String, usize, Option<&'static str>)> =
        WIDGETS.iter().map(|w| (w.to_string(), 0, None)).collect();
    needles.extend(
        FORWARDERS
            .iter()
            .map(|f| (format!("{}(", f.name), f.arg, Some(f.name))),
    );

    let mut ids = BTreeSet::new();
    let mut unresolved = Vec::new();
    let mut forwarding: BTreeSet<&str> = BTreeSet::new();
    let mut called: BTreeSet<&str> = BTreeSet::new();
    let mut tables_read: BTreeSet<(&str, &str)> = BTreeSet::new();
    for source in &sources {
        let text = source.text.as_str();
        for (needle, index, forwarder) in &needles {
            let mut cursor = 0;
            while let Some(pos) = text[cursor..].find(needle.as_str()) {
                let at = cursor + pos;
                cursor = at + needle.len();
                if !is_call(text, at, needle) {
                    continue;
                }
                if let Some(name) = forwarder {
                    called.insert(*name);
                }
                let args = call_args(text, cursor - 1);
                let arg = args.get(*index).copied().unwrap_or("");
                if let Some(stated) = stated_ids(arg) {
                    ids.extend(stated.into_iter().map(str::to_owned));
                    continue;
                }
                if let Some((_, _, name)) = bodies.iter().find(|(rel, (open, close), _)| {
                    *rel == source.rel && (*open..*close).contains(&at)
                }) {
                    forwarding.insert(*name);
                    continue;
                }
                if let Some(table) = TABLES
                    .iter()
                    .find(|t| t.file == source.rel && t.expr == arg)
                {
                    tables_read.insert((table.file, table.expr));
                    ids.extend((table.ids)().into_iter().map(str::to_owned));
                    continue;
                }
                let line = text[..at].matches('\n').count() + 1;
                unresolved.push(format!("{}:{line}: {needle}{arg}, ..)", source.rel));
            }
        }
    }

    for forwarder in FORWARDERS {
        if !forwarding.contains(forwarder.name) {
            stale.push(format!(
                "forwarder `{}` passes no argument to a shared control; drop its record",
                forwarder.name
            ));
        }
        if !called.contains(forwarder.name) {
            stale.push(format!(
                "forwarder `{}` has no caller; drop its record",
                forwarder.name
            ));
        }
    }
    for table in TABLES {
        if !tables_read.contains(&(table.file, table.expr)) {
            stale.push(format!(
                "no call in {} reads `{}`; drop its table record",
                table.file, table.expr
            ));
        }
    }

    assert!(
        unresolved.is_empty(),
        "{} shared-control calls state no documented id. Write the id at the call as a \
         literal, as (\"doc-id\", row) or as MotionKey::new(\"doc-id\", ..), or record a \
         forwarder or table in test_support/control_sites.rs, then document the id in \
         docs/reference/controls.md:\n{}",
        unresolved.len(),
        unresolved.join("\n")
    );
    assert!(
        stale.is_empty(),
        "test_support/control_sites.rs records drifted from the source:\n{}",
        stale.join("\n")
    );
    ids
}

mod tests {
    use super::*;

    /// WHY: the census reads ids from the call's text, so a form it misreads
    /// either hides a control or invents one. Each form the module
    /// documentation lists, and the near misses beside it, are pinned here.
    #[test]
    fn stated_ids_reads_each_form_and_rejects_the_rest() {
        let cases: [(&str, Option<Vec<&str>>); 12] = [
            ("\"bin-new\"", Some(vec!["bin-new"])),
            ("(\"speed-preset\", id)", Some(vec!["speed-preset"])),
            (
                "(\n    \"insert-add\",\n    base.with(\"add\"),\n)",
                Some(vec!["insert-add"]),
            ),
            (
                "MotionKey::new(\"align-edge\", edge.label()).with(SURFACE)",
                Some(vec!["align-edge"]),
            ),
            (
                "crate::motion::MotionKey::new(\"analysis-run\", id)",
                Some(vec!["analysis-run"]),
            ),
            (
                "if playing { \"pause\" } else { \"play\" }",
                Some(vec!["pause", "play"]),
            ),
            ("id", None),
            ("action.id", None),
            ("key.with(\"bypass\")", None),
            ("MotionKey::new(label, &id)", None),
            ("(\"\", id)", None),
            ("\"tab-{index}\"", None),
        ];
        for (arg, want) in cases {
            assert_eq!(stated_ids(arg), want, "arg {arg:?}");
        }
    }

    /// WHY: a shared control that is not a census needle draws an id the
    /// census never reads, so the documented row for it reads as a phantom
    /// and a new id at its calls goes undocumented. Every public function of
    /// `components.rs` that takes a control id or an icon id first is a
    /// shared control, and each needle names one of them. A shared control
    /// defined outside `components.rs` is not read here.
    #[test]
    fn every_shared_control_is_a_census_needle() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/components.rs");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let mut controls: Vec<&str> = text
            .match_indices("pub fn ")
            .filter_map(|(at, head)| {
                let rest = &text[at + head.len()..];
                let open = rest.find('(')?;
                let first = rest[open + 1..].trim_start();
                let takes_id = first.starts_with("id: impl Into<motion::ControlId>")
                    || first.starts_with("id: impl IconId");
                takes_id.then(|| rest[..open].split('<').next().unwrap_or_default())
            })
            .collect();
        let mut needles: Vec<&str> = WIDGETS
            .iter()
            .map(|w| w.trim_start_matches("components::").trim_end_matches('('))
            .collect();
        controls.sort_unstable();
        needles.sort_unstable();
        assert_eq!(
            controls, needles,
            "the shared controls in components.rs and the census needles in WIDGETS differ"
        );
    }

    #[test]
    fn call_args_splits_at_top_level_commas_only() {
        let text = "f((\"a\", k.with(1)), \"x,y\", ',', g(h, i))";
        let open = text.find('(').unwrap();
        assert_eq!(
            call_args(text, open),
            vec!["(\"a\", k.with(1))", "\"x,y\"", "','", "g(h, i)"]
        );
    }

    #[test]
    fn a_definition_or_a_longer_name_is_not_a_call() {
        let text = "fn draw_it(x) {} a.draw_it(y); redraw_it(z); draw_it(w)";
        let needle = "draw_it(";
        let calls: Vec<usize> = text
            .match_indices(needle)
            .map(|(at, _)| at)
            .filter(|&at| is_call(text, at, needle))
            .collect();
        assert_eq!(calls, vec![text.rfind(needle).unwrap()]);
    }
}
