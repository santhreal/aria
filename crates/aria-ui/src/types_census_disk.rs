//! No draw path reads the disk or spawns a process per frame.
//!
//! The class closed here: a `Render::render` pass, a panel body, a block
//! builder, or a menu row closure reading a file, walking a directory, or
//! running a config loader on every frame.
//!
//! `no_draw_reads_the_disk_or_spawns_a_process` scans every draw file in
//! [`super::draw_sources`] and builds the call graph of `&self` methods on
//! [`AppState`](crate::app::AppState) across `app/*.rs`, failing when a draw
//! file calls a filesystem or process primitive directly or calls an `&self`
//! method on `AppState` whose body reaches one. The companion tests verify the
//! held-state lifecycle for each surface moved off the per-frame disk path.
//!
//! Not covered: disk reads inside `&mut self` action handlers invoked on a
//! button press or file-picker answer, and dynamic dispatch through trait
//! objects outside `app/`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use aria_settings::AiSource;
use aria_transcription::WhisperModel;
use gpui::VisualTestContext;

use crate::app::{AppState, DialogKind};
use crate::chrome::menu_file::IMPORT_RECENT_PREFIX;
use crate::chrome::menu_items::items_of;
use crate::test_support::{collect_rs, drawn_shell, sources::without_test_items};
use crate::types::{ActivePanel, AppMenu, SidebarTab};

/// Non-draw persistence and loader modules outside `app/`.
const LOADER_MODULES: &[&str] = &[
    "appearance.rs",
    "keys/keymap_sets.rs",
    "layouts.rs",
    "lib.rs",
    "plugins/gpu.rs",
    "plugins/host.rs",
    "plugins/installed.rs",
    "plugins/services.rs",
    "scratch_pool.rs",
    "settings.rs",
    "theme/file.rs",
    "theme_file.rs",
    "visual_diff.rs",
];

/// Minimum number of `&self` methods on `AppState` expected in `app/`.
const APP_READER_FLOOR: usize = 150;

/// Filesystem, config-loader, and process primitives forbidden on a draw path.
const DISK_PRIMITIVES: &[&str] = &[
    "std::fs::",
    "fs::read",
    "fs::write",
    "fs::metadata",
    "fs::read_dir",
    "fs::remove_",
    "fs::create_dir",
    "fs::copy",
    "fs::rename",
    ".exists()",
    ".is_file()",
    ".is_dir()",
    ".metadata()",
    "File::open",
    "File::create",
    concat!("Command", "::new"),
    "aria_settings::load(",
    "schema_report::report(",
    "UiSettings::load(",
    "load_agent_dir(",
    "ModelManager::new(",
    ".size_bytes()",
];

struct ParsedFn {
    file: String,
    line: usize,
    name: String,
    is_self_ref: bool,
    body: String,
}

fn strip_line_comments(text: &str) -> String {
    text.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                ""
            } else {
                line.split("//").next().unwrap_or(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_app_functions() -> Vec<ParsedFn> {
    let app_dir = super::src().join("app");
    let mut files = Vec::new();
    collect_rs(&app_dir, &mut files);
    let mut out = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(super::src())
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.ends_with("_tests.rs") || rel.contains("_tests_") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&file) else {
            continue;
        };
        let cleaned = strip_line_comments(&without_test_items(&raw));
        let lines: Vec<&str> = cleaned.lines().collect();
        let mut i = 0usize;
        while i < lines.len() {
            let trimmed = lines[i].trim();
            if let Some(name) = fn_name_in_decl(trimmed) {
                let start_line = i + 1;
                let mut sig = String::new();
                let mut depth = 0i32;
                let mut saw_open = false;
                let mut body = String::new();
                while i < lines.len() {
                    let l = lines[i];
                    if !saw_open {
                        sig.push_str(l);
                        sig.push('\n');
                    }
                    for ch in l.chars() {
                        if ch == '{' {
                            saw_open = true;
                            depth += 1;
                        } else if ch == '}' {
                            depth -= 1;
                        }
                    }
                    if saw_open {
                        body.push_str(l);
                        body.push('\n');
                        if depth <= 0 {
                            break;
                        }
                    }
                    i += 1;
                }
                let is_self_ref = sig.contains("&self");
                let is_assoc_ctor =
                    !is_self_ref && matches!(name.as_str(), "new" | "from" | "default" | "of");
                if !sig.contains("&mut self") && !is_assoc_ctor {
                    out.push(ParsedFn {
                        file: rel.clone(),
                        line: start_line,
                        name,
                        is_self_ref,
                        body,
                    });
                }
            }
            i += 1;
        }
    }
    out
}

fn fn_name_in_decl(trimmed: &str) -> Option<String> {
    let rest = ["pub(crate) fn ", "pub(super) fn ", "pub fn ", "fn "]
        .into_iter()
        .find_map(|prefix| trimmed.strip_prefix(prefix))?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

fn direct_primitive(text: &str) -> Option<&'static str> {
    for &pat in DISK_PRIMITIVES {
        if text.contains(pat) {
            return Some(pat);
        }
    }
    if text.contains("load(&path)") || text.contains("load(&self.config_path())") {
        return Some("aria_settings::load(");
    }
    None
}

fn contains_call(haystack: &str, prefix: &str) -> bool {
    let mut start = 0usize;
    while let Some(pos) = haystack[start..].find(prefix) {
        let abs = start + pos;
        let prev = haystack[..abs].chars().next_back();
        let boundary =
            prev.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_' && c != ':' && c != '.');
        if boundary {
            return true;
        }
        start = abs + prefix.len();
    }
    false
}

/// Every draw module and every `&self` method on `AppState` called from a draw
/// module is free of per-frame disk and process calls.
#[test]
fn no_draw_reads_the_disk_or_spawns_a_process() {
    let sources = super::draw_sources();
    assert!(
        sources.len() >= super::DRAW_FLOOR,
        "found {} draw files, fewer than {}",
        sources.len(),
        super::DRAW_FLOOR
    );

    let fns = parse_app_functions();
    let reader_count = fns.iter().filter(|f| f.is_self_ref).count();
    assert!(
        reader_count >= APP_READER_FLOOR,
        "found {reader_count} &self methods in app/, fewer than {APP_READER_FLOOR}"
    );

    let mut tainted_self: BTreeMap<String, String> = BTreeMap::new();
    let mut tainted_free: BTreeMap<(String, String), String> = BTreeMap::new();
    for f in &fns {
        if let Some(prim) = direct_primitive(&f.body) {
            let reason = format!("{}:{} ({prim})", f.file, f.line);
            if f.is_self_ref {
                tainted_self.entry(f.name.clone()).or_insert(reason);
            } else {
                tainted_free
                    .entry((f.file.clone(), f.name.clone()))
                    .or_insert(reason);
            }
        }
    }

    loop {
        let mut changed = false;
        for f in &fns {
            if f.is_self_ref && tainted_self.contains_key(&f.name) {
                continue;
            }
            if !f.is_self_ref && tainted_free.contains_key(&(f.file.clone(), f.name.clone())) {
                continue;
            }
            let mut found = None;
            for (callee, why) in &tainted_self {
                let pat = format!("self.{callee}(");
                if f.body.contains(&pat) {
                    found = Some(format!("{}:{} -> {callee} -> {why}", f.file, f.line));
                    break;
                }
            }
            if found.is_none() {
                for ((file, callee), why) in &tainted_free {
                    let pat = format!("{callee}(");
                    if f.file == *file && contains_call(&f.body, &pat) {
                        found = Some(format!("{}:{} -> {callee} -> {why}", f.file, f.line));
                        break;
                    }
                }
            }
            if let Some(reason) = found {
                if f.is_self_ref {
                    tainted_self.insert(f.name.clone(), reason);
                } else {
                    tainted_free.insert((f.file.clone(), f.name.clone()), reason);
                }
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut offenders = Vec::new();
    for (name, raw) in &sources {
        if LOADER_MODULES.contains(&name.as_str()) {
            continue;
        }
        let cleaned = strip_line_comments(raw);
        for (idx, line) in cleaned.lines().enumerate() {
            let code = line.trim();
            if let Some(prim) = direct_primitive(code) {
                offenders.push(format!("{name}:{}: direct {prim}: {code}", idx + 1));
            }
            for (callee, why) in &tainted_self {
                let dot_call = format!(".{callee}(");
                if code.contains(&dot_call) {
                    offenders.push(format!("{name}:{}: calls {callee} ({why})", idx + 1));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "{} draw sites reach a disk or process call per frame:\n  {}",
        offenders.len(),
        offenders.join("\n  ")
    );
}

#[test]
fn import_recent_check_reads_the_held_missing_set() {
    let (mut state, _) = crate::app::premiere_tests::with_clip();
    let file = state.data_dir.join("clip.png");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png"),
        &file,
    )
    .expect("copy fixture");
    state.import_paths(vec![file.clone()]);

    let row_id = format!("{IMPORT_RECENT_PREFIX}{}", file.display());
    let blocked = |state: &AppState| {
        items_of(AppMenu::File, state)
            .into_iter()
            .find(|item| item.id == row_id)
            .expect("import recent row")
            .blocked_by(state)
    };
    assert_eq!(blocked(&state), None);

    std::fs::remove_file(&file).expect("remove imported file behind state");
    assert_eq!(
        blocked(&state),
        None,
        "menu check must read the held missing set rather than statting the path per draw"
    );

    state.toggle_menu(AppMenu::File);
    assert_eq!(blocked(&state), Some("The file is missing"));

    std::fs::write(&file, b"restored").expect("restore file behind state");
    assert_eq!(
        blocked(&state),
        Some("The file is missing"),
        "held missing set stays until the File menu opens again"
    );
    state.close_menu();
    state.toggle_menu(AppMenu::File);
    assert_eq!(blocked(&state), None);

    std::fs::remove_file(&file).expect("remove again");
    state.import_recent(&file);
    assert_eq!(blocked(&state), Some("The file is missing"));
}

fn pump(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
}

#[gpui::test]
fn ai_composer_draws_the_slash_commands_the_state_holds(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "held-slash");
    let commands_dir = visual.update(|_, app| state.read(app).agent_dir().join("commands"));
    std::fs::create_dir_all(&commands_dir).expect("create commands dir");
    let review = commands_dir.join("review.md");
    std::fs::write(
        &review,
        "---\ndescription: Review cut\n---\nReview $ARGUMENTS",
    )
    .unwrap();

    visual.update(|_, app| {
        state.update(app, |s, cx| {
            s.focus_pane(ActivePanel::Ai);
            s.assistant.prompt.set("/rev");
            cx.notify();
        });
    });
    pump(visual);

    assert_eq!(
        visual.update(|_, app| state.read(app).assistant_slash_commands()),
        vec!["review".to_string()]
    );
    assert_eq!(
        visual
            .update(|_, app| state.read(app).assistant_slash_hint())
            .as_deref(),
        Some("A slash command expands before it is sent: /review")
    );

    let polish = commands_dir.join("polish.md");
    std::fs::write(&polish, "---\ndescription: Polish\n---\nPolish $ARGUMENTS").unwrap();
    std::fs::remove_file(&review).unwrap();
    pump(visual);
    assert_eq!(
        visual.update(|_, app| state.read(app).assistant_slash_commands()),
        vec!["review".to_string()],
        "drawing the composer must not re-read commands/*.md from disk"
    );

    visual.update(|_, app| {
        state.update(app, |s, cx| {
            s.focus_pane(ActivePanel::Ai);
            cx.notify();
        });
    });
    pump(visual);
    assert_eq!(
        visual.update(|_, app| state.read(app).assistant_slash_commands()),
        vec!["polish".to_string()]
    );
}

#[gpui::test]
fn settings_dialog_draws_the_held_cloud_settings_and_schema(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "held-cloud");
    pump(visual);

    let config_path = visual.update(|_, app| state.read(app).config_path());
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        "[ai]\nsource = \"local\"\nbase_url = \"http://localhost:11434/v1\"\nmodel = \"held-local\"\n\n[storage]\nroot = \"relative/path\"\n",
    )
    .unwrap();

    assert!(
        visual
            .update(|_, app| state.read(app).cloud_settings().ai.model)
            .is_empty(),
        "cloud_settings must return the held config until refreshed"
    );
    assert!(
        visual
            .update(|_, app| state.read(app).config_refusals())
            .is_empty()
    );

    visual.update(|_, app| {
        state.update(app, |s, cx| {
            s.open_dialog(DialogKind::Settings);
            cx.notify();
        });
    });
    pump(visual);

    assert_eq!(
        visual.update(|_, app| state.read(app).cloud_settings().ai.model),
        "held-local"
    );
    assert_eq!(
        visual
            .update(|_, app| state.read(app).config_refusals())
            .len(),
        1
    );
    assert!(
        visual
            .update(|_, app| state.read(app).settings_schema_status())
            .contains("all readable by this build")
    );

    std::fs::write(&config_path, "[cloud]\n\n[schema]\ncloud = 999\n").unwrap();
    assert!(
        visual
            .update(|_, app| state.read(app).settings_schema_status())
            .contains("all readable by this build"),
        "settings_schema_status must return the held report between refreshes"
    );

    visual.update(|_, app| {
        state.update(app, |s, cx| {
            s.open_dialog(DialogKind::Settings);
            cx.notify();
        });
    });
    pump(visual);
    assert_eq!(
        visual.update(|_, app| state.read(app).settings_schema_status()),
        "refused: cloud"
    );
    std::fs::write(&config_path, "").unwrap();
    visual.update(|_, app| {
        state.update(app, |s, _| {
            s.set_ai_source(AiSource::OpenRouter).unwrap();
        });
    });
    assert_eq!(
        visual.update(|_, app| state.read(app).cloud_settings().ai.source),
        AiSource::OpenRouter
    );
}

#[gpui::test]
fn storage_settings_draws_the_held_cache_sizes(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "held-caches");

    let (dl_root, an_root) = visual.update(|_, app| {
        let s = state.read(app);
        (s.download_cache_root(), s.analysis_cache_root())
    });
    std::fs::create_dir_all(&dl_root).unwrap();
    std::fs::create_dir_all(&an_root).unwrap();
    std::fs::write(dl_root.join("asset.bin"), vec![0u8; 4096]).unwrap();
    std::fs::write(an_root.join("moments.db"), vec![0u8; 2048]).unwrap();

    assert_eq!(
        visual.update(|_, app| state.read(app).download_cache_size_bytes()),
        0,
        "download_cache_size_bytes must return the held size between refreshes"
    );
    assert_eq!(
        visual.update(|_, app| state.read(app).analysis_cache_size_bytes()),
        0,
        "analysis_cache_size_bytes must return the held size between refreshes"
    );

    visual.update(|_, app| {
        state.update(app, |s, cx| {
            s.open_dialog(DialogKind::Settings);
            cx.notify();
        });
    });
    pump(visual);

    assert_eq!(
        visual.update(|_, app| state.read(app).download_cache_size_bytes()),
        4096
    );
    assert_eq!(
        visual.update(|_, app| state.read(app).analysis_cache_size_bytes()),
        2048
    );

    visual.update(|_, app| {
        state.update(app, |s, _| {
            s.clear_download_cache().unwrap();
            s.clear_analysis_cache().unwrap();
        });
    });
    assert_eq!(
        visual.update(|_, app| state.read(app).download_cache_size_bytes()),
        0
    );
    assert_eq!(
        visual.update(|_, app| state.read(app).analysis_cache_size_bytes()),
        0
    );
}

#[gpui::test]
fn transcription_and_captions_draw_the_held_models(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "held-models");

    let base_path = visual.update(|_, app| {
        state
            .read(app)
            .model_manager()
            .unwrap()
            .model_path(WhisperModel::BaseEn)
            .unwrap()
    });
    std::fs::create_dir_all(base_path.parent().unwrap()).unwrap();
    std::fs::write(&base_path, b"corrupt-stub").unwrap();

    let base_status = |visual: &mut VisualTestContext| {
        visual.update(|_, app| {
            state
                .read(app)
                .model_rows()
                .into_iter()
                .find(|row| row.model == WhisperModel::BaseEn)
                .unwrap()
                .status
        })
    };
    assert_eq!(
        base_status(visual),
        "Not installed",
        "model_rows must return the held rows rather than hashing model files per draw"
    );

    visual.update(|_, app| {
        state.update(app, |s, cx| {
            s.open_dialog(DialogKind::Settings);
            cx.notify();
        });
    });
    pump(visual);
    assert_eq!(base_status(visual), "Corrupt");

    std::fs::remove_file(&base_path).unwrap();
    assert_eq!(
        base_status(visual),
        "Corrupt",
        "removing a model file behind state keeps the held status until refreshed"
    );

    visual.update(|_, app| {
        state.update(app, |s, cx| {
            s.open_dialog(DialogKind::None);
            s.show_sidebar_tab(SidebarTab::Captions);
            cx.notify();
        });
    });
    pump(visual);
    assert_eq!(base_status(visual), "Not installed");
}
