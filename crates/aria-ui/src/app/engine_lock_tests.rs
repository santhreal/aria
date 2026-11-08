//! **A frame takes no session lock.**
//!
//! The engine sits behind a mutex and a preview decode holds it for the whole
//! length of a decode — tens of milliseconds on a warm cache, far longer on a
//! cold seek into a long file. Any value a panel read by locking the session was
//! therefore a frame that could park behind a decode, and the editor read
//! fifteen of them per repaint: the canvas size three times in the settings
//! dialog, the undo list twice in the history panel, the export sheet's output
//! size twice through a resolve, the proxy flag once per pool row, and every
//! inspector parameter once per widget.
//!
//! Those values are projected now — [`crate::app::project_facts`],
//! [`crate::app::selected_detail`], [`crate::app::project_report`] — built from
//! the same engine read that builds the store and keyed by the same projection
//! key, so a drawn value cannot disagree with the store beside it.
//!
//! What is left is declared. A report opened from a menu, a job about to start,
//! a pointer hit test and a republish to the audio engine may all take the
//! session, because none of them is a frame. The list below is that declaration
//! and the test is the fence around it: a `&self` method, or a function taking
//! `&AppState` such as a menu row's guard, that starts locking — directly, or
//! by calling something that does — turns this red on the run that adds it.
//!
//! **What this does not catch.** A `&mut self` method may lock freely and is not
//! examined, because an edit already owns the session; the frame reads `&self`.
//! A lock is seen where it is spelled, where it is one of the project host's
//! methods that lock (read from `aria-plugin`'s source) called on `self.host`
//! or on a `&ProjectHost` parameter, and where it is reached through a call on
//! the reader's `self` or `&AppState` parameter, on one of its fields, or of a
//! function by its bare or module path. One reached through a closure stored in
//! a registry, an associated function called through its type, or a receiver
//! two fields deep is not; nor is a call of a declared reader from a frame. The behavioral half of the proof is
//! `shell/panel_view_session_tests.rs`, which holds the mutex on a helper
//! thread while the window draws, and which does not care how the lock was
//! reached.

use std::path::{Path, PathBuf};

use crate::test_support::collect_rs;

/// A reader that is allowed to take the session, and why it is not a frame.
///
/// `(file, function, reason)`. A row here that stops locking is as much a failure
/// as a lock without a row: a stale declaration is a fence around nothing.
const DECLARED: &[(&str, &str, &str)] = &[
    (
        "app/ai_reach_tools.rs",
        "ai_system_prompt",
        "builds the model's system prompt when a turn is sent, not while drawing one",
    ),
    (
        "app/assistant.rs",
        "lower",
        "validates an assistant command against the project on the turn's own thread",
    ),
    (
        "app/captions.rs",
        "transcription_precheck",
        "copies the selected clip into a transcription request when a run is started",
    ),
    (
        "app/catalog_drag.rs",
        "clip_on",
        "resolves the clip at the drop point when a catalog drag is released onto a track",
    ),
    (
        "app/drag_room.rs",
        "drag_room",
        "reads the room of a trim, roll, slide or stretch about to land from a release \
         or a key; a drag in flight reads the room it held at its press",
    ),
    (
        "app/duck.rs",
        "voice_timeline_spans",
        "walks the voice lane for the ducking command; nothing draws spans",
    ),
    (
        "app/effect_header.rs",
        "effect_on",
        "reads the effect a header verb is about to reset, rename, duplicate, copy or save",
    ),
    (
        "app/effect_header.rs",
        "chain_len",
        "bounds a move or a paste of an effect that is about to land",
    ),
    (
        "app/effect_header.rs",
        "next_free_effect_name",
        "numbers the copy a duplicate is about to add",
    ),
    (
        "app/export_options.rs",
        "export_settings",
        "resolves an export against the project for a job that is about to start; \
         the sheet's own rows read export_size, which is projected",
    ),
    (
        "app/media.rs",
        "lane_kind_for",
        "reads the kind of lane a drop on a new lane is about to add",
    ),
    (
        "app/media.rs",
        "media_placement",
        "reads a pool item's full range for a drop about to land",
    ),
    (
        "app/media_reach_report.rs",
        "build_coverage",
        "writes the requested media-support report",
    ),
    (
        "app/project_reach.rs",
        "lanes_accepting_selection",
        "answers which lanes take the selection for the duplicate command",
    ),
    (
        "app/proxy.rs",
        "proxy_file",
        "names the proxy a source would decode through, for a proxy job",
    ),
    (
        "app/proxy.rs",
        "media_path",
        "resolves a source path for a proxy job",
    ),
    (
        "app/render_reach.rs",
        "image_sequence_settings",
        "the same resolve as export_settings, for the PNG-sequence job",
    ),
    (
        "app/rename.rs",
        "current_name",
        "seeds a rename draft when one opens and compares it on commit; an effect's \
         name is read from the model chain the header menu was opened on",
    ),
    (
        "app/source_patch.rs",
        "source_channels",
        "reads a pool item's channels for a drop or a Source edit about to land",
    ),
    (
        "app/text_reach.rs",
        "title_ink_report",
        "measures every title's ink for the requested report",
    ),
    (
        "app/text_reach_shapes.rs",
        "clip_at_canvas_point",
        "hit-tests the canvas under the pointer; a click, not a frame",
    ),
    (
        "app/tracks.rs",
        "track_is_duck_source",
        "answers whether a lane is the ducking source for the lane command",
    ),
    (
        "app/transport.rs",
        "playback_snapshot",
        "republishes the mix to the audio engine when the timeline moves",
    ),
    (
        "app/transport.rs",
        "republish_audio",
        "hands that snapshot to the audio engine after an edit or a seek",
    ),
    (
        "app/trim_gestures.rs",
        "compute_trim_feedback",
        "reads the room of a drag through timeline_gesture::held, which a drag in flight \
         holds from its press",
    ),
    (
        "app/view_events.rs",
        "plugin_menu_rows",
        "hands plugins the project once when a context menu opens; the drawn menu reads \
         the rows it stored",
    ),
    (
        "app/viewer_match.rs",
        "clip_frame",
        "maps the playhead into a clip's take when Match Frame or a double-click \
         opens a Source tab",
    ),
    (
        "app/viewer_match.rs",
        "linked_trim",
        "converts a Source tab's new In or Out into the trim the clip takes",
    ),
    (
        "surfaces/context_menu/badge.rs",
        "of",
        "reads a clip's properties when its badge menu opens; the drawn menu reads the \
         facts open_context stored",
    ),
    (
        "surfaces/timeline_gesture.rs",
        "held",
        "reads the room through drag_room, which a drag in flight holds from its press; a \
         release or a key with no drag reads it once",
    ),
];

/// How the session state is spelled as a parameter type, whitespace squashed.
const STATE_TYPES: [&str; 3] = ["&AppState", "&crate::app::AppState", "&crate::AppState"];

/// How a lock is spelled, whitespace squashed.
const LOCKS: [&str; 2] = ["engine.lock(", "engine().lock("];

/// The project host, whose methods that lock the engine are locks too.
const PROJECT_HOST: &str = "../aria-plugin/src/project_host.rs";

/// One `&self` method, or one function taking `&AppState`.
struct Reader {
    file: String,
    line: usize,
    name: String,
    /// What the reader reads the session through: `self`, or the name of its
    /// `&AppState` parameter.
    receiver: String,
    /// The parameter list, whitespace squashed: `(&self,host:&ProjectHost`.
    params: String,
    body: String,
}

/// `text` with every whitespace character dropped, so a call a formatter split
/// over four lines is the same call to this scan.
fn squash(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Every `&self` method and every function with a `&AppState` parameter
/// declared in `text`, with its body.
///
/// Bodies are found by matching braces from the header's opening brace, so a
/// nested block, a closure, or a string containing a brace inside a body stays
/// inside that body. A `&mut self` method and a `&mut AppState` parameter are
/// skipped: an edit owns the session.
fn readers(file: &str, text: &str) -> Vec<Reader> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut at = 0usize;
    while let Some(offset) = text[at..].find("fn ") {
        let start = at + offset;
        at = start + 3;
        let Some(open) = text[start..].find('(').map(|i| start + i) else {
            break;
        };
        let name: String = text[start + 3..open].trim().to_string();
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let Some(close) = text[open..].find(')').map(|i| open + i) else {
            continue;
        };
        let args = squash(&text[open..close]);
        let receiver = if args.starts_with("(&self") {
            "self"
        } else {
            let state = args[1..].split(',').find_map(|param| {
                let (param, kind) = param.split_once(':')?;
                STATE_TYPES.contains(&kind).then_some(param)
            });
            match state {
                Some(state) => state,
                None => continue,
            }
        };
        let Some(brace) = text[close..].find('{').map(|i| close + i) else {
            continue;
        };
        let mut depth = 0usize;
        let mut end = brace;
        while end < bytes.len() {
            match bytes[end] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            end += 1;
        }
        found.push(Reader {
            file: file.to_string(),
            line: text[..start].matches('\n').count() + 1,
            name,
            receiver: receiver.to_string(),
            params: args,
            body: squash(&text[brace..end.min(bytes.len())]),
        });
        at = end;
    }
    found
}

/// Whether `text` ends with `word`, and not with a longer identifier that ends
/// the same way.
fn ends_with_word(text: &str, word: &str) -> bool {
    text.strip_suffix(word)
        .is_some_and(|before| !before.ends_with(|c: char| c.is_alphanumeric() || c == '_'))
}

/// Whether `body` calls `name` on `on`, on a field of `on`, or on what a
/// getter of `on` returns: `on.name(`, `on.field.name(` or `on.field().name(`.
/// With `field`, only a call on that field counts.
fn calls_on(body: &str, on: &str, name: &str, field: Option<&str>) -> bool {
    let call = format!(".{name}(");
    let dotted = format!("{on}.");
    body.match_indices(&call).any(|(at, _)| {
        let receiver = &body[..at];
        if field.is_none() && ends_with_word(receiver, on) {
            return true;
        }
        let receiver = receiver.strip_suffix("()").unwrap_or(receiver);
        let start = receiver
            .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
            .map_or(0, |i| i + 1);
        let named = &receiver[start..];
        !named.is_empty()
            && field.is_none_or(|field| named == field)
            && ends_with_word(&receiver[..start], &dotted)
    })
}

/// Whether `body` calls the function `name` by its bare name or through a
/// module path: `name(` or `module::name(`, and not a method `.name(` or an
/// associated function `Type::name(`.
fn calls_function(body: &str, name: &str) -> bool {
    let call = format!("{name}(");
    body.match_indices(&call).any(|(at, _)| {
        let before = &body[..at];
        if before.ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '.') {
            return false;
        }
        let Some(path) = before.strip_suffix("::") else {
            return true;
        };
        let segment = path
            .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
            .map_or(path, |i| &path[i + 1..]);
        segment.starts_with(|c: char| c.is_lowercase())
    })
}

/// The files under `src` compiled only into tests: each module a
/// `#[cfg(test)]` declaration names, and every module those declare.
fn test_only(src: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    loop {
        let before = found.len();
        for file in files {
            let Ok(text) = std::fs::read_to_string(file) else {
                continue;
            };
            let inherited = found.contains(file);
            for (module, gated) in declared_modules(src, file, &text) {
                if (gated || inherited) && !found.contains(&module) {
                    found.push(module);
                }
            }
        }
        if found.len() == before {
            return found;
        }
    }
}

/// The files of the modules `text`, the source of `file`, declares with
/// `mod name;`, each with whether `#[cfg(test)]` gates the declaration. A
/// `#[path]` attribute is read relative to `file`'s directory.
fn declared_modules(src: &Path, file: &Path, text: &str) -> Vec<(PathBuf, bool)> {
    let dir = file.parent().unwrap_or(src);
    let stem = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    let own = if matches!(stem, "mod" | "lib" | "main") {
        dir.to_path_buf()
    } else {
        dir.join(stem)
    };
    let (mut gated, mut path) = (false, None::<String>);
    let mut found = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if let Some(attribute) = line.strip_prefix("#[") {
            let attribute = squash(attribute);
            gated |= attribute == "cfg(test)]";
            if let Some(named) = attribute.strip_prefix("path=\"") {
                path = named.strip_suffix("\"]").map(str::to_string);
            }
            continue;
        }
        let declared = line
            .split_once("mod ")
            .filter(|(visibility, _)| visibility.is_empty() || visibility.starts_with("pub"))
            .and_then(|(_, rest)| rest.strip_suffix(';'));
        if let Some(name) = declared {
            let module = match path.take() {
                Some(path) => dir.join(path),
                None if own.join(format!("{name}.rs")).exists() => own.join(format!("{name}.rs")),
                None => own.join(name).join("mod.rs"),
            };
            found.push((module, gated));
        }
        (gated, path) = (false, None);
    }
    found
}

/// Whether `body` calls `name` on a parameter that `params` declares as the
/// project host.
fn calls_on_host_param(params: &str, body: &str, name: &str) -> bool {
    params
        .split(',')
        .filter_map(|param| param.split_once(':'))
        .filter(|(_, kind)| kind.contains("ProjectHost"))
        .any(|(param, _)| {
            let call = format!("{param}.{name}(");
            body.match_indices(&call)
                .any(|(at, _)| ends_with_word(&body[..at + param.len()], param))
        })
}

/// The project host's `&self` methods that lock the engine, directly or
/// through one another, read from its source.
fn host_locks() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(PROJECT_HOST);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("the project host's source at {path:?}: {err}"));
    let methods = readers(PROJECT_HOST, &text);
    let mut locking: Vec<String> = methods
        .iter()
        .filter(|method| LOCKS.iter().any(|lock| method.body.contains(lock)))
        .map(|method| method.name.clone())
        .collect();
    loop {
        let grown: Vec<String> = methods
            .iter()
            .filter(|method| {
                !locking.contains(&method.name)
                    && locking
                        .iter()
                        .any(|name| calls_on(&method.body, "self", name, None))
            })
            .map(|method| method.name.clone())
            .collect();
        if grown.is_empty() {
            return locking;
        }
        locking.extend(grown);
    }
}

/// The readers in `all` that take the session: those that lock where they say
/// so, call a method of the project host in `host` on their receiver's `host`
/// or on a `&ProjectHost` parameter, or call a reader that takes it. Iterated
/// to a fixed point, so a lock three clean calls deep is still a lock.
fn locking(all: &[Reader], host: &[String]) -> Vec<usize> {
    let mut locking: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, reader)| {
            LOCKS.iter().any(|lock| reader.body.contains(lock))
                || host.iter().any(|name| {
                    calls_on(&reader.body, &reader.receiver, name, Some("host"))
                        || calls_on_host_param(&reader.params, &reader.body, name)
                })
        })
        .map(|(index, _)| index)
        .collect();
    loop {
        let grown: Vec<usize> = all
            .iter()
            .enumerate()
            .filter(|(index, reader)| {
                !locking.contains(index)
                    && locking.iter().any(|held| {
                        let held = &all[*held];
                        if held.receiver == "self" {
                            calls_on(&reader.body, &reader.receiver, &held.name, None)
                        } else {
                            calls_function(&reader.body, &held.name)
                        }
                    })
            })
            .map(|(index, _)| index)
            .collect();
        if grown.is_empty() {
            return locking;
        }
        locking.extend(grown);
    }
}

/// **A frame takes no session lock, and every exception is declared.**
///
/// Derived from the sources: the readers are found by parsing, the offenders are
/// whatever locks without a row, and a row that no longer locks fails too. The
/// closure over calls is what makes it a rule about the class rather than about
/// the eleven methods that happened to lock when it was written — the export
/// sheet's size row reached the session through two clean-looking calls, and a
/// scan that only looked for the literal lock would have called it innocent.
#[test]
fn engine_lock_census() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);
    files.sort();
    assert!(
        files.len() > 200,
        "found {} files under {src:?}, which cannot be this crate: the rule just went vacuous",
        files.len()
    );

    // The scan has to see a wrapped call and name the line the method starts on,
    // leave `&mut self` and `&mut AppState` alone, follow a call through a field
    // of `self` or of a `&AppState` parameter in any position, follow a call of
    // a function through its module path, see the project host's locks on
    // `self.host` and on a host parameter, and tell the project host from
    // another value named `host`.
    {
        let sample = r#"
impl S {
    fn a(&self) -> u32 {
        let e = self
            .engine()
            .lock();
        e.n()
    }
    fn b(&mut self) {
        self.engine.lock();
    }
    fn c(&self) -> u32 {
        self.a()
    }
    fn d(&self) -> u32 {
        self.inner.c()
    }
    fn e(&self, host: &ProjectHost) -> u64 {
        host.revision()
    }
    fn f(&self) -> u64 {
        self.host().snapshot().n
    }
    fn g(&self) -> Option<u64> {
        self.menus.as_ref().map(|host| host.revision())
    }
}
fn h(id: &str, state: &AppState) -> u32 {
    state.inner.c()
}
fn i(state: &AppState) -> u32 {
    super::guards::h("row", state)
}
fn j(state: &mut AppState) {
    state.engine.lock();
}
fn k(state: &AppState) -> u32 {
    Menu::h("row", state) + state.len()
}
fn l(state: &crate::app::AppState) -> u32 {
    state.engine().lock().n()
}
"#;
        let found = readers("sample.rs", sample);
        let names: Vec<&str> = found.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["a", "c", "d", "e", "f", "g", "h", "i", "k", "l"],
            "the scan misread its own sample"
        );
        assert_eq!(found[0].line, 3);
        let host = ["revision".to_owned(), "snapshot".to_owned()];
        let mut locked: Vec<&str> = locking(&found, &host)
            .into_iter()
            .map(|index| found[index].name.as_str())
            .collect();
        locked.sort_unstable();
        assert_eq!(
            locked,
            vec!["a", "c", "d", "e", "f", "h", "i", "l"],
            "the scan misjudged its own sample: `g` reads a menu host and `k` calls an \
             associated function, which the scan does not resolve; every other reader takes \
             the session"
        );
    }

    let tests = test_only(&src, &files);
    assert!(
        tests
            .iter()
            .any(|file| file.ends_with("app/drawn_census_fixtures.rs")),
        "the `#[cfg(test)]` modules found under {src:?} miss the dressed-state fixtures, so \
         the parse is broken and test helpers count as frames: {tests:?}"
    );
    let mut all: Vec<Reader> = Vec::new();
    for file in files.iter().filter(|file| !tests.contains(file)) {
        let rel = file
            .strip_prefix(&src)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        // Tests may hold the session for as long as they like, and this file
        // spells the calls it refuses.
        if rel.ends_with("_tests.rs") || rel == "app/engine_lock_tests.rs" {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        all.extend(readers(&rel, &text));
    }
    assert!(
        all.len() > 300,
        "only {} readers found across {} files; the parse is broken and the rule proves \
         nothing",
        all.len(),
        files.len()
    );

    let host = host_locks();
    assert!(
        ["snapshot", "revision"]
            .iter()
            .all(|name| host.iter().any(|lock| lock == name)),
        "the project host's snapshot and revision read the engine under its lock, and the \
         scan of {PROJECT_HOST} found only {host:?}: the parse is broken"
    );
    let locking = locking(&all, &host);

    let declared = |file: &str, name: &str| {
        DECLARED
            .iter()
            .any(|(row_file, row_name, _)| *row_file == file && *row_name == name)
    };
    let mut offenders: Vec<String> = locking
        .iter()
        .map(|index| &all[*index])
        .filter(|reader| !declared(&reader.file, &reader.name))
        .map(|reader| format!("{}:{} {}()", reader.file, reader.line, reader.name))
        .collect();
    offenders.sort();
    offenders.dedup();
    assert!(
        offenders.is_empty(),
        "these readers take the session lock, so a frame that reads one parks behind \
         whatever the preview decode is doing. Project the value instead — see \
         crate::app::project_facts for a session fact, crate::app::selected_detail for the \
         selected clip, crate::app::project_report for a requested count, or \
         add a row to DECLARED in this file saying why it is not a frame:\n  {}",
        offenders.join("\n  ")
    );

    let mut stale: Vec<String> = DECLARED
        .iter()
        .filter(|(file, name, _)| {
            !locking
                .iter()
                .any(|index| all[*index].file == *file && all[*index].name == *name)
        })
        .map(|(file, name, _)| format!("{file} {name}()"))
        .collect();
    stale.sort();
    assert!(
        stale.is_empty(),
        "these rows no longer take the session, so they are a fence around nothing — delete \
         them:\n  {}",
        stale.join("\n  ")
    );
    for (file, name, reason) in DECLARED {
        assert!(
            reason.len() > 20,
            "{file} {name}() is declared without saying why it is not a frame"
        );
    }
}
