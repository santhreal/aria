//! Where a picture held by the UI leaves the window atlas.
//!
//! gpui keeps a painted picture's texture in the window atlas until
//! `App::drop_image` releases it, whether or not anything still holds the
//! picture. A type that holds pictures and lets one go without handing it to
//! [`crate::app::AppState::release_retired_images`] leaves its texture in the
//! atlas for the life of the window: the filmstrip, the pool and the Viewer
//! comparison each did.
//!
//! The census reads every type, static and alias in the crate that holds a
//! `RenderImage` and fails on one that does not state where its pictures
//! leave the atlas. Each owner's own tests prove the statement, with
//! [`held_or_released_once`] over every path that drops a picture.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use gpui::RenderImage;

use super::collect_rs;

/// Every picture in `given` is in `held` or in `released`, once and not both.
pub(crate) fn held_or_released_once<'a>(
    held: impl IntoIterator<Item = &'a Arc<RenderImage>>,
    released: &[Arc<RenderImage>],
    given: &[Arc<RenderImage>],
    step: &str,
) {
    let mut seen: HashMap<*const RenderImage, usize> = HashMap::new();
    let mut count = |image: &Arc<RenderImage>| *seen.entry(Arc::as_ptr(image)).or_default() += 1;
    for image in held {
        count(image);
    }
    for image in released {
        count(image);
    }
    for (index, image) in given.iter().enumerate() {
        assert_eq!(
            seen.get(&Arc::as_ptr(image)).copied().unwrap_or(0),
            1,
            "{step}: picture {index} is not held or released exactly once"
        );
    }
}

/// A file declaring a type, static or alias that holds a picture, and where
/// those pictures leave the atlas.
const HOLDERS: &[(&str, &str)] = &[
    (
        "crates/aria-ui/src/filmstrips.rs",
        "filmstrip cells: a cell the store drops goes to its retired list, \
         released by the landing or edit that dropped it",
    ),
    (
        "crates/aria-ui/src/app/pool_preview.rs",
        "pool thumbnails and the monitor frame: a picture the store drops goes \
         to its retired list, released when the next render lands",
    ),
    (
        "crates/aria-ui/src/app/viewer_compare.rs",
        "the comparison's reference: a picture let go of goes to its retired \
         list, released when the next picture lands or at the next release",
    ),
    (
        "crates/aria-ui/src/app/state.rs",
        "the Viewer frame on screen, which `show_frame` replaces into \
         `retired_images`, and `retired_images` itself",
    ),
    (
        "crates/aria-ui/src/app/scope_view.rs",
        "scope readings: `retire_scopes` moves a replaced reading into \
         `retired_images`",
    ),
    (
        "crates/aria-ui/src/app/plugin_panels.rs",
        "plugin panel pictures: `sync_plugin_panels` returns the ones no panel \
         shows any more, and `resync_plugin_panels` releases them",
    ),
    (
        "crates/aria-ui/src/preview_worker.rs",
        "a rendered frame in transit: `show_frame` takes it as the Viewer frame \
         on screen",
    ),
    (
        "crates/aria-ui/src/panels/timeline/clip.rs",
        "a filmstrip cell as one draw borrows it; the filmstrip store holds it",
    ),
    (
        "crates/aria-ui/src/icons.rs",
        "the brand mark: one picture decoded once for the life of the process",
    ),
];

/// The lines of `text` that declare a holder of a picture: a field or variant
/// of a `struct` or `enum`, a `static`, or a `type` alias, naming
/// `RenderImage`. A function's parameters and return type borrow a picture
/// and are not read.
fn holding_lines(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    // The brace depth inside the `struct` or `enum` being read, if any.
    let mut body: Option<usize> = None;
    for (number, line) in text.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("").trim();
        let words: Vec<&str> = code.split_whitespace().collect();
        let declares = |keyword: &str| words.iter().take(3).any(|word| *word == keyword);
        let names = code.contains("RenderImage");
        if body.is_none() && (declares("struct") || declares("enum")) {
            if names {
                found.push(number + 1);
            }
            if code.ends_with('{') {
                body = Some(1);
            }
            continue;
        }
        if let Some(depth) = body.as_mut() {
            if names {
                found.push(number + 1);
            }
            *depth += code.matches('{').count();
            *depth = depth.saturating_sub(code.matches('}').count());
            if *depth == 0 {
                body = None;
            }
            continue;
        }
        if names && (declares("static") || declares("type")) {
            found.push(number + 1);
        }
    }
    found
}

/// **A type that holds a picture states where the picture leaves the atlas.**
///
/// Derived from the sources: a new store of pictures turns this red on the
/// run that adds it, until its row states where it releases what it drops,
/// and a row whose file no longer holds a picture turns it red too.
///
/// Does not catch: a declared holder that stops releasing; each holder's own
/// tests catch that.
#[test]
fn every_holder_of_a_picture_states_where_it_leaves_the_atlas() {
    assert_eq!(
        holding_lines(
            "pub struct Shown {\n    image: Arc<RenderImage>,\n}\nfn draw(\n    image: \
             Arc<RenderImage>,\n) {}\nstatic MARK: LazyLock<Arc<RenderImage>> = x;\nenum \
             Slot {\n    Ready { image: Arc<gpui::RenderImage> },\n}\n"
        ),
        vec![2, 7, 9],
        "the reader sees a field, a static and a variant, and skips a parameter"
    );
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
    let mut holders = Vec::new();
    let mut undeclared = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&workspace)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        // Test files hold pictures for the length of a test.
        if rel.ends_with("_tests.rs") || rel.starts_with("crates/aria-ui/src/test_support") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let lines = holding_lines(&text);
        if lines.is_empty() {
            continue;
        }
        holders.push(rel.clone());
        if !HOLDERS.iter().any(|(name, _)| rel == *name) {
            undeclared.extend(lines.iter().map(|line| format!("{rel}:{line}")));
        }
    }
    let stale: Vec<&str> = HOLDERS
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !holders.iter().any(|held| held == name))
        .collect();
    assert!(
        stale.is_empty(),
        "these rows name a file that no longer holds a picture: {stale:?}"
    );
    assert!(
        undeclared.is_empty(),
        "a held picture stays in the window atlas until `App::drop_image` releases \
         it: hand every picture the type drops to \
         `AppState::release_retired_images`, then add a row to HOLDERS stating \
         where:\n  {}",
        undeclared.join("\n  ")
    );
}
