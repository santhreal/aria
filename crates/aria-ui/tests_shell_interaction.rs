use aria_ui::settings::{SIDEBAR_MAX, SIDEBAR_MIN};
use aria_ui::types::ActivePanel;
use gpui::{Modifiers, MouseButton, point, px};

use super::*;

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

    // The rail opens at the widest width it records, so the first drag narrows
    // it and the second widens it.
    visual.simulate_mouse_down(point(px(grip), px(y)), MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(
        point(px(grip - 60.0), px(y)),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.run_until_parked();
    let narrowed = visual.read(|app| state.read(app).ui_settings.sidebar_width);
    assert_eq!(
        narrowed,
        (start - 60.0).clamp(SIDEBAR_MIN, SIDEBAR_MAX),
        "the rail follows the pointer while the button is down"
    );

    visual.simulate_mouse_up(
        point(px(grip - 60.0), px(y)),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).ui_settings.sidebar_width),
        narrowed,
        "the release keeps the width the drag reached"
    );

    // A second drag, abandoned: Escape puts the rail back where the press found
    // it, which is the shell's own gesture cancel and not a binding.
    let before = narrowed;
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
        point(px(grip2 + 40.0), px(y)),
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
