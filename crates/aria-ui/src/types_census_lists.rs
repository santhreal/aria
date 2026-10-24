use std::collections::BTreeMap;
use std::path::PathBuf;

use super::*;

/// The modules that declare what the store holds: the state itself, and the
/// mirrors the projection fills.
fn store_modules() -> Vec<&'static str> {
    let mut modules = vec!["app/state.rs"];
    modules.extend_from_slice(MIRRORS);
    modules
}

/// Fewest copied-list field names the rule expects to find. A parse that stops
/// working goes red here rather than passing on an empty set.
const LIST_FLOOR: usize = 40;

/// Every struct the crate declares, as `type name -> its field declarations`.
///
/// A line-wise parse: `struct Name {` opens a type and the matching brace ends
/// it. Generic structs are skipped, because a store field has never named one
/// and following one would mean resolving its parameters. Enough to walk from a
/// store field into the form it names, which is what a hand-kept list of field
/// names gets wrong the first time somebody adds one.
fn structs_in(files: &[PathBuf]) -> BTreeMap<String, Vec<(String, String)>> {
    let mut types: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let mut open: Option<(String, i64)> = None;
        for line in text.lines() {
            let code = line.trim();
            match &mut open {
                None => {
                    if let Some(name) = struct_name(code) {
                        types.entry(name.clone()).or_default();
                        open = Some((name, 1));
                    }
                }
                Some((name, depth)) => {
                    *depth += code.matches('{').count() as i64 - code.matches('}').count() as i64;
                    if let Some(field) = field_decl(code) {
                        types.entry(name.clone()).or_default().push(field);
                    }
                    if *depth <= 0 {
                        open = None;
                    }
                }
            }
        }
    }
    types
}

/// The name a `struct NAME {` line opens.
fn struct_name(code: &str) -> Option<String> {
    let rest = without_visibility(code).strip_prefix("struct ")?;
    let name = rest.strip_suffix(" {")?;
    let plain = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    plain.then(|| name.to_string())
}

/// The `name: Type` a field declaration carries.
fn field_decl(code: &str) -> Option<(String, String)> {
    let (name, ty) = without_visibility(code).split_once(": ")?;
    let ty = ty.strip_suffix(',')?;
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    plain.then(|| (name.to_string(), ty.to_string()))
}

fn without_visibility(code: &str) -> &str {
    code.strip_prefix("pub(crate) ")
        .or_else(|| code.strip_prefix("pub(super) "))
        .or_else(|| code.strip_prefix("pub "))
        .unwrap_or(code)
}

/// The innermost type a declaration names: `Option<Arc<Vec<UiCaption>>>` is
/// `UiCaption`.
fn inner_type(ty: &str) -> String {
    let mut rest = ty.trim();
    while let Some((_, tail)) = rest.split_once('<') {
        rest = tail;
    }
    rest.trim_end_matches(['>', ' '])
        .trim_matches(['[', ']'])
        .rsplit("::")
        .next()
        .unwrap_or(rest)
        .trim()
        .to_string()
}

/// Whether cloning a value of this type copies a list rather than bumping a
/// refcount.
///
/// A field already behind a handle costs a refcount whatever it points at, so
/// the walk stops there: putting the list behind an `Arc` is the fix, and the
/// rule has to accept its own fix.
fn copies_a_list(
    ty: &str,
    structs: &BTreeMap<String, Vec<(String, String)>>,
    seen: &mut Vec<String>,
) -> bool {
    let bare = ty
        .strip_prefix("Option<")
        .map_or(ty, |rest| rest.trim_end_matches('>'))
        .trim();
    if bare.starts_with("Vec<") {
        return true;
    }
    if bare.starts_with("Arc<") || bare.starts_with("std::sync::Arc<") || bare.starts_with("Rc<") {
        return false;
    }
    let name = inner_type(bare);
    if seen.contains(&name) {
        return false;
    }
    let Some(fields) = structs.get(&name) else {
        return false;
    };
    seen.push(name);
    let copies = fields
        .iter()
        .any(|(_, field_ty)| copies_a_list(field_ty, structs, seen));
    seen.pop();
    copies
}

/// Every field name a draw may not clone: a field the store reaches whose clone
/// copies a list, whether the list is the field itself or sits inside the form
/// the field names.
fn copied_list_fields() -> Vec<String> {
    let src = src();
    let mut files = Vec::new();
    crate::test_support::collect_rs(&src, &mut files);
    let structs = structs_in(&files);
    let roots: Vec<PathBuf> = store_modules()
        .into_iter()
        .map(|module| src.join(module))
        .collect();
    let mut queue: Vec<String> = structs_in(&roots).into_keys().collect();
    let mut walked: Vec<String> = Vec::new();
    let mut names = Vec::new();
    while let Some(ty) = queue.pop() {
        if walked.contains(&ty) {
            continue;
        }
        walked.push(ty.clone());
        let Some(fields) = structs.get(&ty) else {
            continue;
        };
        for (field, field_ty) in fields {
            if copies_a_list(field_ty, &structs, &mut Vec::new()) {
                names.push(field.clone());
            }
            // Through a handle as well: a `Vec` inside a shared store is still
            // copied by a draw that names it and clones it.
            queue.push(inner_type(field_ty));
        }
    }
    names.sort();
    names.dedup();
    names
}

/// **A render clones a handle, not a list.**
///
/// A panel cannot hold a read borrow of the store while it builds listeners, so
/// a panel that wants a whole list clones it, and that clone is the entire list
/// once per frame: the conversation, the marker list, the open-keyframe set and
/// the loaded preset pack were each copied sixty times a second. A list behind
/// an `Arc` costs a refcount instead, and a mutation takes it back with
/// copy-on-write, so the copy happens at most once per edit rather than once per
/// frame. The field types are read out of the store's own sources and followed
/// into the forms they name, so a list one level down — a caption's words
/// inside the captions form — counts too, and a panel that clones the form to
/// read a flag off it is the same copy as cloning the list directly. No list of
/// blessed fields exists to go stale.
#[test]
fn no_render_clones_a_store_list() {
    let fields = copied_list_fields();
    assert!(
        fields.len() >= LIST_FLOOR,
        "only {} copied-list fields found, expected at least {LIST_FLOOR}; the parse this rule \
         reads the store with has stopped working",
        fields.len()
    );
    let mut copies = Vec::new();
    for (name, text) in draw_sources() {
        for (index, line) in text.lines().enumerate() {
            let code = line.trim();
            if code.starts_with("//") || !code.contains("read(cx)") {
                continue;
            }
            for field in &fields {
                if code.contains(&format!(".{field}.clone()")) {
                    copies.push(format!("{name}:{}: {code}", index + 1));
                }
            }
        }
    }
    assert!(
        copies.is_empty(),
        "{} renders copy a whole store list, once per frame each; hold the list behind a \
         handle and clone that:\n  {}",
        copies.len(),
        copies.join("\n  ")
    );
}

/// **A draw reads a projection, it does not build one.**
///
/// The board was the last projection a panel built for itself: the pending
/// strip, the jobs dialog, the export sheet and the status line each called into
/// the job registry per frame, taking its mutex and cloning every job's label,
/// detail and outputs, so one frame did that four times while the job threads
/// writing progress waited on the same lock. It is projected on publication now
/// and read from a handle, and this keeps it that way: a draw that reaches for
/// the registry, the export queue or the refresh itself is red.
#[test]
fn no_draw_builds_the_job_list() {
    const REACHES: &[&str] = &[
        "refresh_jobs(",
        ".jobs.",
        ".export_queue.",
        "export_queue_snapshot(",
    ];
    let mut builds = Vec::new();
    for (name, text) in draw_sources() {
        for (index, line) in text.lines().enumerate() {
            let code = line.trim();
            if code.starts_with("//") {
                continue;
            }
            if REACHES.iter().any(|reach| code.contains(reach)) {
                builds.push(format!("{name}:{}: {code}", index + 1));
            }
        }
    }
    assert!(
        builds.is_empty(),
        "draws that build the job list instead of reading `job_view`: {}\n  {}",
        builds.len(),
        builds.join("\n  ")
    );
}

/// The files allowed to enumerate the property catalog for a clip, and why.
///
/// `app/clip_keys.rs` owns the projected property list, which is what every
/// other reader asks. `projection_clip.rs` builds the timeline strip's keyed
/// ticks, which is the union over every clip in the sequence rather than the
/// selected clip's rows, so it is a different answer and is paid once per
/// projection.
const CATALOG_READERS: &[&str] = &["app/clip_keys.rs", "projection_clip.rs"];

/// Every source file that runs in a frame or a command, as `(name, contents)`.
///
/// Unlike [`draw_sources`] this keeps the projection layer: enumerating the
/// property catalog costs the same wherever it happens, and the point of the
/// rule below is that exactly one module does it.
fn runtime_sources() -> Vec<(String, String)> {
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
            // A census render (`drawn_census_*.rs`) drives a surface from a test
            // and derives its expectations from the catalog at run time, which is
            // the fixture rule the other censuses follow rather than a frame.
            if name.ends_with("_tests.rs")
                || name.ends_with("/tests.rs")
                || name.contains("drawn_census")
                || name == "test_support.rs"
                || name.starts_with("types_census")
                || CATALOG_READERS.contains(&name.as_str())
            {
                return None;
            }
            let text = std::fs::read_to_string(&file).ok()?;
            Some((name, text))
        })
        .collect()
}

/// **One module enumerates the property catalog, and everything else reads its
/// rows.**
///
/// `clip_params_for` clones every catalog row that applies to a clip. The
/// inspector called it once for the row list, again per row to read that row's
/// keys, again per group for the stepper rows, and again per sample while
/// drawing a curve — so opening the keyframe block cloned the same rows
/// thousands of times per frame, and the cost grew with the square of the
/// property count. The rows are projected once per change now
/// ([`crate::app::clip_keys::key_params_of`]), carry their own keys, and are
/// read through `key_params` and `key_param`. This keeps it that way: a surface
/// that wants a property asks for the row.
#[test]
fn only_the_projection_enumerates_the_property_catalog() {
    let mut readers = Vec::new();
    for (name, text) in runtime_sources() {
        for (index, line) in text.lines().enumerate() {
            let code = line.trim();
            if code.starts_with("//") {
                continue;
            }
            if code.contains("clip_params_for(") || code.contains("clip_param_catalog(") {
                readers.push(format!("{name}:{}: {code}", index + 1));
            }
        }
    }
    assert!(
        readers.is_empty(),
        "sites that enumerate the property catalog themselves: {}, which clones every row \
         that applies to the clip — read the projected rows through `key_params` or \
         `key_param` instead:\n  {}",
        readers.len(),
        readers.join("\n  ")
    );
}

/// **A draw reads the folder views, it does not build them.**
///
/// The pool's breadcrumb and its folder rows were two walks of the whole folder
/// list per frame, each cloning every folder it kept, and the strip's own count
/// was a third. They are one projection now
/// ([`crate::app::AppState::refresh_bins`]) read through `bin_path`,
/// `child_bins` and the projected length. This keeps it that way: a surface that
/// wants the folders asks for the list.
#[test]
fn no_draw_walks_the_pool_folders() {
    let mut walkers = Vec::new();
    for (name, text) in draw_sources() {
        for (index, line) in text.lines().enumerate() {
            let code = line.trim();
            if code.starts_with("//") {
                continue;
            }
            if code.contains(".bins") {
                walkers.push(format!("{name}:{}: {code}", index + 1));
            }
        }
    }
    assert!(
        walkers.is_empty(),
        "sites in the draw layer that walk the folder list: {}, which copies every folder \
         it keeps on every frame; read `bin_path()` or `child_bins()` instead:\n  {}",
        walkers.len(),
        walkers.join("\n  ")
    );
}

/// **A draw does not copy a list it reached through the state.**
///
/// A projected list is built once and read unchanged, so a `to_vec()` on the way
/// into a frame buys nothing and costs the whole list: the history dialog copied
/// every step of the undo stack, the project report copied its project facts and
/// its clip facts, and the multicam strip copied every camera group, on every
/// frame each was open. Each of those lists is a handle now and the accessor
/// hands one back, so a frame refcounts.
///
/// The rule is about lists reached through the state, which is the only thing a
/// frame pays for repeatedly. A copy of an event payload — the paths of a file
/// drop, say — happens once per gesture and is not a frame cost, so it is not
/// matched: a draw reaches the state through `state.read(…)` or a binding taken
/// from it, and nothing else in this crate looks like that.
#[test]
fn no_draw_copies_a_list_it_reached_through_the_state() {
    let mut copies = Vec::new();
    for (name, text) in draw_sources() {
        for (index, line) in text.lines().enumerate() {
            let code = line.trim();
            if code.starts_with("//") || !code.contains(".to_vec()") {
                continue;
            }
            let through_state = code.contains("state.read(")
                || code.contains(".store.")
                || code.starts_with("read.")
                || code.contains(" read.")
                || code.contains("(read.");
            if through_state {
                copies.push(format!("{name}:{}: {code}", index + 1));
            }
        }
    }
    assert!(
        copies.is_empty(),
        "sites in the draw layer that copy a whole projected list on every frame: {}; hold \
         the list as an `Arc<[_]>` and hand back a handle instead:\n  {}",
        copies.len(),
        copies.join("\n  ")
    );
}

/// **One place decides which folder is open.**
///
/// The two folder views are built from that field and from the folder tree, so a
/// command that moved the view and wrote the field itself left the
/// breadcrumb and the rows describing the folder it came from, which is
/// exactly what creating a folder inside another one did. Every move goes
/// through `AppState::set_open_bin`, which writes and rebuilds, and this counts
/// the assignments so the next command cannot reintroduce it.
#[test]
fn one_place_decides_which_folder_is_open() {
    let src = src();
    let mut files = Vec::new();
    crate::test_support::collect_rs(&src, &mut files);
    let mut writes = Vec::new();
    for file in files {
        let name = file
            .strip_prefix(&src)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let code = line.trim();
            if code.starts_with("//") {
                continue;
            }
            if code.contains("current_bin = ") && !code.contains("current_bin == ") {
                writes.push(format!("{name}:{}: {code}", index + 1));
            }
        }
    }
    assert_eq!(
        writes.len(),
        1,
        "places that assign which folder is open: {}; it has one owner, \
         `AppState::set_open_bin`, because writing it without rebuilding the folder \
         views leaves them describing the folder before the move:\n  {}",
        writes.len(),
        writes.join("\n  ")
    );
}
