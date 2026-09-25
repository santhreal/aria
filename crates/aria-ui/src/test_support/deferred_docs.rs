//! Every deferred item says what would end the deferral.

use std::path::Path;

/// **A parked item says what would unpark it.**
///
/// `docs/deferred.md` held seven items with a *Wanted* and a *Today* and no
/// third line, so each one was a paragraph about a feature nobody could pick
/// up: two readers would disagree about what finishing it meant, and three
/// of the seven had in fact shipped while the document still parked them.
/// Each entry now carries either the observable result that closes it or the
/// reason it will not be done.
///
/// Derived from the document: a section added without one of the two lines
/// turns this red on the run that adds it, and a document with too few
/// sections or no resolutions cannot make it vacuously green.
#[test]
fn deferred_items_have_acceptance() {
    let doc = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate sits two levels inside the workspace")
        .join("docs/deferred.md");
    let text = std::fs::read_to_string(&doc).expect("docs/deferred.md is part of the tree");

    // A `##` heading groups; a `###` heading is one parked item. The
    // resolved list is prose under its own group, not a parked item.
    let mut group = String::new();
    let mut item: Option<(String, bool)> = None;
    let mut items = 0usize;
    let mut resolutions = 0usize;
    let mut offenders = Vec::new();
    let close = |item: Option<(String, bool)>, offenders: &mut Vec<String>| {
        if let Some((name, answered)) = item {
            if !answered {
                offenders.push(name);
            }
        }
    };
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("## ") {
            close(item.take(), &mut offenders);
            group = name.trim().to_string();
            continue;
        }
        if let Some(name) = line.strip_prefix("### ") {
            close(item.take(), &mut offenders);
            items += 1;
            item = Some((format!("{group} / {}", name.trim()), false));
            continue;
        }
        if group == "Resolved" && line.starts_with("- **") {
            resolutions += 1;
            continue;
        }
        let answered =
            line.starts_with("- **Acceptance:**") || line.starts_with("- **Out of scope:**");
        if answered {
            if let Some((_, seen)) = item.as_mut() {
                *seen = true;
            }
        }
    }
    close(item.take(), &mut offenders);

    assert!(
        items >= 4,
        "found {items} parked items in {doc:?}, which cannot be this document: the \
         rule just went vacuous"
    );
    assert!(
        resolutions >= 3,
        "the Resolved group lists {resolutions} entries; it held five, and losing it \
         is how a shipped feature gets parked again"
    );
    assert!(
        offenders.is_empty(),
        "a parked item says neither what would close it nor why it will not be \
         done — add a `- **Acceptance:**` line naming the observable result, or a \
         `- **Out of scope:**` line naming the reason:\n  {}",
        offenders.join("\n  ")
    );
}
