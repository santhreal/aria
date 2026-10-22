//! What the store may hold, and what a draw may copy.
//!
//! The store's types are mirrors: the projection builds them from the model once
//! per change, and every panel reads them once per frame. A mirror field that
//! holds a `String` therefore costs an allocation *per frame per visible row*,
//! because gpui takes a `SharedString` to draw text and the only way to get one
//! out of a `String` is to copy it. The draw path was full of
//! `SharedString::from(row.label.clone())`: a heap copy of the label, thrown away
//! at the end of the frame, sixty times a second, for every row on screen.
//!
//! `SharedString` in the mirror makes the same read an `Arc` bump. The rules here
//! keep it that way without a list of blessed sites: the field types are read out
//! of the `types_*.rs` sources, and the draw sites are read out of the whole draw
//! layer, so a new mirror field that holds an owned string, or a new draw that
//! copies one, goes red the moment it exists.

use std::path::{Path, PathBuf};

/// The mirror modules: every type the store hands a panel lives in one of these.
///
/// `app/clip_params.rs` is a builder for everything else, but the row it hands
/// the inspector is a mirror by every property that matters here: it is built
/// per group per frame and its text is drawn, so an owned string in it is an
/// allocation per row per frame.
const MIRRORS: &[&str] = &[
    "types.rs",
    "types_clip.rs",
    "types_clip_bar.rs",
    "types_editor.rs",
    "types_lane.rs",
    "types_pool.rs",
    "types_target.rs",
    "app/clip_params.rs",
];

/// Fewest mirror fields the rule expects to find. A refactor that moves the
/// mirrors somewhere else makes this go red rather than pass on an empty scan.
const FIELD_FLOOR: usize = 120;

/// Fewest draw files the copy rule expects to scan, for the same reason.
const DRAW_FLOOR: usize = 150;

/// The layers the copy rule does not reach, and why.
///
/// `app/` and `projection*.rs` are where a mirror is *built*: they read owned
/// text out of the model and have to copy it once, which is the allocation the
/// draw path is no longer paying. `app.rs` is that layer's module list and
/// declares its children by path, so it names them without drawing anything.
/// Everything else in the crate either draws or hands text to something that
/// draws, so a copy there is a per-frame copy.
const BUILDERS: &[&str] = &["app/", "app.rs", "projection"];

/// The crate's `src` directory.
fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `pub` field declared in `text`, as `(line number, source line)`.
fn public_fields(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let code = line.trim();
            let (head, _) = code.split_once(':')?;
            head.strip_prefix("pub ")
                .filter(|name| {
                    !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                })
                .map(|_| (index + 1, code.to_string()))
        })
        .collect()
}

/// Every source file that draws, as `(path relative to `src`, contents)`.
///
/// Test modules are excluded because a test that builds a fixture out of owned
/// text is not a frame: the `_tests.rs` files, and `test_support.rs` with the
/// modules under `test_support/`, which build only under `cfg(test)`. The
/// builders are excluded for the reason above. In every other file the
/// test-only items are blanked wherever they sit
/// ([`crate::test_support::sources::without_test_items`]): a test that formats
/// an id it expects to find in a registry is not a control naming itself, and
/// a test-only re-export above the draw code does not hide the draw code.
fn draw_sources() -> Vec<(String, String)> {
    let src = src();
    let mut files = Vec::new();
    crate::test_support::collect_rs(&src, &mut files);
    files
        .into_iter()
        .filter_map(|file| {
            let name = file
                .strip_prefix(&src)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            if name.ends_with("_tests.rs")
                || name.ends_with("/tests.rs")
                || name == "test_support.rs"
                || name.starts_with("test_support/")
                || name.starts_with("types_census")
                || BUILDERS.iter().any(|layer| name.starts_with(layer))
            {
                return None;
            }
            let text = std::fs::read_to_string(&file).ok()?;
            Some((
                name,
                crate::test_support::sources::without_test_items(&text),
            ))
        })
        .collect()
}

/// Fewest top-level functions the draw files declare outside a test-only item.
const TOP_LEVEL_FN_FLOOR: usize = 1000;

/// **The draw scans read past a test-only item.**
///
/// A test-only item can sit anywhere in a file, such as a `#[cfg(test)]`
/// re-export above the menu bar, and a scan that stops at the first one reads
/// only part of the file. Every top-level function a draw file declares outside
/// a test-only item is in the text the draw scans read. A top-level function
/// starts in the first column; it is test-only when an attribute line directly
/// above it is a test attribute.
#[test]
fn the_draw_scans_read_every_top_level_function_of_a_draw_file() {
    const DECLARES: [&str; 6] = [
        "fn ",
        "pub fn ",
        "pub(crate) fn ",
        "pub(super) fn ",
        "const fn ",
        "pub(crate) const fn ",
    ];
    let src = src();
    let mut checked = 0usize;
    let mut unread = Vec::new();
    for (name, scanned) in draw_sources() {
        let text = std::fs::read_to_string(src.join(&name))
            .unwrap_or_else(|err| panic!("{name} is a draw file and must read: {err}"));
        let lines: Vec<&str> = text.lines().collect();
        let scanned: Vec<&str> = scanned.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if !DECLARES.iter().any(|declares| line.starts_with(declares)) {
                continue;
            }
            let test_only = lines[..index]
                .iter()
                .rev()
                .take_while(|above| above.starts_with("#[") || above.starts_with("///"))
                .any(|above| {
                    ["#[cfg(test)]", "#[test]", "#[gpui::test"]
                        .iter()
                        .any(|attribute| above.starts_with(attribute))
                });
            if test_only {
                continue;
            }
            checked += 1;
            if scanned.get(index) != Some(line) {
                unread.push(format!("{name}:{}: {line}", index + 1));
            }
        }
    }
    assert!(
        checked >= TOP_LEVEL_FN_FLOOR,
        "found {checked} top-level draw functions, fewer than the {TOP_LEVEL_FN_FLOOR} this \
         rule was written against: the draw layer moved and it is now checking almost nothing"
    );
    assert!(
        unread.is_empty(),
        "{} top-level draw functions are missing from the text the draw scans read:\n  {}",
        unread.len(),
        unread.join("\n  ")
    );
}

/// **A mirror holds shared text, not owned text.**
///
/// A `String` in a store type is an allocation the draw path has to copy every
/// frame; a `SharedString` is a refcount. The rule reads the field list out of
/// the sources, so it cannot go stale and it has no exemption list: a field that
/// genuinely wants an owned string is a field that does not belong in a mirror.
#[test]
fn no_mirror_type_holds_an_owned_string() {
    let src = src();
    let mut fields = 0usize;
    let mut owned = Vec::new();
    for module in MIRRORS {
        let path = src.join(module);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{module} is a mirror module and must exist: {err}"));
        for (line, code) in public_fields(&text) {
            fields += 1;
            if code.contains("String") && !code.contains("SharedString") {
                owned.push(format!("{module}:{line}: {code}"));
            }
        }
    }
    assert!(
        fields >= FIELD_FLOOR,
        "found {fields} mirror fields, fewer than the {FIELD_FLOOR} this rule was written \
         against: the mirrors moved and it is now checking almost nothing"
    );
    assert!(
        owned.is_empty(),
        "{} mirror fields hold an owned string, which the draw path has to copy once per \
         frame per visible row — hold a `SharedString` and let the projection build it \
         once:\n  {}",
        owned.len(),
        owned.join("\n  ")
    );
}

/// **A draw does not copy a string to draw it.**
///
/// `SharedString::from(x.clone())` allocates a copy of `x` and then wraps the
/// copy: the `clone` is the whole cost, and it is paid every frame. Reading a
/// `SharedString` out of the store and cloning *that* is free, so the pattern
/// only ever appears where a mirror is still holding owned text. The scan covers
/// every module that draws, not only `panels/`: the same copy in the titlebar,
/// in a surface, in a shared component or in the pending-work rows is the same
/// per-frame allocation.
#[test]
fn no_draw_copies_a_string_to_hand_it_to_a_draw() {
    let sources = draw_sources();
    assert!(
        sources.len() >= DRAW_FLOOR,
        "found {} draw files, fewer than the {DRAW_FLOOR} this rule was written against: \
         the draw layer moved and it is now checking almost nothing",
        sources.len()
    );
    let mut copies = Vec::new();
    for (name, text) in &sources {
        for (index, line) in text.lines().enumerate() {
            let code = line.trim();
            let Some(rest) = code.split_once("SharedString::from(").map(|(_, rest)| rest) else {
                continue;
            };
            if rest.contains(".clone())") {
                copies.push(format!("{name}:{}: {code}", index + 1));
            }
        }
    }
    assert!(
        copies.is_empty(),
        "{} draw sites copy a string on the way into a `SharedString`, once per frame \
         each — hold the shared string in the store instead:\n  {}",
        copies.len(),
        copies.join("\n  ")
    );
}

/// **One module shares the catalogs, and a draw reads them from it.**
///
/// A model catalog holds `String`, because the model crate has no idea a UI
/// exists. A panel that reads one inside a render therefore rebuilds every id
/// and label it draws, once per frame, and two panels reading the same catalog
/// rebuild it twice. `catalog_text` shares each one behind a process-lifetime
/// accessor, so this rule is what makes a seventh catalog go through it rather
/// than quietly allocating: the only file allowed to name a model catalog is the
/// one whose whole job is sharing them.
#[test]
fn only_the_sharing_module_reads_a_model_catalog() {
    let mut direct = Vec::new();
    for (name, text) in draw_sources() {
        if name == "catalog_text.rs" {
            continue;
        }
        for (index, line) in text.lines().enumerate() {
            let code = line.trim();
            if code.starts_with("//") {
                continue;
            }
            if code.contains("aria_models::") && code.contains("_catalog()") {
                direct.push(format!("{name}:{}: {code}", index + 1));
            }
        }
    }
    assert!(
        direct.is_empty(),
        "{} draw sites read a model catalog directly, which rebuilds its text once per \
         frame — add an accessor to `catalog_text` and draw from that:\n  {}",
        direct.len(),
        direct.join("\n  ")
    );
}

#[path = "types_census_identities.rs"]
mod identities;
#[path = "types_census_lists.rs"]
mod lists;
