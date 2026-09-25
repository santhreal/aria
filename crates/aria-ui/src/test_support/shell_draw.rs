//! Nothing reads the shell entity while it is being drawn.

use std::path::Path;

use super::collect_rs;

/// **Nothing reads the shell entity back off the context.**
///
/// A panel is drawn inside an update of the `Shell` entity, so
/// `cx.entity().read(cx)` asks gpui to lease an entity it has already
/// leased: `entity_map.rs` calls `double_lease_panic` and the frame dies.
/// Seven render paths did it — the timeline ruler's marker row, the
/// inspector's animation, keyframe, speed, template, info and parameter
/// blocks — which means the editor route panicked on the first frame it
/// drew any of them. The `Entity<AppState>` is passed down instead; the tab
/// bodies already carried it.
///
/// Derived from the sources and fails by default: the next file that reaches
/// for the shell this way turns this red on the run that adds it, whatever
/// it is called and wherever it lives.
#[test]
fn nothing_reads_the_shell_entity_while_it_is_being_drawn() {
    let crate_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate sits two levels inside the workspace")
        .to_path_buf();
    let mut files = Vec::new();
    collect_rs(&crate_src, &mut files);
    assert!(
        files.len() > 200,
        "found {} files under {crate_src:?}, which cannot be this crate: the rule just \
         went vacuous",
        files.len()
    );

    // Whitespace is squashed first, so the same call split over four lines
    // by a formatter is the same call to this rule. Detection and reporting
    // are the same pass on purpose: a matcher that finds the call and a
    // reporter that cannot name a line is a rule that passes on the defect,
    // which is exactly what the first draft of this test did.
    const LEASED: &str = "cx.entity().read(";
    let hits = |text: &str| -> Vec<usize> {
        let mut squashed = String::new();
        // The line each byte of `squashed` came from.
        let mut lines = Vec::new();
        for (number, line) in text.lines().enumerate() {
            for ch in line.chars().filter(|c| !c.is_whitespace()) {
                squashed.push(ch);
                lines.resize(squashed.len(), number + 1);
            }
        }
        let mut found = Vec::new();
        let mut from = 0usize;
        while let Some(at) = squashed[from..].find(LEASED) {
            found.push(lines[from + at]);
            from += at + LEASED.len();
        }
        found
    };
    assert_eq!(
        hits("fn f() {\n    let s = cx\n        .entity()\n        .read(cx);\n}\n"),
        vec![2],
        "the matcher has to see the wrapped form and name the line it starts on"
    );

    let mut offenders = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&workspace)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        // This file spells the call it refuses, so it would match itself.
        if rel.starts_with("crates/aria-ui/src/test_support") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in hits(&text) {
            offenders.push(format!("{rel}:{line}"));
        }
    }
    assert!(
        offenders.is_empty(),
        "reading the shell entity during its own render panics in gpui; take the \
         state as a parameter — `state: &gpui::Entity<crate::app::AppState>` — the way \
         the inspector tab bodies do:\n  {}",
        offenders.join("\n  ")
    );
}
