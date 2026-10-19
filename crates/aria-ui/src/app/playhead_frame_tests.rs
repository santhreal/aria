//! A playhead moved while paused shows the frame at the new playhead, whichever
//! key, menu row, palette row or plugin moved it.
//!
//! Closes the class "a paused playhead move that leaves the old frame in the
//! Viewer": Home, End and the arrow keys moved the playhead and asked for no
//! frame, so the Viewer kept the picture from before the move under a timecode
//! that had changed. Every move lands in `set_playhead`, which marks the frame
//! stale, and the shell's next draw asks for it. The first test moves the
//! playhead through `set_playhead` the way a control that asks for no frame
//! moves it. The second presses, in the window, every shipped chord that moves
//! a paused playhead; the set is derived from the binding table at run time, so
//! a chord added later is covered on the run that adds it.
//!
//! Not covered: a write to `AppState::playhead` that bypasses `set_playhead`,
//! which marks nothing, and the Source monitor, whose frame is asked for by its
//! own draw.

use std::path::PathBuf;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};

use super::AppState;
use crate::keys::{Binding, KeyBody, KeyRoute};
use crate::test_support::drawn_shell;
use crate::types::ActivePanel;

/// Rounds of draw and decode a settle allows before it reports the Viewer
/// stuck.
const SETTLE_ROUNDS: usize = 64;

/// The panels a chord is tried under, in order: the window's context, then the
/// two panels with contexts of their own.
const FOCUS: [ActivePanel; 3] = [
    ActivePanel::Media,
    ActivePanel::Timeline,
    ActivePanel::Preview,
];

/// Chords the derived set of playhead movers must include.
const TRANSPORT: [&str; 6] = [
    "key-start",
    "key-end",
    "key-step-back",
    "key-step-forward",
    "key-step-back-fast",
    "key-step-forward-fast",
];

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// A headless editor over `ctx` with an empty project open.
///
/// The first editor over a context registers its engine there and every later
/// one opens on it, so the compositor is built once rather than once a state.
fn editor(ctx: &aria_plugin::PluginContext, tag: &str) -> AppState {
    let mut state = AppState::new(ctx.clone(), crate::scratch_pool::scratch(tag))
        .unwrap_or_else(|err| panic!("editor state: {err:?}"));
    state
        .new_project()
        .unwrap_or_else(|err| panic!("a project to edit: {err:?}"));
    state
}

/// One still clip, two sequence markers, In and Out, the clip selected, and the
/// playhead in the middle. Returns the middle.
fn lay_out(state: &mut AppState) -> i64 {
    let media = state.import_media(fixture()).expect("import the fixture");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place the clip");
    let duration = state.store.sequence.duration_ticks;
    assert!(
        duration >= 64,
        "a clip of {duration} ticks leaves no room to step"
    );
    for at in [duration / 4, duration * 3 / 4] {
        state.set_playhead(at);
        state.add_marker_at_playhead().expect("a marker");
    }
    state.set_playhead(duration / 8);
    state.set_mark_in();
    state.set_playhead(duration * 7 / 8);
    state.set_mark_out();
    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| aria_models::ClipId::from_raw(clip.id))
        .next()
        .expect("one clip");
    state.select_clip(clip);
    let mid = duration / 2;
    state.set_playhead(mid);
    mid
}

/// Run `binding` the way the shell's key handler runs it.
fn press(state: &mut AppState, binding: &Binding, cx: &mut gpui::Context<AppState>) {
    match binding.run {
        KeyBody::Plain(run) => crate::keys::dispatch_plain_key_down(state, binding, false, run),
        KeyBody::Windowed(run) => run(state, cx),
        KeyBody::Command(_) => state.run_plugin_command(binding.id),
    }
}

/// The panel whose focus makes the shell resolve `binding`, or none when no
/// focus does.
fn focus_for(router: &mut AppState, binding: &Binding) -> Option<ActivePanel> {
    let host = router
        .key_host()
        .expect("the UI plugin mounts the key host");
    let chord = binding.chord;
    FOCUS.into_iter().find(|&panel| {
        router.active_panel = panel;
        host.resolve_focused(
            crate::keys::focus_route(router),
            chord.key,
            chord.ctrl,
            chord.shift,
            chord.alt,
        )
        .is_some_and(|fired| fired.id == binding.id)
    })
}

/// Every shipped binding that moves a paused playhead over the laid-out
/// project, with the panel whose focus fires it.
///
/// Each binding runs on a state and a project of its own, so one binding's
/// edit cannot move or hide another's move. The states share one plugin
/// context with the UI plugin mounted, as the window's state has, and with it
/// one engine and one key host.
fn movers(cx: &TestAppContext) -> Vec<(Binding, ActivePanel)> {
    let ctx = aria_plugin::PluginContext::new();
    ctx.mount(&crate::plugins::UiPlugin)
        .unwrap_or_else(|err| panic!("the ui plugin needs nothing: {err:?}"));
    let mut router = editor(&ctx, "playhead-route");
    let mut found = Vec::new();
    for binding in crate::keys::bindings::SHIPPED_BINDINGS {
        if binding.route == KeyRoute::Home {
            continue;
        }
        let Some(panel) = focus_for(&mut router, binding) else {
            continue;
        };
        let mut state = editor(&ctx, binding.id);
        let mid = lay_out(&mut state);
        state.active_panel = panel;
        let entity = cx.update(|app| app.new(|_| state));
        cx.update(|app| entity.update(app, |state, cx| press(state, binding, cx)));
        let (playhead, playing) = cx.read(|app| {
            let state = entity.read(app);
            (state.playhead.value, state.playing)
        });
        if playhead != mid && !playing {
            found.push((*binding, panel));
        }
    }
    found
}

/// How many frames the Viewer has asked for.
fn requests(state: &Entity<AppState>, visual: &VisualTestContext) -> u64 {
    visual.read(|app| state.read(app).preview_generation)
}

/// The playhead, and the tick of the frame on screen.
fn on_screen(state: &Entity<AppState>, visual: &VisualTestContext) -> (i64, Option<i64>) {
    visual.read(|app| {
        let state = state.read(app);
        (state.playhead.value, state.shown_tick)
    })
}

/// Draw and decode until no frame is in flight and none is owed.
async fn settle(state: &Entity<AppState>, visual: &mut VisualTestContext) {
    for _ in 0..SETTLE_ROUNDS {
        visual.run_until_parked();
        let busy = visual.read(|app| {
            let state = state.read(app);
            state.preview_decoding || state.still_frame_stale()
        });
        if !busy {
            return;
        }
        let executor = visual.executor();
        let barrier = visual.read(|app| state.read(app).preview_worker.run(&executor, || ()));
        assert_eq!(barrier.await, Some(()), "the preview worker stopped");
    }
    panic!("the Viewer still owed a frame after {SETTLE_ROUNDS} rounds of draw and decode");
}

/// Open a new project, lay it out with `panel` focused, and show the frame at
/// its middle. The frame is asked for here, so the starting picture does not
/// rest on the mark under test.
async fn laid_out(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    panel: ActivePanel,
) -> i64 {
    let mid = visual.update(|_, app| {
        state.update(app, |state, cx| {
            state
                .new_project()
                .unwrap_or_else(|err| panic!("a project to edit: {err:?}"));
            let mid = lay_out(state);
            state.active_panel = panel;
            cx.notify();
            mid
        })
    });
    visual.update(|_, app| crate::app::spawn_preview(state.clone(), app));
    settle(state, visual).await;
    assert_eq!(
        on_screen(state, visual),
        (mid, Some(mid)),
        "the laid-out project does not show the frame at its middle"
    );
    mid
}

/// The keystroke a window delivers for `binding`'s chord.
fn keystroke(binding: &Binding) -> gpui::Keystroke {
    gpui::Keystroke {
        modifiers: gpui::Modifiers {
            control: binding.chord.ctrl,
            shift: binding.chord.shift,
            alt: binding.chord.alt,
            ..Default::default()
        },
        key: binding.chord.key.to_owned(),
        key_char: None,
    }
}

#[gpui::test]
async fn a_paused_move_shows_the_frame_at_the_new_playhead(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "playhead-frame");
    let mid = laid_out(&state, visual, ActivePanel::Timeline).await;
    let end = visual.read(|app| state.read(app).store.sequence.duration_ticks);
    // Forward, back, to the start, and past the end, which clamps to the end.
    for (to, lands) in [
        (mid + 1, mid + 1),
        (mid - 3, mid - 3),
        (0, 0),
        (end + 50, end),
    ] {
        let asked = requests(&state, visual);
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.set_playhead(to);
                cx.notify();
            });
        });
        settle(&state, visual).await;
        assert!(
            requests(&state, visual) > asked,
            "a paused move to {to} asked for no frame"
        );
        assert_eq!(
            on_screen(&state, visual),
            (lands, Some(lands)),
            "a paused move to {to} left another frame on screen"
        );
    }
    // A move onto the playhead changes nothing on screen and asks for nothing.
    let asked = requests(&state, visual);
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.set_playhead(end);
            cx.notify();
        });
    });
    settle(&state, visual).await;
    assert_eq!(
        requests(&state, visual),
        asked,
        "a move onto the playhead asked for a frame"
    );
    assert_eq!(on_screen(&state, visual), (end, Some(end)));
}

#[gpui::test]
async fn every_chord_that_moves_a_paused_playhead_shows_the_frame_it_lands_on(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let movers = movers(cx);
    let ids: Vec<&str> = movers.iter().map(|(binding, _)| binding.id).collect();
    for id in TRANSPORT {
        assert!(
            ids.contains(&id),
            "{id} does not move a paused playhead; the movers found: {ids:?}"
        );
    }
    let (state, visual) = drawn_shell(cx, "playhead-keys");
    for (binding, panel) in movers {
        let mid = laid_out(&state, visual, panel).await;
        let fired = visual.read(|app| {
            let state = state.read(app);
            let chord = binding.chord;
            state
                .key_host()
                .and_then(|host| {
                    host.resolve_focused(
                        crate::keys::focus_route(state),
                        chord.key,
                        chord.ctrl,
                        chord.shift,
                        chord.alt,
                    )
                })
                .map(|fired| fired.id)
        });
        assert_eq!(
            fired,
            Some(binding.id),
            "the window does not resolve {} with {panel:?} focused",
            binding.id
        );
        let asked = requests(&state, visual);
        let taken =
            visual.update(|window, app| window.dispatch_keystroke(keystroke(&binding), app));
        assert!(taken, "the window let {} through", binding.id);
        settle(&state, visual).await;
        let (playhead, shown) = on_screen(&state, visual);
        assert!(
            !visual.read(|app| state.read(app).playing),
            "{} started playback in the window and not in the probe",
            binding.id
        );
        assert_ne!(
            playhead, mid,
            "{} moved the playhead in the probe and not in the window",
            binding.id
        );
        assert!(
            requests(&state, visual) > asked,
            "{} moved the playhead to {playhead} and asked for no frame",
            binding.id
        );
        assert_eq!(
            shown,
            Some(playhead),
            "{} moved the playhead to {playhead} and the Viewer shows {shown:?}",
            binding.id
        );
    }
}
