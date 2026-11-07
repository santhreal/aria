//! The window's own frame resizes from every free side and moves no content.
//!
//! The class closed here: a side the window is not tiled against with no band
//! to resize it from, a band on a tiled side, a point two bands claim, a band
//! that resizes a side far from where it lies, a press on a band that also
//! reaches the content under it, and a frame that moves the content off the
//! window's origin, which would put every menu the shell opens at a pointer
//! position beside the pointer. Every one of the sixteen tilings is
//! enumerated, at sample points along each side, through each corner, and in
//! the middle of the window.
//!
//! Not covered: the platform's response to a resize or move request, which a
//! test window does not implement, and the pixels of the hairline.

use gpui::{
    Bounds, Context, InteractiveElement, IntoElement, Modifiers, MouseButton, Pixels, ResizeEdge,
    Size, Styled, TestAppContext, Tiling, Window, div, point, px, size,
};

use super::{BAND, CORNER, Frame, Handle, WindowTitle, framed, handles, resize_cursor};
use crate::theme::Theme;

/// Every tiling: bit 0 top, 1 right, 2 bottom, 3 left.
fn tilings() -> impl Iterator<Item = Tiling> {
    (0u8..16).map(|bits| Tiling {
        top: bits & 1 != 0,
        right: bits & 2 != 0,
        bottom: bits & 4 != 0,
        left: bits & 8 != 0,
    })
}

/// The sides an edge or a corner resizes, as `[top, right, bottom, left]`.
fn sides(edge: ResizeEdge) -> [bool; 4] {
    match edge {
        ResizeEdge::Top => [true, false, false, false],
        ResizeEdge::TopRight => [true, true, false, false],
        ResizeEdge::Right => [false, true, false, false],
        ResizeEdge::BottomRight => [false, true, true, false],
        ResizeEdge::Bottom => [false, false, true, false],
        ResizeEdge::BottomLeft => [false, false, true, true],
        ResizeEdge::Left => [false, false, false, true],
        ResizeEdge::TopLeft => [true, false, false, true],
    }
}

/// How far `at` is from each side of `window`, as `[top, right, bottom, left]`.
fn reach(at: [f32; 2], window: Size<Pixels>) -> [f32; 4] {
    let (w, h) = (f32::from(window.width), f32::from(window.height));
    [at[1], w - at[0], h - at[1], at[0]]
}

/// Offsets from each end of an axis `len` long: inside a band, between a
/// band and a corner, past a corner, and the middle.
fn samples(len: f32) -> Vec<f32> {
    let near = [
        0.5,
        BAND - 0.5,
        BAND + 0.5,
        CORNER - 0.5,
        CORNER + 0.5,
        3.0 * CORNER,
    ];
    let mut out: Vec<f32> = near.iter().flat_map(|d| [*d, len - d]).collect();
    out.push(len / 2.0);
    out
}

const SIZES: [Size<Pixels>; 3] = [
    size(px(1440.), px(900.)),
    size(px(1024.), px(640.)),
    size(px(480.), px(320.)),
];

#[test]
fn every_free_side_resizes_from_its_edge_and_nothing_else_does() {
    for window in SIZES {
        for tiling in tilings() {
            let bands: Vec<Handle> = handles(window, tiling).into_iter().flatten().collect();
            let tiled = [tiling.top, tiling.right, tiling.bottom, tiling.left];
            for band in &bands {
                assert_eq!(resize_cursor(band.edge), cursor_for(band.edge));
            }
            for y in samples(f32::from(window.height)) {
                for x in samples(f32::from(window.width)) {
                    let at = point(px(x), px(y));
                    let on: Vec<&Handle> = bands
                        .iter()
                        .filter(|band| band.bounds.contains(&at))
                        .collect();
                    assert!(
                        on.len() <= 1,
                        "{tiling:?} at ({x}, {y}): two bands claim one point"
                    );
                    let near = reach([x, y], window);
                    // Every free side within a band's width of the point.
                    let wanted: Vec<usize> =
                        (0..4).filter(|&n| !tiled[n] && near[n] < BAND).collect();
                    match on.first() {
                        None => assert!(
                            wanted.is_empty(),
                            "{tiling:?} at ({x}, {y}): free sides {wanted:?} have no band here"
                        ),
                        Some(band) => {
                            let resized = sides(band.edge);
                            for n in wanted {
                                assert!(
                                    resized[n],
                                    "{tiling:?} at ({x}, {y}): the {:?} band does not resize side {n}",
                                    band.edge
                                );
                            }
                            for n in (0..4).filter(|&n| resized[n]) {
                                assert!(
                                    !tiled[n],
                                    "{tiling:?}: the {:?} band resizes tiled side {n}",
                                    band.edge
                                );
                                assert!(
                                    near[n] < CORNER,
                                    "{tiling:?} at ({x}, {y}): the {:?} band resizes side {n}, {} px away",
                                    band.edge,
                                    near[n]
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

fn cursor_for(edge: ResizeEdge) -> gpui::CursorStyle {
    let [top, right, bottom, left] = sides(edge);
    match (top || bottom, left || right) {
        (true, false) => gpui::CursorStyle::ResizeUpDown,
        (false, true) => gpui::CursorStyle::ResizeLeftRight,
        _ if (top && left) || (bottom && right) => gpui::CursorStyle::ResizeUpLeftDownRight,
        _ => gpui::CursorStyle::ResizeUpRightDownLeft,
    }
}

#[test]
fn only_a_frame_the_app_draws_has_window_controls() {
    assert!(!Frame::Platform.draws_controls());
    assert!(Frame::Caption.draws_controls());
    for tiling in tilings() {
        assert!(Frame::Client(tiling).draws_controls(), "{tiling:?}");
    }
}

#[test]
fn the_title_names_the_project_and_marks_unsaved_changes() {
    let cases = [
        (WindowTitle::Home, "Aria", false),
        (
            WindowTitle::Project {
                name: "untitled".into(),
                dirty: false,
            },
            "untitled - Aria",
            false,
        ),
        (
            WindowTitle::Project {
                name: "Trailer cut".into(),
                dirty: true,
            },
            "*Trailer cut - Aria",
            true,
        ),
        (WindowTitle::Panel("Info"), "Info - Aria", false),
    ];
    for (title, text, edited) in cases {
        assert_eq!(title.text(), text);
        assert_eq!(title.edited(), edited, "{text}");
    }
}

/// A window whose whole content is one probe that counts the presses it
/// receives, inside the frame under test.
struct Framed {
    frame: Frame,
    presses: usize,
}

impl gpui::Render for Framed {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let probe = div()
            .id("framed-probe")
            .size_full()
            .debug_selector(|| "framed-probe".into())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.presses += 1),
            )
            .into_any_element();
        framed(probe, self.frame, &theme, window)
    }
}

#[gpui::test]
fn the_content_fills_the_window_and_a_band_press_never_reaches_it(cx: &mut TestAppContext) {
    cx.update(|app| {
        Theme::install(
            crate::theme::Appearance::Dark,
            Default::default(),
            None,
            app,
        );
    });
    let frames = [Frame::Platform, Frame::Caption]
        .into_iter()
        .chain(tilings().map(Frame::Client));
    for frame in frames {
        let (view, visual) =
            crate::test_support::window_view(cx, move |_, _| Framed { frame, presses: 0 });
        visual.run_until_parked();
        let window = visual.update(|window, _| window.viewport_size());
        let probe = visual
            .debug_bounds("framed-probe")
            .unwrap_or_else(|| panic!("{frame:?}: the content was not drawn"));
        assert_eq!(
            probe,
            Bounds::new(point(px(0.), px(0.)), window),
            "{frame:?}: the frame moved or shrank the content"
        );
        let bands: Vec<Handle> = match frame {
            Frame::Client(tiling) => handles(window, tiling).into_iter().flatten().collect(),
            Frame::Platform | Frame::Caption => Vec::new(),
        };
        for band in &bands {
            visual.simulate_click(band.bounds.center(), Modifiers::none());
        }
        assert_eq!(
            visual.read(|app| view.read(app).presses),
            0,
            "{frame:?}: a press on a resize band reached the content"
        );
        let middle = point(window.width / 2., window.height / 2.);
        visual.simulate_click(middle, Modifiers::none());
        assert_eq!(
            visual.read(|app| view.read(app).presses),
            1,
            "{frame:?}: a press in the middle of the window missed the content"
        );
    }
}

/// WHY: Home drew a title bar of its own under a window manager's frame: a
/// second strip below the manager's title, holding nothing to drag and no
/// window buttons. A frame has the bar when the application draws the top
/// edge: its own window buttons, or macOS's traffic lights over a transparent
/// title bar. Every frame is matched, so a frame added later states its answer
/// here before it builds.
#[test]
fn only_a_frame_whose_top_edge_the_app_draws_has_a_title_bar() {
    let frames = [Frame::Platform, Frame::Caption]
        .into_iter()
        .chain(tilings().map(Frame::Client));
    for frame in frames {
        let expected = match frame {
            Frame::Platform => cfg!(target_os = "macos"),
            Frame::Caption | Frame::Client(_) => true,
        };
        assert_eq!(frame.has_title_bar(), expected, "{frame:?}");
    }
}

/// WHY: the shell is what reads [`Frame::has_title_bar`]; the rule above holds
/// nothing if Home draws its bar regardless. The test window reports server
/// decorations, so on Linux its frame is the window manager's and Home starts
/// at the window's top edge.
#[gpui::test]
fn home_draws_its_title_bar_only_on_a_frame_that_has_one(cx: &mut TestAppContext) {
    let (state, visual) = crate::test_support::drawn_shell(cx, "home-title-bar");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.go_home();
            cx.notify();
        })
    });
    visual.run_until_parked();
    let frame = visual.update(|window, _| Frame::of(window));
    let body = visual
        .debug_bounds("home-body")
        .unwrap_or_else(|| panic!("{frame:?}: Home was not drawn"));
    let bar = visual.debug_bounds("home-titlebar");
    if frame.has_title_bar() {
        let bar = bar.unwrap_or_else(|| panic!("{frame:?}: Home drew no title bar"));
        assert_eq!(
            bar.origin.y,
            px(0.),
            "{frame:?}: the bar is off the top edge"
        );
        assert!(
            body.origin.y >= bar.origin.y + bar.size.height,
            "{frame:?}: Home's body at {body:?} runs under its bar at {bar:?}"
        );
    } else {
        assert_eq!(
            bar, None,
            "{frame:?}: Home drew a title bar under the platform's"
        );
        assert_eq!(
            body.origin.y,
            px(0.),
            "{frame:?}: Home's body leaves a strip above it with no bar in it"
        );
    }
    if cfg!(target_os = "linux") {
        assert_eq!(
            frame,
            Frame::Platform,
            "the test window reports server decorations, so the branch above that ran is the platform's"
        );
    }
}

/// WHY: a project saved while its internal `Project::name` was still `"untitled"`
/// (such as a session created with `Engine::new_session` and saved to `rich.aria`)
/// kept `"name": "untitled"` in the `.aria` JSON, and opening that file through
/// CLI `open_paths`, the Open dialog, Recents, Recovery, or drag-and-drop left
/// the window title as `"untitled - Aria"` instead of the opened project's stem.
/// This test enumerates every open path against the single [`AppState::window_title`]
/// choke point and asserts both the project name and the dirty/clean state.
#[test]
fn window_title_tracks_project_name_and_dirty_state_across_every_open_path() {
    #[derive(Debug, Clone, Copy)]
    enum OpenPath {
        CliStartupOpenPaths,
        OpenDialog,
        Recents,
        Recovery,
        DragDropHome,
        DragDropEditor,
        SaveAsFromUntitled,
    }

    const ALL_PATHS: [OpenPath; 7] = [
        OpenPath::CliStartupOpenPaths,
        OpenPath::OpenDialog,
        OpenPath::Recents,
        OpenPath::Recovery,
        OpenPath::DragDropHome,
        OpenPath::DragDropEditor,
        OpenPath::SaveAsFromUntitled,
    ];

    for (idx, path_kind) in ALL_PATHS.into_iter().enumerate() {
        let temp = crate::scratch_pool::scratch(&format!("window-title-open-paths-{idx}"));
        let data_dir = temp.join("data");
        std::fs::create_dir_all(&data_dir).expect("create data_dir");
        let project_file = temp.join("golden-cut.aria");

        // Write a .aria file whose internal JSON `"name"` is still `"untitled"`,
        // reproducing the exact on-disk shape of an untitled session saved to disk.
        aria_models::Project::new("untitled", aria_models::Rational::FPS_24)
            .save_to_file(&project_file)
            .expect("save untitled project fixture");

        let mut state =
            crate::app::AppState::new(aria_plugin::PluginContext::new(), data_dir.clone())
                .expect("init AppState");
        assert_eq!(state.window_title(), WindowTitle::Home, "{path_kind:?}");

        match path_kind {
            OpenPath::CliStartupOpenPaths => {
                // Exact entry point called by `aria_ui::run_app_with` for `config.open_paths`.
                state
                    .drop_on_home(vec![project_file.clone()])
                    .expect("CLI open_paths");
            }
            OpenPath::OpenDialog => {
                state
                    .open_project(project_file.clone())
                    .expect("open dialog");
            }
            OpenPath::Recents => {
                state.request_open_project(project_file.clone());
            }
            OpenPath::Recovery => {
                state.project_path = Some(project_file.clone());
                state.write_recovery().expect("write recovery copy");
                state =
                    crate::app::AppState::new(aria_plugin::PluginContext::new(), data_dir.clone())
                        .expect("relaunch AppState with pending recovery");
                state.restore_recovery().expect("restore recovery");
            }
            OpenPath::DragDropHome => {
                state.request_drop_on_home(vec![project_file.clone()]);
            }
            OpenPath::DragDropEditor => {
                state.new_project().expect("enter editor");
                state
                    .handle_dropped_paths(vec![project_file.clone()])
                    .expect("drop project in editor");
            }
            OpenPath::SaveAsFromUntitled => {
                state.new_project().expect("new untitled project");
                state.project_path = Some(project_file.clone());
                state.save_project().expect("save as golden-cut.aria");
            }
        }

        let expect_initially_dirty = matches!(path_kind, OpenPath::Recovery);
        let title = state.window_title();
        assert_eq!(
            title,
            WindowTitle::Project {
                name: "golden-cut".into(),
                dirty: expect_initially_dirty,
            },
            "{path_kind:?}: window title must adopt the file stem instead of staying untitled"
        );
        assert_eq!(
            title.text(),
            if expect_initially_dirty {
                "*golden-cut - Aria"
            } else {
                "golden-cut - Aria"
            },
            "{path_kind:?}"
        );

        // Mutate the project and verify the dirty indicator flips on, then save and verify it clears.
        state
            .add_track(aria_models::TrackKind::Video, "B-Roll")
            .expect("add track");
        assert_eq!(
            state.window_title().text(),
            "*golden-cut - Aria",
            "{path_kind:?}: dirty state must be reflected in window title"
        );

        state.save_project().expect("save project");
        assert_eq!(
            state.window_title().text(),
            "golden-cut - Aria",
            "{path_kind:?}: saving must clear the dirty marker while keeping project name"
        );
    }
}

/// WHY: a project saved as `p.aria` and copied to `rec.aria` (or saved via
/// `Save As` to a new file) used to display `"p"` in the window title,
/// breadcrumb, unsaved prompt, export default filename, template filename, and
/// project rename draft while showing `"rec"` in Recents, the recovery banner,
/// and the recovery status line. Closes that class by scanning every non-test
/// `.rs` file in `aria-ui` to enforce that no UI surface reads
/// `store.project_name` directly, enumerating every call site of the single
/// owner (`AppState::project_display_name` / `session::project_name_of`),
/// failing if a new call site is added without a surface reader here, and
/// asserting every surface agrees across an unsaved project, an opened copy
/// (`rec.aria` copied from `p.aria`), `Save As` to a new stem, and crash
/// recovery.
#[test]
fn every_project_name_surface_reads_one_owner_across_open_save_as_and_recovery() {
    use std::collections::BTreeSet;
    use std::path::Path;

    use crate::app::picker::Answer;
    use crate::test_support::collect_rs;
    use crate::test_support::control_sites::strip_comments;
    use crate::test_support::sources::without_test_items;
    use crate::types::RenameTarget;
    use aria_commands::{Command, EditCommand};

    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);
    assert!(files.len() > 200);

    let mut direct_store_reads = Vec::new();
    let mut owner_sites = BTreeSet::new();

    for file in &files {
        let rel = file
            .strip_prefix(&src)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with("test_support/") || rel.ends_with("_tests.rs") {
            continue;
        }
        let raw = std::fs::read_to_string(file).expect("read source file");
        let code = strip_comments(&without_test_items(&raw));
        for (line_idx, line) in code.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.contains("project_name")
                && !trimmed.contains("project_name_of(")
                && !matches!(
                    rel.as_str(),
                    "app/session.rs" | "projection.rs" | "app/engine_sync.rs" | "types_editor.rs"
                )
            {
                direct_store_reads.push(format!("{rel}:{}: {trimmed}", line_idx + 1));
            }
            if trimmed.contains("project_display_name(") || trimmed.contains("project_name_of(") {
                if rel == "app/session.rs"
                    && (trimmed.starts_with("pub(crate) fn project_name_of")
                        || trimmed.starts_with("pub fn project_display_name")
                        || trimmed.starts_with("project_name_of("))
                {
                    continue;
                }
                owner_sites.insert(rel.clone());
            }
        }
    }

    assert!(
        direct_store_reads.is_empty(),
        "UI surfaces must read AppState::project_display_name or session::project_name_of instead of store.project_name:\n  {}",
        direct_store_reads.join("\n  ")
    );

    let expected_sites: BTreeSet<String> = [
        "app/export_request.rs",
        "app/key_verbs.rs",
        "app/recents.rs",
        "app/recovery.rs",
        "app/rename.rs",
        "app/render_reach.rs",
        "app/session.rs",
        "app/templates.rs",
        "app/timeline_files.rs",
        "app/unsaved.rs",
        "chrome.rs",
        "panels/settings_dialog.rs",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(
        owner_sites, expected_sites,
        "the set of files calling project_display_name / project_name_of changed; register any new surface in this test"
    );

    type SurfaceReader = (&'static str, fn(&mut crate::app::AppState) -> String);
    let active_surfaces: &[SurfaceReader] = &[
        ("app/session.rs:window_title", |state| {
            match state.window_title() {
                WindowTitle::Project { name, .. } => name.to_string(),
                WindowTitle::Home | WindowTitle::Panel(_) => {
                    panic!("expected editor WindowTitle::Project")
                }
            }
        }),
        ("chrome.rs:breadcrumb", |state| {
            state.project_display_name().to_owned()
        }),
        ("app/unsaved.rs:unsaved_line", |state| {
            let line = state.unsaved_line();
            let start = line.find('“').expect("opening quote in unsaved_line") + '“'.len_utf8();
            let end = line[start..]
                .find('”')
                .expect("closing quote in unsaved_line")
                + start;
            line[start..end].to_owned()
        }),
        ("app/export_request.rs:export_destination", |state| {
            state
                .export_destination()
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("export stem")
                .to_owned()
        }),
        ("app/rename.rs:RenameTarget::Project", |state| {
            state.begin_rename(RenameTarget::Project);
            let draft = state.rename_draft.to_string();
            state.cancel_rename();
            draft
        }),
        ("app/templates.rs:save_template", |state| {
            let tpl = state.data_dir.join("probe-template.ariat");
            state
                .save_template(tpl.clone(), String::new())
                .expect("save template with default project name");
            aria_models::Template::load_from_file(&tpl)
                .expect("load saved template")
                .meta()
                .name
                .to_owned()
        }),
    ];

    let assert_active_surfaces = |state: &mut crate::app::AppState, expected: &str, stage: &str| {
        for (surface, read) in active_surfaces {
            assert_eq!(
                read(state),
                expected,
                "{stage}: surface {surface} disagreed on project name"
            );
        }
    };

    let temp = crate::scratch_pool::scratch("project-name-one-owner");
    let data_dir = temp.join("data");
    std::fs::create_dir_all(&data_dir).expect("create data_dir");
    let mut state = crate::app::AppState::new(aria_plugin::PluginContext::new(), data_dir.clone())
        .expect("init AppState");

    // 1. Before first save: stored name is used across all active surfaces.
    state.new_project().expect("new project");
    state
        .apply_command(Command::Edit(EditCommand::SetProjectName {
            name: "Rough Cut".into(),
        }))
        .expect("rename unsaved project");
    assert!(state.project_path.is_none());
    assert_active_surfaces(&mut state, "Rough Cut", "unsaved project before first save");

    // 2. Save as p.aria, copy p.aria to rec.aria on disk, and open rec.aria.
    let p_path = temp.join("p.aria");
    state.supply_pick(Answer::file(&p_path));
    state.save_or_pick();
    assert_active_surfaces(&mut state, "p", "after first save to p.aria");
    assert_eq!(state.recents()[0].label.as_ref(), "p");

    let rec_path = temp.join("rec.aria");
    std::fs::copy(&p_path, &rec_path).expect("copy p.aria to rec.aria");
    assert_eq!(
        aria_models::Project::load_from_file(&rec_path)
            .expect("raw load rec.aria")
            .name,
        "p",
        "copied file still carries old JSON name on disk before open"
    );
    state.open_project(rec_path.clone()).expect("open rec.aria");
    assert_active_surfaces(
        &mut state,
        "rec",
        "after opening rec.aria copied from p.aria",
    );
    assert_eq!(
        state.recents()[0].label.as_ref(),
        "rec",
        "app/recents.rs:visible_recents must report rec"
    );

    // 3. Save As from rec.aria to final-cut.aria adopts the new stem everywhere.
    let final_path = temp.join("final-cut.aria");
    state.supply_pick(Answer::file(&final_path));
    state.save_project_as();
    assert_active_surfaces(&mut state, "final-cut", "after Save As to final-cut.aria");
    assert_eq!(state.recents()[0].label.as_ref(), "final-cut");
    assert_eq!(
        aria_models::Project::load_from_file(&final_path)
            .expect("load final-cut.aria")
            .name,
        "final-cut"
    );

    // 4. Crash recovery of rec.aria (whose on-disk JSON still holds "name": "p").
    state.open_project(rec_path).expect("reopen rec.aria");
    state
        .add_track(aria_models::TrackKind::Video, "Recovery Track")
        .expect("dirty edit");
    state.write_recovery().expect("write recovery copy");
    // Simulate a recovery file written when internal Project.name was still "p".
    let mut raw_rec =
        aria_models::Project::load_from_file(&crate::app::recovery::recovery_path(&data_dir))
            .expect("load recovery file");
    raw_rec.name = "p".into();
    raw_rec
        .save_to_file(&crate::app::recovery::recovery_path(&data_dir))
        .expect("overwrite recovery file with stale internal name");

    let mut relaunched = crate::app::AppState::new(aria_plugin::PluginContext::new(), data_dir)
        .expect("relaunch AppState");
    let offer = relaunched
        .pending_recovery()
        .expect("pending recovery offer");
    assert_eq!(
        offer.name, "rec",
        "app/recovery.rs:load_pending_recovery banner must report rec"
    );
    relaunched.restore_recovery().expect("restore recovery");
    assert_eq!(
        relaunched.status_message.as_deref(),
        Some("Recovered unsaved work in rec. Save to keep it."),
        "app/recovery.rs:restore_recovery status line must report rec"
    );
    assert_active_surfaces(
        &mut relaunched,
        "rec",
        "after restoring recovery of rec.aria",
    );
}
