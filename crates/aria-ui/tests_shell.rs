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
use aria_ui::types::{ActivePanel, UiRoute};
// `App::new` is a trait method: building the state entity needs the trait in
// scope, nothing else from it.
use gpui::AppContext as _;
use gpui::{Modifiers, MouseButton, point, px};

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
/// purpose: it reads the operator's own data directory for external plugins and
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

    // Dock the mixer frame to the left of the timeline so the timeline starts
    // to the right of a left rail (`panel_x >= SIDEBAR_MIN`).
    edit(&state, visual, |state| {
        state.dock_tree.dock(
            aria_ui::dock::PANEL_MIXER,
            aria_ui::dock::FrameId::BOTTOM,
            aria_ui::dock::Zone::Left,
        );
    });
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
    let outline = visual.read(|app| state.read(app).ui_settings.outline_width);
    let content_left = panel_x + outline;
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
    let [lx, ly, lw, lh] = visual
        .read(|app| {
            state
                .read(app)
                .dock_frame_rect(aria_ui::dock::FrameId::LEFT)
        })
        .expect("the left frame records where it was drawn");
    let start = lw.round();
    let y = ly + lh * 0.5;
    // Two pixels into the six-pixel splitter, which begins where the left frame ends.
    let grip = lx + lw + 2.0;

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
    let [lx2, _, lw2, _] = visual
        .read(|app| {
            state
                .read(app)
                .dock_frame_rect(aria_ui::dock::FrameId::LEFT)
        })
        .expect("the left frame records where it was drawn");
    let grip2 = lx2 + lw2 + 2.0;
    visual.simulate_mouse_down(
        point(px(grip2), px(y)),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.simulate_mouse_move(
        point(px(grip2 - 40.0), px(y)),
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

    // Arrow keys belong to the focused pane: in the Viewer they move the
    // selected clip's picture (viewer.md), so Shift+3 gives the Timeline focus.
    // There Left and Right step the playhead whether or not a clip is selected,
    // and Ctrl+Left and Ctrl+Right move the selected clip (keyboard.md). A step
    // that moved the clip, or a Ctrl chord that moved the playhead, fails here.
    visual.simulate_keystrokes("shift-3");
    assert_eq!(
        visual.read(|app| state.read(app).active_panel),
        ActivePanel::Timeline,
        "Shift+3 focuses the Timeline"
    );
    assert_eq!(clip_edges(&state, visual).0, 0, "the clip starts at zero");
    visual.simulate_keystrokes("right right right");
    assert_eq!(
        visual.read(|app| state.read(app).playhead.value),
        3,
        "three steps forward move the playhead three frames with a clip selected"
    );
    assert_eq!(
        clip_edges(&state, visual).0,
        0,
        "stepping the playhead leaves the selected clip where it is"
    );
    visual.simulate_keystrokes("left left left");
    visual.simulate_keystrokes("ctrl-right ctrl-right ctrl-right");
    assert_eq!(
        clip_edges(&state, visual).0,
        3,
        "three Ctrl+Right nudge the selected clip three frames"
    );
    assert_eq!(
        visual.read(|app| state.read(app).playhead.value),
        0,
        "nudging a clip does not move the playhead"
    );
    visual.simulate_keystrokes("ctrl-left ctrl-left ctrl-left");
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
    let markers = |visual: &mut gpui::VisualTestContext| {
        visual.read(|app| {
            state
                .read(app)
                .store
                .sequence
                .markers
                .iter()
                .map(|marker| marker.tick)
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(
        markers(visual),
        vec![3],
        "M adds a marker under the playhead"
    );
    visual.simulate_keystrokes("ctrl-alt-m");
    assert_eq!(
        markers(visual),
        Vec::<i64>::new(),
        "Ctrl+Alt+M clears the marker under the playhead"
    );

    // Marks. Plain sets, Ctrl+Shift clears, and the two are independent.
    visual.simulate_keystrokes("right right");
    visual.simulate_keystrokes("i");
    visual.simulate_keystrokes("shift-right shift-right");
    visual.simulate_keystrokes("o");
    let (mark_in, mark_out) = visual.read(|app| {
        let read = state.read(app);
        (read.mark_in, read.mark_out)
    });
    assert_eq!(mark_in, Some(5), "I marks in at the playhead");
    assert_eq!(mark_out, Some(15), "O marks out ten frames on");
    visual.simulate_keystrokes("ctrl-shift-i");
    let (mark_in, mark_out) = visual.read(|app| {
        let read = state.read(app);
        (read.mark_in, read.mark_out)
    });
    assert_eq!(mark_in, None, "Ctrl+Shift+I clears In");
    assert_eq!(mark_out, Some(15), "clearing In leaves Out alone");

    // Trimming. Q ripple-trims the head of the clip under the playhead to the
    // playhead: the clip keeps its start, loses the frames before the playhead,
    // and the playhead returns to the start. W ripple-trims the tail.
    let (head, tail) = clip_edges(&state, visual);
    assert_eq!(head, 0, "the clip was dropped at the start");
    visual.simulate_keystrokes("q");
    let (trimmed_head, trimmed_tail) = clip_edges(&state, visual);
    assert_eq!(trimmed_head, head, "Q keeps the clip's start");
    assert_eq!(
        trimmed_tail,
        tail - 15,
        "Q removes the frames before the playhead and closes the gap"
    );
    assert_eq!(
        visual.read(|app| state.read(app).playhead.value),
        head,
        "Q returns the playhead to where the clip starts"
    );

    visual.simulate_keystrokes("end");
    visual.simulate_keystrokes("left left");
    let playhead = visual.read(|app| state.read(app).playhead.value);
    assert!(
        playhead > trimmed_head && playhead < trimmed_tail,
        "the playhead has to land inside the clip for W to have anything to do: \
         {playhead} against {trimmed_head}..{trimmed_tail}"
    );
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

/// Numeric entry goes in through the window: a digit over the Timeline opens
/// the playhead timecode with that digit, the text-input route types the next
/// one, and Enter seeks. A keypad `+` with the clip selected moves the clip and
/// leaves the playhead.
///
/// The class this closes: the first digit reaching the shell on two paths. A
/// hook that let the key-down propagate typed it twice (`112`), and one that
/// ran before the bindings stole a bound digit chord.
#[gpui::test]
fn interaction_typed_digits_seek_and_a_keypad_sign_moves_the_clip(cx: &mut gpui::TestAppContext) {
    let (state, visual) = editing!(cx, "numeric");
    edit(&state, visual, |state| {
        state.active_panel = aria_ui::ActivePanel::Timeline;
        state.clear_selection();
    });

    visual.simulate_keystrokes("1 2 enter");
    assert_eq!(
        visual.read(|app| state.read(app).playhead.value),
        12,
        "1, 2, Enter seeks to frame 12"
    );
    assert!(
        !visual.read(|app| state.read(app).clock_focused()),
        "Enter releases the timecode"
    );

    visual.simulate_keystrokes("ctrl-a");
    let before = clip_edges(&state, visual);
    visual.simulate_keystrokes("add 3 enter");
    assert_eq!(
        clip_edges(&state, visual),
        (before.0 + 3, before.1 + 3),
        "keypad +, 3, Enter moves the selected clip three frames"
    );
    assert_eq!(visual.read(|app| state.read(app).playhead.value), 12);

    // Shift+1 stays the Project panel chord rather than a typed digit.
    visual.simulate_keystrokes("shift-1");
    assert!(!visual.read(|app| state.read(app).clock_focused()));
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

/// Frame maximize on `` ` ``, frame cycling on `Ctrl+Tab` / `Ctrl+Shift+Tab`,
/// and closing the active tab on `Ctrl+W` all run through the shell's key
/// dispatcher and draw without panicking.
#[gpui::test]
fn interaction_dock_keys_maximize_cycle_and_close_tabs(cx: &mut gpui::TestAppContext) {
    use aria_ui::dock::{FrameId, PANEL_ASSISTANT, PANEL_MIXER, PANEL_VIEWER};

    let (state, visual) = editing!(cx, "dock-keys");

    // Shift+6 activates Audio Track Mixer in the centre frame.
    visual.simulate_keystrokes("shift-6");
    visual.run_until_parked();
    assert!(
        visual.read(|app| state.read(app).dock_tree.is_active(PANEL_MIXER)),
        "Shift+6 activates the Mixer tab"
    );
    assert_eq!(
        visual.read(|app| state.read(app).dock_tree.focused),
        FrameId::CENTER
    );

    // Backquote maximizes the focused frame; a second backquote restores.
    visual.simulate_keystrokes("`");
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).dock_tree.maximized),
        Some(FrameId::CENTER),
        "backquote maximizes the focused frame"
    );
    visual.simulate_keystrokes("`");
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).dock_tree.maximized),
        None,
        "second backquote restores the split tree"
    );

    // Ctrl+W closes the active Mixer tab and falls back to Viewer.
    visual.simulate_keystrokes("ctrl-w");
    visual.run_until_parked();
    assert!(
        !visual.read(|app| state.read(app).dock_tree.is_open(PANEL_MIXER)),
        "Ctrl+W closes the active Mixer tab"
    );
    assert!(
        visual.read(|app| state.read(app).dock_tree.is_active(PANEL_VIEWER))
            || visual.read(|app| state.read(app).dock_tree.is_active(PANEL_ASSISTANT)),
        "another tab in the centre frame becomes active"
    );

    // Ctrl+Tab and Ctrl+Shift+Tab cycle `focused` across open frames in reading order.
    let before = visual.read(|app| state.read(app).dock_tree.focused);
    visual.simulate_keystrokes("ctrl-tab");
    visual.run_until_parked();
    let next = visual.read(|app| state.read(app).dock_tree.focused);
    assert_ne!(next, before, "Ctrl+Tab advances to the next frame");
    visual.simulate_keystrokes("ctrl-shift-tab");
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).dock_tree.focused),
        before,
        "Ctrl+Shift+Tab returns to the previous frame"
    );
}

/// Dragging a tab onto another frame's drop zone docks it into a split, and
/// floating a panel opens a second GPUI window sharing `AppState`.
#[gpui::test]
fn interaction_dock_tab_drag_splits_frame_and_floating_window_shares_state(
    cx: &mut gpui::TestAppContext,
) {
    use aria_ui::dock::{FrameId, PANEL_ASSISTANT, PANEL_INSPECTOR, PANEL_MIXER, Zone};

    let (state, visual) = editing!(cx, "dock-drag-float");

    let center_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::CENTER))
        .expect("centre frame records its painted bounds");
    let right_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::RIGHT))
        .expect("right frame records its painted bounds");
    assert!(center_rect[2] > 0.0 && center_rect[3] > 0.0);
    assert!(right_rect[2] > 0.0 && right_rect[3] > 0.0);

    // Dock Assistant into the bottom half of the right frame and redraw.
    edit(&state, visual, |state| {
        assert!(
            state
                .dock_tree
                .dock(PANEL_ASSISTANT, FrameId::RIGHT, Zone::Bottom)
                .is_some()
        );
    });
    assert_eq!(
        visual.read(|app| state.read(app).dock_tree.root_frames().len()),
        6,
        "docking to Bottom splits the right frame into two frames (5 -> 6 root frames)"
    );
    assert!(
        visual.read(|app| state.read(app).dock_tree.check_invariants().is_ok()),
        "dock tree invariants hold after split"
    );

    // Float Mixer into a detached frame and open a second window with Shell::new_floating
    // sharing the same AppState entity.
    let float_id = visual.update(|_, app| {
        state.update(app, |state, cx| {
            state
                .float_dock_panel(PANEL_MIXER, (100.0, 100.0, 480.0, 320.0), cx)
                .expect("float mixer")
        })
    });
    let shared = gpui::Entity::clone(&state);
    let (_float_shell, float_visual) =
        window::window_view(cx, |_, cx| Shell::new_floating(shared, float_id, cx));
    float_visual.run_until_parked();
    assert_eq!(
        float_visual.read(|app| state.read(app).dock_tree.floating.len()),
        1,
        "floating window shares AppState and sees the floating frame"
    );

    // Redocking from the shared state returns the tab to the main tree.
    float_visual.update(|_, app| {
        state.update(app, |state, cx| {
            assert!(state.redock_floating_frame(float_id, cx));
            assert!(state.dock_tree.is_open(PANEL_INSPECTOR));
        });
    });
    float_visual.run_until_parked();
    assert!(
        float_visual.read(|app| state.read(app).dock_tree.floating.is_empty()),
        "redocking empties the floating list"
    );
    assert!(
        float_visual.read(|app| state.read(app).dock_tree.is_open(PANEL_MIXER)),
        "redocked Mixer tab is back in the main tree"
    );
}

/// Pointer gestures on the frame tab bar: middle-clicking the only tab in the
/// left frame closes it and collapses the frame, and dragging a tab from the
/// centre frame's tab bar into the bottom zone of the right frame splits the
/// right frame.
#[gpui::test]
fn interaction_dock_pointer_middle_close_and_drag_to_zone(cx: &mut gpui::TestAppContext) {
    use aria_ui::dock::{FrameId, PANEL_VIEWER};

    let (state, visual) = editing!(cx, "dock-pointer");

    let [left_x, left_y, _, _] = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::LEFT))
        .expect("left frame bounds");

    // Middle-click the Project tab, then the Effect Controls tab in the left
    // frame's 24 px tab bar: closing the last tab collapses the left frame.
    for panel in [aria_ui::dock::PANEL_PROJECT, aria_ui::dock::PANEL_INSPECTOR] {
        visual.simulate_mouse_down(
            point(px(left_x + 24.0), px(left_y + 12.0)),
            MouseButton::Middle,
            Modifiers::none(),
        );
        visual.simulate_mouse_up(
            point(px(left_x + 24.0), px(left_y + 12.0)),
            MouseButton::Middle,
            Modifiers::none(),
        );
        visual.run_until_parked();
        assert!(
            !visual.read(|app| state.read(app).dock_tree.is_open(panel)),
            "middle-clicking closes {panel}"
        );
    }
    assert!(
        visual.read(|app| state.read(app).dock_tree.frame(FrameId::LEFT).is_none()),
        "closing the last tab in the left frame collapses the frame"
    );

    // Drag the active Viewer tab from the centre frame's tab bar to the bottom
    // drop zone of the right frame.
    let [cx_x, cx_y, _, _] = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::CENTER))
        .expect("centre frame bounds");
    let [rx_x, rx_y, rx_w, rx_h] = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::RIGHT))
        .expect("right frame bounds");

    let start_pt = point(px(cx_x + 24.0), px(cx_y + 12.0));
    let drop_pt = point(px(rx_x + rx_w * 0.5), px(rx_y + rx_h * 0.90));
    visual.simulate_mouse_down(start_pt, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(drop_pt, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    visual.simulate_mouse_up(drop_pt, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();

    let viewer_frame = visual
        .read(|app| state.read(app).dock_tree.find(PANEL_VIEWER))
        .expect("Viewer remains docked");
    assert_ne!(
        viewer_frame,
        FrameId::CENTER,
        "dragging Viewer to the right frame's bottom zone moves it into a new split frame"
    );
}

/// Editing arrangement matches `docs/spec/layout.md`:
/// - Upper row: Left (`Project | Effect Controls`, 320 px), Centre (`Viewer`),
///   Right (`Effects & Presets` active, 300 px; unregistered `Info`/`Character`
///   filtered from drawn tabs).
/// - Lower row: `Timeline` on the left, `Audio Meters` (64 px) on the right.
/// - Dragging the splitter between `CENTER` and `RIGHT` or between upper and
///   lower rows resizes the frames, and double-clicking restores the workspace's
///   saved `0.55 / 0.45` ratio.
#[gpui::test]
fn interaction_dock_editing_arrangement_and_splitter_restore(cx: &mut gpui::TestAppContext) {
    use aria_ui::dock::{
        FrameId, PANEL_AUDIO_METERS, PANEL_EFFECTS, PANEL_INSPECTOR, PANEL_PROJECT, PANEL_TIMELINE,
        PANEL_VIEWER,
    };

    let (state, visual) = editing!(cx, "dock-editing-arrangement");

    let left_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::LEFT))
        .expect("LEFT frame drawn");
    let center_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::CENTER))
        .expect("CENTER frame drawn");
    let right_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::RIGHT))
        .expect("RIGHT frame drawn");
    let bottom_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::BOTTOM))
        .expect("BOTTOM timeline frame drawn");
    let meters_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::METERS))
        .expect("METERS audio meters frame drawn");

    // Upper row frames sit side by side above the lower row.
    assert!(left_rect[0] < center_rect[0] && center_rect[0] < right_rect[0]);
    assert!(bottom_rect[1] >= left_rect[1] + left_rect[3] - 2.0);
    assert!(meters_rect[0] >= bottom_rect[0] + bottom_rect[2] - 2.0);
    assert!(
        (meters_rect[2] - 64.0).abs() <= 12.0,
        "Audio Meters frame is 64 px wide in lower row: {meters_rect:?}"
    );

    visual.read(|app| {
        let s = state.read(app);
        let left = s.dock_tree.frame(FrameId::LEFT).expect("LEFT");
        assert_eq!(left.tabs, vec![PANEL_PROJECT, PANEL_INSPECTOR]);
        assert_eq!(left.active_panel(), PANEL_PROJECT);
        let right = s.dock_tree.frame(FrameId::RIGHT).expect("RIGHT");
        assert_eq!(right.active_panel(), PANEL_EFFECTS);
        let bottom = s.dock_tree.frame(FrameId::BOTTOM).expect("BOTTOM");
        assert_eq!(bottom.active_panel(), PANEL_TIMELINE);
        let meters = s.dock_tree.frame(FrameId::METERS).expect("METERS");
        assert_eq!(meters.active_panel(), PANEL_AUDIO_METERS);
        let center = s.dock_tree.frame(FrameId::CENTER).expect("CENTER");
        assert_eq!(center.active_panel(), PANEL_VIEWER);
    });

    // Drag the vertical splitter between the upper row and lower row down by 50 px,
    // then double-click it to restore the saved 0.55 / 0.45 workspace ratio.
    let split_x = center_rect[0] + center_rect[2] * 0.5;
    let split_y = left_rect[1] + left_rect[3] + 2.0;
    visual.simulate_mouse_down(
        point(px(split_x), px(split_y)),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.simulate_mouse_move(
        point(px(split_x), px(split_y + 50.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.simulate_mouse_up(
        point(px(split_x), px(split_y + 50.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.run_until_parked();

    let moved_weights = visual
        .read(|app| state.read(app).dock_tree.split_weights(&[]))
        .expect("root weights");
    assert!(
        (moved_weights[0] - 0.55).abs() > 0.01,
        "dragging vertical splitter changed root weights: {moved_weights:?}"
    );

    // Double-click the moved vertical splitter to restore 0.55 / 0.45.
    let new_left_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::LEFT))
        .expect("LEFT frame drawn");
    let new_split_y = new_left_rect[1] + new_left_rect[3] + 2.0;
    visual.simulate_event(gpui::MouseDownEvent {
        position: point(px(split_x), px(new_split_y)),
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    visual.run_until_parked();
    let restored_weights = visual
        .read(|app| state.read(app).dock_tree.split_weights(&[]))
        .expect("root weights");
    assert!((restored_weights[0] - 0.55).abs() < 1e-4);
    assert!((restored_weights[1] - 0.45).abs() < 1e-4);
}

/// Single-clicking an active tab keeps the frame open and focused (never
/// collapsing it), and motion/drop-zone/multi-monitor helpers obey the spec.
#[gpui::test]
fn interaction_dock_active_tab_click_never_collapses_and_helpers_match_spec(
    cx: &mut gpui::TestAppContext,
) {
    use aria_ui::dock::{FloatRect, FrameId, PANEL_PROJECT, Zone};
    use aria_ui::shell::dock_view::{
        DROP_ZONE_FOLLOW_SPEC, DROP_ZONE_SPEC, MAXIMIZE_SPEC, WORKSPACE_MORPH_SPEC,
        clamp_float_rect_to_displays, drop_zone_preview_fractions, splitter_motion_key,
    };

    let (state, visual) = editing!(cx, "dock-active-tab-click");
    let [left_x, left_y, left_w, _] = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::LEFT))
        .expect("LEFT bounds");

    // Click the already-active Project tab twice as single clicks: the frame
    // stays open and unmaximized, and focus moves to FrameId::LEFT.
    let project_tab_pt = point(px(left_x + 24.0), px(left_y + 12.0));
    visual.simulate_click(project_tab_pt, Modifiers::none());
    visual.run_until_parked();
    assert!(visual.read(|app| state.read(app).dock_tree.is_open(PANEL_PROJECT)));
    assert_eq!(
        visual.read(|app| state.read(app).dock_tree.focused),
        FrameId::LEFT
    );
    assert_eq!(visual.read(|app| state.read(app).dock_tree.maximized), None);
    let [_, _, left_w_after, _] = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::LEFT))
        .expect("LEFT still drawn");
    assert!((left_w_after - left_w).abs() < 1.0);

    // Splitter motion keys for sibling nested splits at the same depth never collide.
    assert_ne!(splitter_motion_key(&[0], 0), splitter_motion_key(&[1], 0));
    assert_ne!(splitter_motion_key(&[0], 0), splitter_motion_key(&[0], 1));

    // Drop-zone preview fractions cover the exact half/full regions (`layout.md:38-41`).
    assert_eq!(
        drop_zone_preview_fractions(Zone::Left),
        (0.0, 0.0, 0.5, 1.0)
    );
    assert_eq!(
        drop_zone_preview_fractions(Zone::Right),
        (0.5, 0.0, 0.5, 1.0)
    );
    assert_eq!(drop_zone_preview_fractions(Zone::Top), (0.0, 0.0, 1.0, 0.5));
    assert_eq!(
        drop_zone_preview_fractions(Zone::Bottom),
        (0.0, 0.5, 1.0, 0.5)
    );
    assert_eq!(
        drop_zone_preview_fractions(Zone::Center),
        (0.0, 0.0, 1.0, 1.0)
    );

    // Off-screen floating window is clamped onto the primary monitor (`layout.md:48`),
    // while a valid second-monitor rectangle is preserved.
    let displays = [[0.0, 0.0, 1920.0, 1080.0], [1920.0, 0.0, 1920.0, 1080.0]];
    let clamped = clamp_float_rect_to_displays(
        FloatRect {
            x: -2400.0,
            y: -1200.0,
            w: 480.0,
            h: 320.0,
        },
        (160.0, 80.0),
        &displays,
    );
    assert!(clamped.x >= 0.0 && clamped.x + clamped.w <= 1920.0);
    assert!(clamped.y >= 0.0 && clamped.y + clamped.h <= 1080.0);

    let second_mon = clamp_float_rect_to_displays(
        FloatRect {
            x: 2100.0,
            y: 120.0,
            w: 480.0,
            h: 320.0,
        },
        (160.0, 80.0),
        &displays,
    );
    assert_eq!(second_mon.x, 2100.0);
    assert_eq!(second_mon.y, 120.0);

    // MotionSpec timings match `docs/spec/layout.md:51-53, 81`, and the
    // docking and workspace curve matches `docs/spec/look.md` Motion.
    assert_eq!(DROP_ZONE_SPEC.delay_ms, 80);
    assert_eq!(DROP_ZONE_SPEC.duration_ms, 120);
    assert_eq!(DROP_ZONE_FOLLOW_SPEC.duration_ms, 120);
    assert_eq!(MAXIMIZE_SPEC.duration_ms, 160);
    assert_eq!(WORKSPACE_MORPH_SPEC.duration_ms, 180);
    assert_eq!(
        WORKSPACE_MORPH_SPEC.easing,
        gpui::motion::Easing::EaseOut,
        "a workspace change decelerates into place"
    );
}
