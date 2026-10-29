//! Every Rust source file in the workspace stays within [`MAX_LINES`] lines.

use std::path::Path;

use super::collect_rs;

/// The most lines a Rust source file holds.
const MAX_LINES: usize = 1000;

/// **No Rust source file in the workspace is longer than 1000 lines.**
///
/// A file past that holds more than one concern, and a reader scrolls past
/// the ones they did not open it for. The measured instances were
/// `components.rs` at 1096 lines, which held the record a test keeps of the
/// controls beside the controls and a slider row no surface drew, and
/// `zoom_bar_tests.rs` at 1047, which held the drawn bar's tests beside the
/// state's.
///
/// Derived from the sources under `crates`, `apps`, `examples` and `fuzz`, so
/// a new file is measured on the run that adds it. Not covered: files of
/// other languages, such as WGSL shaders and Markdown.
#[test]
fn no_source_file_is_longer_than_a_thousand_lines() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate sits two levels inside the workspace")
        .to_path_buf();
    let mut files = Vec::new();
    for root in ["crates", "apps", "examples", "fuzz"] {
        collect_rs(&workspace.join(root), &mut files);
    }
    files.sort();
    assert!(
        files.len() > 500,
        "found {} source files under {workspace:?}, which cannot be this workspace: \
         the rule just went vacuous",
        files.len()
    );

    let offenders: Vec<String> = files
        .iter()
        .filter_map(|file| {
            let text = std::fs::read_to_string(file)
                .unwrap_or_else(|err| panic!("read {}: {err}", file.display()));
            let lines = text.lines().count();
            (lines > MAX_LINES).then(|| {
                let rel = file.strip_prefix(&workspace).unwrap_or(file);
                format!("{}: {lines} lines", rel.display())
            })
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "split each file below into modules of one concern, each at most {MAX_LINES} lines:\n{}",
        offenders.join("\n")
    );
}
