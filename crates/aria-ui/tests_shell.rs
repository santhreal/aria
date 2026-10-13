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
use aria_ui::settings::{SIDEBAR_MAX, SIDEBAR_MIN};
use aria_ui::shell::Shell;
use aria_ui::transport::TIMELINE_HEADER_W;
use aria_ui::types::UiRoute;
// `App::new` is a trait method: building the state entity needs the trait in
// scope, nothing else from it.
use gpui::AppContext as _;
use gpui::{Modifiers, MouseButton, point, px};

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
/// purpose: it reads the operator's own data directory for external plugins and
/// catalog overrides, and a test must not.
macro_rules! shell {
    ($cx:ident, $tag:expr) => {{
        let state = $cx.update(|app| {
            aria_ui::theme::Theme::install(
                aria_ui::theme::Appearance::Dark,
                Default::default(),
                app,
            );
            let ctx = PluginContext::new();
            ctx.mount(&aria_ui::plugins::UiPlugin)
                .expect("the ui plugin needs nothing");
            app.new(|_| AppState::new(ctx, data_dir($tag)).expect("editor state"))
        });
        let owned = gpui::Entity::clone(&state);
        let (_shell, visual) = $cx.add_window_view(|_, cx| Shell::new(owned, cx));
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
const DIALOG_KINDS: usize = 9;

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
        DialogKind::History => "History",
        DialogKind::ProjectReport => "Project report",
        DialogKind::UnsavedChanges => "Unsaved changes",
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
    DialogKind::History,
    DialogKind::ProjectReport,
    DialogKind::UnsavedChanges,
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
/// production paths — the operator who asks for reduced motion gets exactly
/// this — and both are what makes a position assertible.
macro_rules! editing {
    ($cx:ident, $tag:expr) => {{
        let (state, visual) = shell!($cx, $tag);
        visual.update(|_, app| app.set_reduce_motion(true));
        edit(&state, visual, with_clip);
        (state, visual)
    }};
}

/// A click on the ruler puts the playhead on the tick under the pointer.
///
/// The class this closes: a pointer arrives in window coordinates and the
/// timeline is not at the window's left edge. Every conversion in the panel
/// subtracted the track-header column alone, which is only right when the panel
/// starts at x=0 — it never does, because the media rail and its splitter are to
/// its left. A click on the first frame of the sequence landed the rail's whole
/// width into it: at the default zoom, three seconds off. Nothing caught it
/// because no test had ever driven a pointer.
///
/// The position comes from the panel's own measurement rather than from a sum of
/// layout constants, so this asserts the conversion and not a copy of the
/// layout: whatever the rail is doing, the first pixel of content is tick zero.
#[gpui::test]
fn interaction_a_ruler_click_puts_the_playhead_under_the_pointer(cx: &mut gpui::TestAppContext) {
    let (state, visual) = editing!(cx, "ruler-click");

    let frame = visual
        .read(|app| state.read(app).timeline_frame)
        .expect("the timeline records where it was drawn");
    let [panel_x, panel_y, panel_w, panel_h] = frame;
    assert!(
        panel_w > 0.0 && panel_h > 0.0,
        "the panel was drawn with a size: {frame:?}"
    );
    assert!(
        panel_x >= SIDEBAR_MIN,
        "the timeline is right of the media rail, which is what the old \
         conversion assumed away: {frame:?}"
    );
    let content_left = panel_x + TIMELINE_HEADER_W;
    let fps = visual.read(|app| i64::from(state.read(app).store.sequence.frame_rate.num));

    // The ruler is the top strip of the panel. 80px along its content is one
    // second at the default zoom, whose pixels-per-second the transport owns.
    visual.simulate_click(
        point(px(content_left + 80.0), px(panel_y + 8.0)),
        Modifiers::none(),
    );
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).playhead.value),
        fps,
        "a click one second along the ruler is one second into the sequence"
    );

    // The first pixel of content is the start, not a tick the rail's width in.
    visual.simulate_click(
        point(px(content_left), px(panel_y + 8.0)),
        Modifiers::none(),
    );
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).playhead.value),
        0,
        "the first pixel of the ruler is tick zero"
    );

    // Over the track headers is before the start, and clamps there rather than
    // going negative.
    visual.simulate_click(
        point(px(panel_x + 8.0), px(panel_y + 8.0)),
        Modifiers::none(),
    );
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).playhead.value),
        0,
        "a pointer left of the content cannot move the playhead below zero"
    );
}

/// Dragging the rail's splitter resizes the rail, and Escape abandons the drag.
///
/// A real pointer gesture with three events: the press that starts it, the move
/// that carries it, and the release that commits it. The splitter's position is
/// the rail's own width, which is projected state — the one piece of chrome
/// whose geometry needs no measurement.
#[gpui::test]
fn interaction_a_splitter_drag_resizes_the_media_rail(cx: &mut gpui::TestAppContext) {
    let (state, visual) = editing!(cx, "splitter-drag");
    let start = visual.read(|app| state.read(app).ui_settings.sidebar_width);
    let y = visual
        .read(|app| state.read(app).timeline_frame)
        .map(|[_, y, _, h]| y + h * 0.5)
        .expect("the timeline records where it was drawn");
    // Two pixels into the four-pixel splitter, which begins where the rail ends.
    let grip = start + 2.0;

    visual.simulate_mouse_down(point(px(grip), px(y)), MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(
        point(px(grip + 60.0), px(y)),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.run_until_parked();
    let widened = visual.read(|app| state.read(app).ui_settings.sidebar_width);
    assert_eq!(
        widened,
        (start + 60.0).clamp(SIDEBAR_MIN, SIDEBAR_MAX),
        "the rail follows the pointer while the button is down"
    );

    visual.simulate_mouse_up(
        point(px(grip + 60.0), px(y)),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).ui_settings.sidebar_width),
        widened,
        "the release keeps the width the drag reached"
    );

    // A second drag, abandoned: Escape puts the rail back where the press found
    // it, which is the shell's own gesture cancel and not a binding.
    let before = widened;
    visual.simulate_mouse_down(
        point(px(before + 2.0), px(y)),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.simulate_mouse_move(
        point(px(before - 40.0), px(y)),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.run_until_parked();
    assert_ne!(
        visual.read(|app| state.read(app).ui_settings.sidebar_width),
        before,
        "the second drag moved the rail before it was abandoned"
    );
    visual.simulate_keystrokes("escape");
    assert_eq!(
        visual.read(|app| state.read(app).ui_settings.sidebar_width),
        before,
        "Escape returns the rail to the width the press found"
    );
}

/// Keys drive selection, trimming, markers, the snap, and the marks.
///
/// Every keystroke goes in through the window: the shell's focus handle takes
/// it, the chord table resolves it against the editor route, and the action runs
/// on the state. A chord that stopped resolving, a route guard that swallowed
/// it, or an action wired to the wrong verb all fail here.
#[gpui::test]
fn interaction_keys_select_trim_mark_and_snap(cx: &mut gpui::TestAppContext) {
    let (state, visual) = editing!(cx, "keys");
    let fps = visual.read(|app| i64::from(state.read(app).store.sequence.frame_rate.num));

    // Selection. Ctrl+A takes every unlocked clip.
    visual.simulate_keystrokes("ctrl-a");
    assert_eq!(
        visual.read(|app| state.read(app).selection.len()),
        1,
        "Ctrl+A selects the clip on the timeline"
    );

    // A step key belongs to the selection when there is one: Right nudges the
    // clip and leaves the playhead alone. That is the branch
    // `keys_binding_actions::step` takes first, and it is the one a test that
    // only ever watches the playhead cannot see.
    assert_eq!(clip_edges(&state, visual).0, 0, "the clip starts at zero");
    visual.simulate_keystrokes("right right right");
    assert_eq!(
        clip_edges(&state, visual).0,
        3,
        "three steps forward nudges the selected clip three frames"
    );
    assert_eq!(
        visual.read(|app| state.read(app).playhead.value),
        0,
        "nudging a clip does not move the playhead"
    );
    visual.simulate_keystrokes("left left left");
    assert_eq!(clip_edges(&state, visual).0, 0, "the nudge reverses");

    // With nothing selected the same key moves the playhead, and a marker lands
    // where the playhead is.
    edit(&state, visual, |state| state.clear_selection());
    visual.simulate_keystrokes("right right right");
    assert_eq!(
        visual.read(|app| state.read(app).playhead.value),
        3,
        "three steps forward is three frames"
    );
    visual.simulate_keystrokes("m");
    let marks = visual.read(|app| state.read(app).store.sequence.markers.clone());
    assert_eq!(
        marks.iter().map(|marker| marker.tick).collect::<Vec<_>>(),
        vec![3],
        "M adds a marker under the playhead"
    );

    // The snap: a marker near the playhead is moved onto it.
    visual.simulate_keystrokes("right right");
    visual.simulate_keystrokes("ctrl-alt-m");
    assert_eq!(
        visual.read(|app| state
            .read(app)
            .store
            .sequence
            .markers
            .iter()
            .map(|marker| marker.tick)
            .collect::<Vec<_>>()),
        vec![5],
        "Ctrl+Alt+M snaps the nearby marker onto the playhead"
    );

    // Marks. Plain sets, Alt clears, and the two are independent.
    visual.simulate_keystrokes("i");
    visual.simulate_keystrokes("shift-right shift-right");
    visual.simulate_keystrokes("o");
    let (mark_in, mark_out) = visual.read(|app| {
        let read = state.read(app);
        (read.mark_in, read.mark_out)
    });
    assert_eq!(mark_in, Some(5), "I marks in at the playhead");
    assert_eq!(mark_out, Some(25), "O marks out ten frames on");
    visual.simulate_keystrokes("alt-i");
    let (mark_in, mark_out) = visual.read(|app| {
        let read = state.read(app);
        (read.mark_in, read.mark_out)
    });
    assert_eq!(mark_in, None, "Alt+I clears In");
    assert_eq!(mark_out, Some(25), "clearing In leaves Out alone");

    // Trimming. Q pulls the head to the playhead, W pushes the tail to it, and
    // the clip keeps the length between them. Both act on a selection, so the
    // playhead is placed with nothing selected and the selection taken back
    // afterwards — a step key with a selection is a nudge, not a seek.
    visual.simulate_keystrokes("ctrl-a");
    let (head, tail) = clip_edges(&state, visual);
    assert_eq!(head, 0, "the clip was dropped at the start");
    visual.simulate_keystrokes("q");
    let (trimmed_head, trimmed_tail) = clip_edges(&state, visual);
    assert_eq!(trimmed_head, 25, "Q moves the head to the playhead");
    assert_eq!(trimmed_tail, tail, "Q leaves the tail alone");

    edit(&state, visual, |state| state.clear_selection());
    visual.simulate_keystrokes("end");
    visual.simulate_keystrokes("left left");
    let playhead = visual.read(|app| state.read(app).playhead.value);
    assert!(
        playhead > trimmed_head && playhead < trimmed_tail,
        "the playhead has to land inside the clip for W to have anything to do: \
         {playhead} against {trimmed_head}..{trimmed_tail}"
    );
    visual.simulate_keystrokes("ctrl-a");
    visual.simulate_keystrokes("w");
    let (after_head, after_tail) = clip_edges(&state, visual);
    assert_eq!(after_head, trimmed_head, "W leaves the head alone");
    assert_eq!(after_tail, playhead, "W moves the tail to the playhead");
    assert!(
        after_tail - after_head > 0,
        "a trim never collapses the clip"
    );
    assert!(fps > 0, "the sequence has a frame rate");

    // Backspace takes the selection off the timeline, and Ctrl+Z puts it back.
    visual.simulate_keystrokes("ctrl-a");
    visual.simulate_keystrokes("backspace");
    assert_eq!(
        visual.read(|app| placed(state.read(app))),
        0,
        "Backspace deletes the selected clip"
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_eq!(
        visual.read(|app| placed(state.read(app))),
        1,
        "Ctrl+Z brings it back"
    );
}

/// Typed text lands in a focused box once, as the character the keystroke
/// produced, and a keystroke the shell consumes lands nowhere else.
///
/// The class this closes: a keystroke reaches the shell down two paths, the
/// key-down handler and the window's text-input handler, and a box that took
/// text from both typed every character twice, the key name first: Shift+F as
/// `fF`. The other way round, a keystroke the shell spends on a rebind capture
/// or the palette must not reach a box as text as well. The window runs both
/// paths as the Linux backends do: the key-down first, then the input handler
/// with the keystroke's text, unless the key-down stopped it. A key name that
/// is a shifted symbol, `^` or `<`, is what Linux reports for Shift+6 and
/// Shift+comma.
///
/// Not covered: an IME preedit run, which the test platform does not produce.
#[gpui::test]
fn interaction_typed_text_lands_once_and_a_consumed_key_lands_nowhere(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual) = editing!(cx, "typing");
    let query = |visual: &mut gpui::VisualTestContext| {
        visual.read(|app| state.read(app).media_query.clone())
    };

    edit(&state, visual, |state| state.focus_media_search(true));
    visual.simulate_keystrokes("shift-f i x space shift-^ shift-<");
    assert_eq!(
        query(visual),
        "Fix ^<",
        "each keystroke types its text once"
    );
    visual.simulate_keystrokes("backspace");
    assert_eq!(query(visual), "Fix ^", "Backspace takes one character");

    // A capture in flight takes the keystroke from the box that holds focus.
    edit(&state, visual, |state| state.begin_rebind("key-mixer"));
    visual.simulate_keystrokes("shift-#");
    edit(&state, visual, AppState::cancel_rebind);
    assert_eq!(query(visual), "Fix ^", "a captured chord is not typed");

    // The palette types the keystroke's text, not its key name.
    edit(&state, visual, |state| {
        state.focus_media_search(false);
        state.open_dialog(DialogKind::Palette);
    });
    visual.simulate_keystrokes("shift-f i x");
    assert_eq!(
        visual.read(|app| state.read(app).palette_query.clone()),
        "Fix",
        "the palette query takes the typed case"
    );
    assert_eq!(query(visual), "Fix ^", "palette keys reach no box");
}

/// The first clip's edges, as the timeline projects them.
fn clip_edges(state: &gpui::Entity<AppState>, visual: &mut gpui::VisualTestContext) -> (i64, i64) {
    visual.read(|app| {
        let read = state.read(app);
        let clip = read
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .next()
            .expect("one clip on the timeline");
        (clip.timeline_start_ticks, clip.timeline_end_ticks)
    })
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

/// The Home rail and the editor tool strip paint their glyphs.
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
        aria_ui::theme::Theme::install(aria_ui::theme::Appearance::Dark, Default::default(), app);
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
        "editor tool strip",
        &[
            icons::SKIP_BACK,
            icons::CHEVRON_LEFT,
            icons::PLAY,
            icons::CHEVRON_RIGHT,
            icons::SKIP_FORWARD,
            icons::MAGNET,
            icons::UPLOAD,
            icons::SETTINGS,
        ],
    );

    // The app drops at the end of the case, and gpui's leak detector fails it
    // for any entity handle still alive after `App::shutdown`.
    drop(state);
    drop(window);
    app.update(|app| app.shutdown());
}
