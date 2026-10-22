//! The shell, drawn.
//!
//! The class these close: every other test in this crate drives [`AppState`]
//! directly and never builds an element. A panel that panics on an empty
//! project, a `None` frame, a dialog with no selection, or a route it has never
//! been drawn in ships green under that arrangement — the state is right and
//! nobody ever asked the tree for a frame. These tests open a headless window,
//! put the real [`Shell`] in it, and draw: every panel of the route, every
//! dialog, before and after an edit, in both routes.
//!
//! What each test asserts is therefore two things at once: the state the shell
//! reports, and that producing a frame from that state does not panic. A layout
//! that divides by a zero lane height, an `unwrap` on a missing thumbnail, or an
//! element id built from an empty collection fails here and nowhere else.
//!
//! Not covered: what the frame looks like. Pixels are the compositor's tests;
//! this is about the tree existing.
//!
//! Pointers and keys are covered, in the cases named `interaction_*`. A key goes
//! in through the window's focus handle and comes out as projected state, so the
//! chord table, the route guard and the action all run. A pointer needs a
//! position, and a position needs geometry — so these drive the two surfaces
//! whose geometry the app itself records or owns: the timeline, which measures
//! where it was drawn, and the rail splitter, which is exactly as wide as the
//! rail beside it. What is still not covered is a click on anything sized by
//! text, because the test text system does not promise a real layout pass.
//!
//! A crate-root file rather than a `tests/` directory, wired through
//! `[[test]] path` in `Cargo.toml`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use aria_plugin::PluginContext;
use aria_ui::app::{AppState, DialogKind};
use aria_ui::shell::Shell;
use aria_ui::types::UiRoute;
// `App::new` is a trait method: building the state entity needs the trait in
// scope, nothing else from it.
use gpui::AppContext as _;

#[path = "src/test_support/window.rs"]
mod window;

/// A data directory of this test's own, at one fixed path per tag.
///
/// Every case needs its own: the shell writes settings and a recovery file, and
/// a case that inherited another's would be asserting about the previous case.
/// The tag is what makes it its own — a pid and a clock in the name made it a
/// new directory per *run* as well, which is how a temp root nothing empties
/// grows without bound. Emptied on entry, so a reused path carries nothing.
fn data_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aria-shell-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a data directory for the test");
    dir
}

/// The still every case imports. A real file, because import reads it.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// Clips anywhere in the sequence. Summed rather than indexed by lane: a drop
/// creates the lane it needs, so which lane ends up holding the clip is
/// placement's business and is asserted where placement is tested.
fn placed(state: &AppState) -> usize {
    state
        .store
        .sequence
        .tracks
        .iter()
        .map(|track| track.clips.len())
        .sum()
}

/// A headless window with the real shell in it.
///
/// Returns the state entity as well, because a case acts on the state and then
/// asks for another frame — which is the whole shape of these tests.
///
/// The theme is installed first because [`Theme::of`] reads a global and panics
/// without one: production installs it in `aria_ui::run` before it opens a
/// window, and a test that skips that step fails inside gpui with "no state of
/// type ThemeHandle exists" rather than in the panel that asked. Dark and the
/// default accent rather than the system appearance, so the tree a case draws
/// does not depend on the desktop the suite runs on.
///
/// The UI plugin is mounted, because it is the registry the shell reads: panels,
/// keys, commands, overlays and menus are all contributions, so a shell built
/// over a bare context draws a window with no panels in it and answers no key at
/// all. These cases used to do exactly that — they asserted that drawing an
/// empty frame did not panic. The rest of production's mount is not here on
/// purpose: it reads the real data directory for external plugins and
/// catalog overrides, and a test must not.
macro_rules! shell {
    ($cx:ident, $tag:expr) => {{
        let state = $cx.update(|app| {
            aria_ui::theme::Theme::install(
                aria_ui::theme::Appearance::Dark,
                Default::default(),
                None,
                app,
            );
            let ctx = PluginContext::new();
            ctx.mount(&aria_ui::plugins::UiPlugin)
                .expect("the ui plugin needs nothing");
            app.new(|_| AppState::new(ctx, data_dir($tag)).expect("editor state"))
        });
        let owned = gpui::Entity::clone(&state);
        let (_shell, visual) = window::window_view($cx, |_, cx| Shell::new(owned, cx));
        (state, visual)
    }};
}

/// Edit the state, and take the frame that edit asks for.
///
/// The shell repaints when the state notifies, which is how every listener in
/// production is written. A case that edited without notifying asserted its
/// state and never drew again: the tree it claimed to exercise was the one from
/// before the edit, and on the first case that was the empty Home route.
fn edit(
    state: &gpui::Entity<AppState>,
    visual: &mut gpui::VisualTestContext,
    body: impl FnOnce(&mut AppState),
) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            body(state);
            cx.notify();
        });
    });
    visual.run_until_parked();
}

/// A project with the fixture still on the timeline.
fn with_clip(state: &mut AppState) {
    state.new_project().expect("new project");
    let media = state.import_media(fixture()).expect("import");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place the clip");
}

/// Home draws, and starting a project draws the editor.
///
/// Two routes, one window: the shell keeps its panels across the switch, so a
/// panel that only works when it was built after a project exists fails here.
#[gpui::test]
fn home_draws_and_a_new_project_draws_the_editor(cx: &mut gpui::TestAppContext) {
    let (state, visual) = shell!(cx, "home");
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).route),
        UiRoute::Home,
        "a fresh session opens on Home"
    );

    edit(&state, visual, |state| {
        state.new_project().expect("new project");
    });
    assert_eq!(
        visual.read(|app| state.read(app).route),
        UiRoute::Editor,
        "starting a project moves to the editor"
    );
}

/// An import, a drop, and an undo each produce a frame.
///
/// The timeline is the panel with the most arithmetic per frame — pixels per
/// tick, lane heights, glides keyed per clip — and an empty sequence, a
/// one-clip sequence and an undone sequence are three different shapes of it.
#[gpui::test]
fn an_edit_and_its_undo_both_draw(cx: &mut gpui::TestAppContext) {
    let (state, visual) = shell!(cx, "edit");
    edit(&state, visual, with_clip);
    let clips = visual.read(|app| placed(state.read(app)));
    assert_eq!(clips, 1, "the clip is on the timeline");

    edit(&state, visual, |state| {
        assert!(state.undo(), "the drop is undoable");
    });
    let after = visual.read(|app| placed(state.read(app)));
    assert_eq!(after, 0, "undo took the clip back off");
}

/// Every dialog draws, over a project that has something in it.
///
/// Enumerated below rather than derived, because [`DialogKind`] has a payload
/// variant and a `None`. What makes the list complete is [`named`]: an
/// exhaustive match that stops compiling when a variant is added.
#[gpui::test]
fn every_dialog_draws_over_a_real_project(cx: &mut gpui::TestAppContext) {
    let (state, visual) = shell!(cx, "dialogs");
    edit(&state, visual, with_clip);

    assert_eq!(
        DIALOGS.len(),
        DIALOG_KINDS,
        "the list below must carry one row per DialogKind variant"
    );
    for kind in DIALOGS.iter().copied() {
        edit(&state, visual, |state| state.open_dialog(kind));
        assert_eq!(
            visual.read(|app| state.read(app).dialog),
            kind,
            "{} did not stay open long enough to be drawn",
            named(kind)
        );
    }
}

/// How many kinds there are. Paired with [`named`]: a new variant fails to
/// compile there, and this count then fails until the list below carries it.
const DIALOG_KINDS: usize = 15;

/// Every variant, named. Exhaustive on purpose — this match is what makes the
/// list a list of all of them rather than a list of the ones somebody thought
/// of.
fn named(kind: DialogKind) -> &'static str {
    match kind {
        DialogKind::None => "no dialog",
        DialogKind::Export => "Export",
        DialogKind::Settings => "Settings",
        DialogKind::Palette => "Palette",
        DialogKind::Jobs => "Jobs",
        DialogKind::ProjectReport => "Project report",
        DialogKind::UnsavedChanges => "Unsaved changes",
        DialogKind::InstallPlugin => "Install plugin",
        DialogKind::SpeedDuration => "Speed/Duration",
        DialogKind::AudioGain => "Audio Gain",
        DialogKind::About => "About",
        DialogKind::RemoveAttributes => "Remove Attributes",
        DialogKind::KeyframeVelocity => "Keyframe Velocity",
        DialogKind::MarkerEdit => "Edit Marker",
        DialogKind::Plugin(id) => id,
    }
}

/// The dialogs this crate ships. One row per [`DialogKind`] variant, including
/// the plugin payload, which is drawn with an id no plugin registered — the
/// overlay host must draw an empty surface rather than unwrap a lookup.
const DIALOGS: &[DialogKind] = &[
    DialogKind::None,
    DialogKind::Export,
    DialogKind::Settings,
    DialogKind::Palette,
    DialogKind::Jobs,
    DialogKind::ProjectReport,
    DialogKind::UnsavedChanges,
    DialogKind::InstallPlugin,
    DialogKind::SpeedDuration,
    DialogKind::AudioGain,
    DialogKind::About,
    DialogKind::RemoveAttributes,
    DialogKind::KeyframeVelocity,
    DialogKind::MarkerEdit,
    DialogKind::Plugin("aria.test.absent"),
];

/// Going back to Home with edits pending draws the prompt, not the editor.
///
/// The route and the dialog change in one step, and both panels are in the tree
/// while it happens: the prompt is drawn over the editor it is asking about.
#[gpui::test]
fn leaving_a_dirty_project_draws_the_prompt(cx: &mut gpui::TestAppContext) {
    let (state, visual) = shell!(cx, "home-again");
    edit(&state, visual, |state| {
        with_clip(state);
        state.request_go_home();
    });

    let (route, dialog) = visual.read(|app| {
        let read = state.read(app);
        (read.route, read.dialog)
    });
    assert_eq!(
        dialog,
        DialogKind::UnsavedChanges,
        "unsaved edits ask before the session is replaced"
    );
    assert_eq!(
        route,
        UiRoute::Editor,
        "the prompt is drawn over the editor"
    );
}

/// An editor with one clip on the timeline, drawn, with motion off.
///
/// Motion off for two reasons, both about geometry: a pane's drawn width eases
/// toward the committed one, so a frame caught mid-glide is not the width the
/// settings hold; and the boot cover is retired on its first frame instead of
/// fading, so nothing is over the tree while a pointer goes into it. Both are
/// production paths (reduced motion produces exactly this), and both are what
/// makes a position assertible.
macro_rules! editing {
    ($cx:ident, $tag:expr) => {{
        let (state, visual) = shell!($cx, $tag);
        visual.update(|_, app| app.set_reduce_motion(true));
        edit(&state, visual, with_clip);
        (state, visual)
    }};
}

/// How long the helper thread keeps the session. Long enough that a frame which
/// waits for it cannot come in under the bound, short enough that a regression
/// costs a second and a half rather than the suite.
const HOLD: Duration = Duration::from_millis(1500);

/// A frame that waited for the session would be at least this slow. A frame that
/// does not is two orders of magnitude faster; the gap is what makes a debug
/// build on a loaded machine irrelevant to the verdict.
const BOUND: Duration = Duration::from_millis(400);

/// **Every panel draws while something else holds the session.**
///
/// The class this closes: the engine is one mutex and a preview decode holds it
/// for the whole decode, so any value a panel fetched by locking the session was
/// a frame that could park behind a seek. The editor read about fifteen of them
/// per repaint. `engine_lock_census` fences the source; this is the behaviour,
/// and it does not care how a lock was reached — through a `&self` method, a
/// closure in a registry, or a free function.
///
/// A regression fails as a **bound**, never as a hang: the holder releases
/// itself after [`HOLD`] whatever this thread does, so the worst case is a slow
/// red test. The holder is a plain thread that takes a mutex and waits on a
/// channel — it wakes no gpui task, which is what the test scheduler asserts
/// about.
///
/// Both dialogs that used to lock hardest are open across the measurement: the
/// project report, whose counts are taken on the way open, and the export sheet,
/// whose size row resolved a whole export twice per frame.
#[gpui::test]
fn a_frame_draws_while_the_session_is_held(cx: &mut gpui::TestAppContext) {
    let (state, visual) = editing!(cx, "held-session");
    // Opening a dialog is an edit path and may take the session; drawing it may
    // not. Both happen before the hold starts.
    edit(&state, visual, |state| {
        state.open_dialog(DialogKind::ProjectReport)
    });
    edit(&state, visual, |state| {
        state.open_dialog(DialogKind::Export)
    });

    let engine = visual.read(|app| Arc::clone(state.read(app).engine()));
    let watched = Arc::clone(&engine);
    let (taken, acquired) = mpsc::channel::<()>();
    let (finished, release) = mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let _held = engine.lock();
        taken.send(()).expect("the test is still listening");
        // Self-releasing. Nothing this thread does depends on the frame, so a
        // frame that never comes cannot wedge it.
        let _ = release.recv_timeout(HOLD);
    });
    acquired
        .recv_timeout(Duration::from_secs(5))
        .expect("the helper thread took the session");

    // One notify, one frame, measured. The body edits nothing: an edit would
    // re-project, and re-projecting is allowed to wait for the session.
    let started = Instant::now();
    edit(&state, visual, |_| {});
    let elapsed = started.elapsed();

    // The bound first. A frame that waited runs past HOLD, at which point the
    // holder has released and the guard below would blame the harness for what
    // the render path did.
    assert!(
        elapsed < BOUND,
        "a frame took {elapsed:?} with the session held, against a bound of {BOUND:?} and a \
         hold of {HOLD:?}. Something a panel draws is asking the engine — see \
         crates/aria-ui/src/app/engine_lock_tests.rs for where the value belongs"
    );
    assert!(
        watched.try_lock().is_none(),
        "the frame came in under the bound but the helper had already let go, so nothing \
         was measured: raise HOLD"
    );

    drop(finished);
    holder.join().expect("the helper thread finished");
}

/// Records every asset path read and serves the app's own icons.
#[derive(Clone, Default)]
struct Loads(Arc<parking_lot::Mutex<Vec<String>>>);

impl gpui::AssetSource for Loads {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        self.0.lock().push(path.to_owned());
        gpui::AssetSource::load(&aria_ui::icons::Assets, path)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        gpui::AssetSource::list(&aria_ui::icons::Assets, path)
    }
}

/// The Home rail, the editor tool bar, and the Viewer's control row paint
/// their glyphs.
///
/// Closes the class of icon controls that draw a blank square: every icon in
/// the shell sits inside a control that sets the text colour, and gpui paints
/// an SVG only in a colour of its own. A glyph is read through the asset
/// source only when it paints, so each path read is a glyph in the frame. Not
/// covered: the colour value, and icons on surfaces this case does not open.
#[test]
fn home_and_editor_icon_controls_paint_their_glyphs() {
    use aria_ui::icons;

    let loads = Loads::default();
    let mut app = gpui::TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem),
        Arc::new(loads.clone()),
    );
    let state = app.update(|app| {
        aria_ui::theme::Theme::install(
            aria_ui::theme::Appearance::Dark,
            Default::default(),
            None,
            app,
        );
        let ctx = PluginContext::new();
        ctx.mount(&aria_ui::plugins::UiPlugin)
            .expect("the ui plugin needs nothing");
        app.new(|_| AppState::new(ctx, data_dir("icons")).expect("editor state"))
    });
    let owned = gpui::Entity::clone(&state);
    let mut window = app.open_window(|_, cx| Shell::new(owned, cx));
    let painted = |loads: &Loads, route: &str, paths: &[&str]| {
        let read = loads.0.lock();
        for path in paths {
            assert!(
                read.iter().any(|seen| seen == path),
                "{route}: {path} was never painted"
            );
        }
    };

    window.draw();
    painted(
        &loads,
        "home rail",
        &[
            icons::PLUS,
            icons::FOLDER_OPEN,
            icons::CLAPPERBOARD,
            icons::IMAGE,
            icons::SPARKLES,
        ],
    );

    app.update(|app| {
        state.update(app, |state, cx| {
            state.new_project().expect("new project");
            cx.notify();
        });
    });
    window.draw();
    painted(
        &loads,
        "editor tool bar and Viewer row",
        &[
            icons::SKIP_BACK,
            icons::CHEVRON_LEFT,
            icons::PLAY,
            icons::CHEVRON_RIGHT,
            icons::SKIP_FORWARD,
            icons::MAGNET,
            icons::LINK,
            icons::WRENCH,
            icons::UPLOAD,
        ],
    );

    // The app drops at the end of the case, and gpui's leak detector fails it
    // for any entity handle still alive after `App::shutdown`.
    drop(state);
    drop(window);
    app.update(|app| app.shutdown());
}

#[path = "tests_shell_dock.rs"]
mod dock;
#[path = "tests_shell_interaction.rs"]
mod interaction;
