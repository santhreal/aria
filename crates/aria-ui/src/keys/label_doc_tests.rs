//! A control the documentation names by its label and its id, as
//! `**Label** (`id`)` or `**Menu > Label** (`id`)`, is named by the label the
//! product draws for that id.
//!
//! WHY: the filter reference named the Inspector's `filter-intensity` and
//! `lut-intensity` rows **Filter intensity** and **LUT intensity** while the
//! Inspector draws **Filter amount** and **LUT amount**, so a reader searched
//! for a row that is not there. This reads every page under `docs/` and holds
//! each such pair whose id is a palette command, a clip property row, or a tool
//! to that entry's label.
//!
//! Not caught: a pair whose id belongs to no registry read here, such as a
//! menu row or an Inspector control without a catalog row; a label named
//! without its id; and text inside fenced code blocks.

use std::collections::BTreeMap;
use std::path::Path;

use super::docs_markdown_tests::collect_md_files;
use super::reference_doc_tests::shipped_palette_commands;

/// Every id a page may pair with a label, with the label the product draws.
fn shipped_labels() -> BTreeMap<String, String> {
    let mut labels: BTreeMap<String, String> = shipped_palette_commands()
        .into_iter()
        .map(|(id, label)| (id.to_owned(), label.to_owned()))
        .collect();
    labels.extend(
        aria_models::clip_param_catalog()
            .iter()
            .map(|row| (row.id.to_string(), row.label.to_string())),
    );
    labels.extend(
        crate::tool_catalog::tool_specs()
            .iter()
            .map(|tool| (tool.id.to_owned(), tool.label.to_owned())),
    );
    labels
}

/// Each `**label** (`id`` pair on `line`: the bold text's last menu step and
/// the first code span after it.
fn label_pairs(line: &str) -> Vec<(&str, &str)> {
    let mut pairs = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find("**") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("**") else {
            break;
        };
        let bold = &after[..close];
        let tail = &after[close + 2..];
        if let Some(id) = tail
            .strip_prefix(" (`")
            .and_then(|code| code.split_once('`'))
            .map(|(id, _)| id)
        {
            let label = bold.rsplit(" > ").next().unwrap_or(bold).trim();
            pairs.push((label, id));
        }
        rest = tail;
    }
    pairs
}

#[test]
fn every_label_paired_with_an_id_is_the_label_drawn_for_it() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut pages = Vec::new();
    collect_md_files(&docs, &mut pages);
    let shipped = shipped_labels();
    let mut checked = 0_usize;
    let mut wrong = Vec::new();
    for page in &pages {
        let text = std::fs::read_to_string(page)
            .unwrap_or_else(|err| panic!("read {}: {err}", page.display()));
        let mut fenced = false;
        for (index, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            for (label, id) in label_pairs(line) {
                let Some(drawn) = shipped.get(id) else {
                    continue;
                };
                checked += 1;
                if label != drawn {
                    let shown = page.strip_prefix(&docs).unwrap_or(page);
                    wrong.push(format!(
                        "docs/{}:{}: **{label}** (`{id}`) is drawn as {drawn}",
                        shown.display(),
                        index + 1
                    ));
                }
            }
        }
    }
    assert!(
        checked >= 30,
        "checked {checked} label and id pairs under docs/"
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The pair reader at its edges: a menu path takes its last step, a pair
/// needs the code span right after the bold text, and a line holds several.
#[test]
fn the_pair_reader_takes_the_last_step_and_the_first_code_span() {
    assert_eq!(
        label_pairs("drag **Filter amount** (`filter-intensity`, `0.0`)"),
        vec![("Filter amount", "filter-intensity")]
    );
    assert_eq!(
        label_pairs("**Window > Workspaces > Save Workspace** (`win-workspace-save`)"),
        vec![("Save Workspace", "win-workspace-save")]
    );
    assert_eq!(
        label_pairs("**Slip** (`tool-slip`) or **Slide** (`tool-slide`), not **Razor**"),
        vec![("Slip", "tool-slip"), ("Slide", "tool-slide")]
    );
    assert!(label_pairs("**Bold** then `code`").is_empty());
    assert!(label_pairs("**Unclosed (`id`)").is_empty());
}
