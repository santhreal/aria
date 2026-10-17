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

use std::collections::BTreeMap;
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
/// text is not a frame; the builders are excluded for the reason above. A file
/// that keeps its tests inline is cut at its first `#[cfg(test)]`, which in this
/// crate is always the last item in the file: a test that formats an id it
/// expects to find in a registry is not a control naming itself.
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
                || name == "types_census.rs"
                || BUILDERS.iter().any(|layer| name.starts_with(layer))
            {
                return None;
            }
            let text = std::fs::read_to_string(&file).ok()?;
            let text = match text.find("#[cfg(test)]") {
                Some(at) => text[..at].to_string(),
                None => text,
            };
            Some((name, text))
        })
        .collect()
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
        "only {} copied-list fields found, expected at least {LIST_FLOOR} — the parse this rule \
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
        "{} renders copy a whole store list, once per frame each — hold the list behind a \
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
        "{} draw(s) build the job list instead of reading `job_view`:\n  {}",
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
                || name == "types_census.rs"
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
        "{} site(s) enumerate the property catalog themselves, which clones every row \
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
        "{} site(s) in the draw layer walk the folder list, which copies every folder \
         it keeps on every frame — read `bin_path()` or `child_bins()` instead:\n  {}",
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
        "{} site(s) in the draw layer copy a whole projected list on every frame — hold \
         the list as an `Arc<[_]>` and hand back a handle instead:\n  {}",
        copies.len(),
        copies.join("\n  ")
    );
}

/// **One place decides which folder is open.**
///
/// The two folder views are built from that field and from the folder tree, so a
/// command that moved the operator and wrote the field itself left the
/// breadcrumb and the rows describing the folder they came from — which is
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
        "which folder is open is assigned in {} place(s); it has one owner, \
         `AppState::set_open_bin`, because writing it without rebuilding the folder \
         views leaves them describing the folder before the move:\n  {}",
        writes.len(),
        writes.join("\n  ")
    );
}

/// `text` with every whole-line comment blanked.
///
/// Two reasons: a rule about `format!` is described in prose a file away and
/// must not match itself, and a comment's parentheses would otherwise unbalance
/// the span the next rule matches.
fn without_comments(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.trim_start().starts_with("//") {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every element-identity construction in `text`, as `(line number, source)`.
///
/// A construction is `.id(`, `.with_animation(` or `ElementId::` followed by its
/// balanced argument list, so an id split over four lines is read as one thing
/// and a nested call inside it does not end the span early. A type annotation
/// such as `id: gpui::ElementId,` is not a construction, and is skipped because
/// the text between the name and the next `(` carries a separator no call would
/// have.
///
/// An animation is one of these because GPUI keys animation state by element id
/// exactly as it keys interaction state, so a one-shot transition named with
/// `format!` allocates on every frame it draws, which is every frame it runs.
fn identity_spans(text: &str) -> Vec<(usize, String)> {
    const NEEDLES: [&str; 3] = [".id(", "ElementId::", ".with_animation("];
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    for needle in NEEDLES {
        let mut from = 0;
        while let Some(found) = text[from..].find(needle) {
            let start = from + found;
            from = start + needle.len();
            let after = start + needle.len();
            let Some(open) = text[start..].find('(').map(|at| start + at).filter(|open| {
                *open <= after || !text[after..*open].contains([',', ';', ')', '\n'])
            }) else {
                continue;
            };
            let mut depth = 0usize;
            let mut end = open + 1;
            for (offset, byte) in bytes[open..].iter().enumerate() {
                match byte {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = open + offset + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            spans.push((text[..start].lines().count(), text[start..end].to_string()));
        }
    }
    spans
}

/// **An element identity is never built out of text.**
///
/// `ElementId` converts from `String` and so does `SharedString`, so a control
/// could name itself with `format!` — and fourteen did, once per multicam angle,
/// mixer strip, meter, fader, live job, curve parameter and easing preset, on
/// every frame each drew. The loaders then hashed the same text straight back
/// into a motion key, so the string was allocated to be thrown away.
///
/// The shared controls take a `motion::ControlId` now, which converts from a
/// static name or a `MotionKey` and from nothing else, so those call sites
/// cannot return. A raw `.id(…)` on a `div` still accepts an owned string, which
/// is what this counts.
#[test]
fn no_element_identity_is_built_from_a_formatted_string() {
    let mut built = Vec::new();
    for (name, text) in draw_sources() {
        for (line, span) in identity_spans(&without_comments(&text)) {
            if span.contains("format!") {
                built.push(format!("{name}:{line}: {}", span.replace('\n', " ")));
            }
        }
    }
    assert!(
        built.is_empty(),
        "{} element identit(ies) are formatted on every frame they draw — hash the \
         parts with `MotionKey::new` and pass the key, or name the control with a \
         literal:\n  {}",
        built.len(),
        built.join("\n  ")
    );
}

/// The span of `text` from `at` up to the first `;` outside every bracket.
///
/// A statement rather than a line, so an initialiser that runs over four lines
/// is read as one expression and a `;` inside a closure does not end it early.
fn statement(text: &str, at: usize) -> &str {
    let mut depth = 0i32;
    for (offset, byte) in text.as_bytes()[at..].iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b';' if depth == 0 => return &text[at..at + offset],
            _ => {}
        }
    }
    &text[at..]
}

/// The span from the bracket at `open` to the one that closes it, inclusive.
fn bracketed(text: &str, open: usize) -> &str {
    let mut depth = 0i32;
    for (offset, byte) in text.as_bytes()[open..].iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open..=open + offset];
                }
            }
            _ => {}
        }
    }
    &text[open..]
}

/// Whether `name` says the value it holds is a control's identity.
fn names_an_identity(name: &str) -> bool {
    matches!(name, "id" | "key") || name.ends_with("_id") || name.ends_with("_key")
}

/// Every function in `text`, as `(line number, name, parameter list, body)`.
fn functions(text: &str) -> Vec<(usize, &str, &str, &str)> {
    let mut found = Vec::new();
    for (at, _) in text.match_indices("fn ") {
        let after = at + 3;
        let name_len = text[after..]
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(0);
        let Some(open) = text[after..].find('(').map(|offset| after + offset) else {
            continue;
        };
        let parameters = bracketed(text, open);
        let rest = open + parameters.len();
        // A trait method with no body, or a function pointer type: nothing draws.
        let Some(brace) = text[rest..].find('{').map(|offset| rest + offset) else {
            continue;
        };
        if text[rest..brace].contains(';') {
            continue;
        }
        found.push((
            text[..at].lines().count(),
            &text[after..after + name_len],
            parameters,
            bracketed(text, brace),
        ));
    }
    found
}

/// **An identity is not laundered through a local either.**
///
/// The rule above matches the construction, so a `format!` bound to a name and
/// handed over afterwards would slip past it — which is the shape both the
/// colour wheels and the timeline's grips had: `let side_id = if left {
/// format!("trim-l-{id}") } else { … }`, four of them per clip per frame, on the
/// hottest surface in the app. The initialiser is read to its `;`, so the
/// `format!` does not have to sit on the line the name is on.
#[test]
fn no_draw_binds_an_identity_to_a_formatted_string() {
    let mut bound = Vec::new();
    for (name, text) in draw_sources() {
        for (at, _) in text.match_indices("let ") {
            let bound_at = statement(&text, at);
            let Some((head, value)) = bound_at.split_once('=') else {
                continue;
            };
            if !value.contains("format!") {
                continue;
            }
            let declared = head.split(':').next().unwrap_or(head).trim();
            let local = declared.rsplit(' ').next().unwrap_or(declared);
            if names_an_identity(local) {
                bound.push(format!(
                    "{name}:{}: {}",
                    text[..at].lines().count(),
                    bound_at.replace('\n', " ")
                ));
            }
        }
    }
    assert!(
        bound.is_empty(),
        "{} identit(ies) in the draw layer are built from text and then passed on — \
         build a `MotionKey` from the parts instead:\n  {}",
        bound.len(),
        bound.join("\n  ")
    );
}

/// **A helper that draws a control takes its identity, not text.**
///
/// The third way round the rule, and the one that survived longest: a panel's
/// own `chip` or `item` or `handle_dot` took a `String` and assigned it to an
/// element id itself, so the caller could format one and neither rule above was
/// looking at that argument. The mixer did, once per output chip and once per
/// send chip, per strip, per frame.
///
/// A function is matched when it assigns an element id and names a parameter as
/// an identity, so a new helper is red until its identity is a
/// `motion::ControlId` — which is the only identity type a caller cannot build
/// out of text. Parameters GPUI hands an element itself are prefixed with `_`
/// and are not the control's own identity.
#[test]
fn no_draw_helper_names_its_control_with_text() {
    const TEXT: [&str; 3] = ["String", "SharedString", "ElementId"];
    let mut helpers = Vec::new();
    for (name, text) in draw_sources() {
        for (line, function, parameters, body) in functions(&text) {
            if !body.contains(".id(") && !body.contains(".with_animation(") {
                continue;
            }
            for parameter in parameters.split(',') {
                let Some((held, kind)) = parameter.split_once(':') else {
                    continue;
                };
                let held = held.trim_start_matches(['(', ' ', '\n']).trim();
                if held.starts_with('_') || !names_an_identity(held) {
                    continue;
                }
                // `GlobalElementId` and `InspectorElementId` are GPUI's own,
                // handed to an element's layout rather than chosen by a caller.
                if kind.contains("GlobalElementId") || kind.contains("InspectorElementId") {
                    continue;
                }
                let borrowed_text = kind.trim_start().starts_with('&') && kind.contains("str");
                if borrowed_text || TEXT.iter().any(|shape| kind.contains(shape)) {
                    helpers.push(format!("{name}:{line}: fn {function}({held}:{kind})"));
                }
            }
        }
    }
    assert!(
        helpers.is_empty(),
        "{} draw helper(s) assign an element id from an identity they took as text, so a \
         caller can format one — take a `motion::ControlId`:\n  {}",
        helpers.len(),
        helpers.join("\n  ")
    );
}

/// **A function that hands back an identity does not build one out of text.**
///
/// The two rules above catch a control naming itself where it draws. This
/// catches the same thing one call away: a helper named `guide_id`, `frame_id`
/// or `stop_id` whose answer is a `format!`. The snap guide did exactly that,
/// once per magnet, on every mouse move of every drag, to name a line that was
/// on screen for a fifth of a second.
///
/// A name is the test rather than the return type, because the return type is
/// what the fix changes: these return a `MotionKey` or a `ControlId`, and
/// neither is reachable from a string.
#[test]
fn no_draw_returns_an_identity_built_from_text() {
    let mut builders = Vec::new();
    for (name, text) in draw_sources() {
        for (line, function, _, body) in functions(&without_comments(&text)) {
            if names_an_identity(function) && body.contains("format!") {
                builders.push(format!("{name}:{line}: fn {function}"));
            }
        }
    }
    assert!(
        builders.is_empty(),
        "{} function(s) named for an identity build one with `format!`, so every caller \
         allocates per frame — return a `MotionKey` or a `motion::ControlId`:\n  {}",
        builders.len(),
        builders.join("\n  ")
    );
}
