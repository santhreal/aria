//! Source census for centralized production error writes.

use super::*;

/// Every production write to `last_error` must go through `AppState::report_error`.
///
/// The class this closes: arbitrary call sites formatting or assigning errors directly
/// to `last_error`, bypassing centralized formatting and error routing. The allowed
/// choke point (`AppState::report_error`) is derived dynamically from source, and any
/// direct `.last_error = Some(...)` write in production files outside this choke point
/// is rejected.
#[test]
fn no_production_direct_last_error_writes_outside_report_error() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);
    assert!(
        files.len() > 50,
        "found {} source files under {src:?}, which cannot be this crate",
        files.len()
    );

    // Derive the choke point: locate `fn report_error` definition in AppState methods
    let mut choke_point_found = false;
    let mut choke_file_rel = String::new();
    let mut choke_line_start = 0usize;
    let mut choke_line_end = 0usize;

    for file in &files {
        let rel = file
            .strip_prefix(&src)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if rel == "test_support/error_writes.rs" {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (idx, line) in text.lines().enumerate() {
            if line.contains("pub fn report_error(") || line.contains("pub(crate) fn report_error(")
            {
                choke_point_found = true;
                choke_file_rel = rel.clone();
                choke_line_start = idx + 1;
                choke_line_end = idx + 6;
                break;
            }
        }
        if choke_point_found {
            break;
        }
    }

    assert!(
        choke_point_found,
        "AppState::report_error definition was not found in source tree"
    );

    let mut offenders = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&src)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");

        // Skip dedicated test files
        if rel.ends_with("_tests.rs")
            || rel.contains("/tests/")
            || rel == "test_support/error_writes.rs"
            || rel == "scratch_pool.rs"
        {
            continue;
        }

        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let text = super::sources::without_test_items(&text);

        for (idx, line) in text.lines().enumerate() {
            let line_num = idx + 1;
            let trimmed = line.trim();

            // If this is the derived choke point line range, allow the assignment
            if rel == choke_file_rel && line_num >= choke_line_start && line_num <= choke_line_end {
                continue;
            }

            // Check for direct assignment to last_error with Some(...)
            if trimmed.contains("last_error = Some(")
                || trimmed.contains(".last_error = Some(")
                || (trimmed.contains("last_error =") && trimmed.contains("Some("))
            {
                offenders.push(format!("{rel}:{line_num}: {trimmed}"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "direct production writes to `last_error` found outside `AppState::report_error`:\n  {}",
        offenders.join("\n  ")
    );
}

/// No production write to `status_message` formats an error value into its
/// sentence.
///
/// The class this closes: a failure with a fix (a file that could not be
/// written, a plugin that did not mount, a decode that failed) posted on the
/// status line, which draws a receipt in faint ink and clears it after a few
/// seconds. A failure goes through `AppState::report_error` to the error line.
/// A statement that assigns `status_message` and interpolates `{err…}`, `{e}`
/// or `{error…}` is rejected. Not caught: an error value passed through a
/// variable of another name, or stored as a dialog's own reason and shown
/// again (`err.to_string()` of a form's refusal), which is a refusal and not
/// a failure.
#[test]
fn no_production_status_line_formats_an_error_value() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);
    let mut offenders = Vec::new();
    let mut writes = 0;
    for file in &files {
        let rel = file
            .strip_prefix(&src)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.ends_with("_tests.rs") || rel.starts_with("test_support") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let text = super::sources::without_test_items(&text);
        for (at, _) in text.match_indices("status_message =") {
            writes += 1;
            let statement = &text[at..text[at..].find(';').map_or(text.len(), |end| at + end)];
            if ["{err", "{e}", "{e:", "{error"]
                .iter()
                .any(|token| statement.contains(token))
            {
                let line = text[..at].matches('\n').count() + 1;
                offenders.push(format!("{rel}:{line}: {}", statement.trim()));
            }
        }
    }
    assert!(
        writes > 50,
        "found {writes} status line writes, which cannot be this crate"
    );
    assert!(
        offenders.is_empty(),
        "status line writes that carry an error value; post them with `report_error`:\n  {}",
        offenders.join("\n  ")
    );
}
