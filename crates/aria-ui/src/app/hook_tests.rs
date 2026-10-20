//! Contracts for the two shipped extension points.
//!
//! The class closed here is "a policy lives in one action". A track lock was
//! honoured by exactly two model actions, and a dropped path's meaning was an
//! extension test written twice in the shell. Both are now plugin hooks: a
//! waterfall that may refuse a command before it reaches the engine, and a serial
//! election that asks the plugins which one owns a file type.
//!
//! Not covered here: the drawing of the status strip, and the loading of an
//! external plugin, which has no directory yet.

use std::path::{Path, PathBuf};

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, RationalTime, TrackId};
use aria_plugin::{CommandProposed, DropAction, Origin, PluginContext, ResolveDropped};

use super::AppState;
use crate::plugins::mount_all;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// A proposal outside any host, for exercising the chain alone.
fn proposal(command: Command) -> CommandProposed {
    let project = aria_models::Project::new("hooks", aria_core::Rational::FPS_24);
    CommandProposed::new(command, std::sync::Arc::new(project), Origin::Editor)
}

/// The shipped configuration: every built-in plugin mounted, one clip placed.
/// The lock guard is an engine-plugin handler, so a bare context would not have
/// it — which is the point of mounting here.
pub(crate) fn mounted_state_with_clip() -> (AppState, ClipId, TrackId) {
    let ctx = PluginContext::new();
    let dir = crate::scratch_pool::scratch("hooks");
    let _ = mount_all(&ctx, &dir);
    let mut state = AppState::new(ctx, dir).expect("state");
    state.new_project().expect("new project");
    let media = state.import_media(fixture()).expect("import fixture");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place clip");
    let (clip, track) = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter().map(move |clip| (clip.id, track.id)))
        .next()
        .expect("one clip");
    (state, ClipId::from_raw(clip), TrackId::from_raw(track))
}

fn move_to(state: &mut AppState, clip: ClipId, track: TrackId, start: i64) -> Result<(), String> {
    state
        .apply_command(Command::Edit(EditCommand::MoveClip {
            clip,
            to_track: track,
            start: RationalTime::new(start, state.playhead.rate),
        }))
        .map(|_| ())
        .map_err(|err| match err {
            EngineError::Refused(reason) => reason,
            other => panic!("expected a refusal, got {other}"),
        })
}

fn clip_start(state: &AppState, clip: ClipId) -> i64 {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|c| c.id == clip.raw())
        .expect("clip still present")
        .timeline_start_ticks
}

#[test]
fn a_locked_lane_refuses_the_edit_and_names_itself() {
    let (mut state, clip, track) = mounted_state_with_clip();
    move_to(&mut state, clip, track, 24).expect("an unlocked lane accepts the move");
    let before = clip_start(&state, clip);
    assert_eq!(before, 24);
    let revision = state.revision;

    state.set_track_locked(track, true).expect("lock the lane");
    let reason = move_to(&mut state, clip, track, 96).expect_err("a locked lane refuses");
    let name = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|t| t.id == track.raw())
        .map(|t| t.name.to_string())
        .expect("track row");
    assert!(
        reason.contains(&name),
        "the refusal must name the lane, got {reason:?}"
    );
    assert_eq!(
        clip_start(&state, clip),
        before,
        "a refused command must not move the clip"
    );
    assert!(
        state.revision > revision,
        "locking is itself an edit, so the revision moved for that and nothing else"
    );

    // A refusal is copy the user can act on, not an internal error string.
    state.report_error(EngineError::Refused(reason.clone()));
    assert_eq!(state.last_error.as_deref(), Some(reason.as_str()));
}

#[test]
fn unlocking_a_locked_lane_is_the_one_command_the_guard_lets_through() {
    let (mut state, clip, track) = mounted_state_with_clip();
    state.set_track_locked(track, true).expect("lock the lane");
    move_to(&mut state, clip, track, 96).expect_err("still locked");

    // Without the exemption a lock would be permanent: the command that clears it
    // targets the locked lane.
    state
        .set_track_locked(track, false)
        .expect("a lock can always be cleared");
    move_to(&mut state, clip, track, 96).expect("the lane is free again");
    assert_eq!(clip_start(&state, clip), 96);
}

/// `CommandProposed` runs with the engine locked, so a shipped handler that
/// locks the engine itself hangs every edit; render-and-replace did that once,
/// emitting the event under its own lock while the lane policy locked again.
/// Every edit path goes through the host now, and every mounted handler runs
/// on each proposal, so one bounded proposal per path covers the class: a
/// handler that blocks fails here instead of hanging the suite.
///
/// Not caught: a handler that deadlocks only for a command kind other than
/// the ones proposed here.
#[test]
fn every_mounted_handler_answers_a_proposal_without_locking_the_engine() {
    let (mut state, clip, track) = mounted_state_with_clip();
    state.set_track_locked(track, true).expect("lock the lane");
    let host = std::sync::Arc::clone(state.host());
    let (sent, answers) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let single = host.apply(
            &Origin::Editor,
            Command::Edit(EditCommand::RippleDelete { clip }),
        );
        let transacted = host.transact(
            Origin::plugin("test.hooks"),
            "Delete",
            vec![Command::Edit(EditCommand::RippleDelete { clip })],
        );
        let _ = sent.send((
            single.map(|_| ()).map_err(|err| err.to_string()),
            transacted.map(|_| ()).map_err(|err| err.to_string()),
        ));
    });
    let Ok((single, transacted)) = answers.recv_timeout(std::time::Duration::from_secs(20)) else {
        // The engine is still locked by the stuck thread; dropping the state
        // would wait on it forever.
        std::mem::forget(state);
        panic!("a CommandProposed handler blocked on the engine it runs under");
    };
    for (path, answer) in [("apply", single), ("transact", transacted)] {
        let reason = answer.expect_err("the lane is locked");
        assert!(
            reason.contains("is locked"),
            "{path}: the lane policy must answer from the proposal, got {reason:?}"
        );
    }
}

#[test]
fn every_dropped_file_type_goes_to_the_plugin_that_owns_it() {
    let (mut state, clip, _) = mounted_state_with_clip();
    state.select_clip(clip);
    let cases: &[(&str, DropAction)] = &[
        ("/tmp/clip.mp4", DropAction::ImportMedia),
        ("/tmp/take.MOV", DropAction::ImportMedia),
        ("/tmp/warm.cube", DropAction::ApplyLut),
        ("/tmp/WARM.CUBE", DropAction::ApplyLut),
        ("/tmp/pack/SKILL.md", DropAction::InstallSkill),
        ("/tmp/cut.edl", DropAction::ImportTimeline),
        ("/tmp/cut.SRT", DropAction::ImportTimeline),
        ("/tmp/cut.vtt", DropAction::ImportTimeline),
        // Unclaimed means media: the importer's own error is the better message.
        ("/tmp/notes.txt", DropAction::ImportMedia),
        ("/tmp/extensionless", DropAction::ImportMedia),
        // A plain markdown file is not a skill.
        ("/tmp/pack/README.md", DropAction::ImportMedia),
    ];
    for (path, want) in cases {
        assert_eq!(
            state.classify_drop(Path::new(path)),
            *want,
            "{path} was claimed by the wrong plugin"
        );
    }

    // With nothing selected a grade has no clip to apply to, so the importer
    // for its extension reads it onto the sequence.
    state.clear_selection();
    for path in ["/tmp/warm.cube", "/tmp/WARM.CUBE"] {
        assert_eq!(
            state.classify_drop(Path::new(path)),
            DropAction::ImportTimeline,
            "{path} with no clip selected"
        );
    }
}

#[test]
fn the_project_format_is_the_host_s_and_no_plugin_set_changes_it() {
    // The engine plugin does not claim `.aria`: on a machine where it fails to
    // mount, a dropped project must still open rather than be imported as media.
    for state in [
        mounted_state_with_clip().0,
        AppState::new(
            PluginContext::new(),
            crate::scratch_pool::scratch("hooks-bare"),
        )
        .expect("bare state"),
    ] {
        assert_eq!(
            state.classify_drop(Path::new("/tmp/edit.aria")),
            DropAction::OpenProject
        );
        assert_eq!(
            state.classify_drop(Path::new("/tmp/EDIT.ARIA")),
            DropAction::OpenProject
        );
    }
}

#[test]
fn the_first_plugin_to_claim_a_path_wins_and_the_rest_are_not_asked() {
    let ctx = PluginContext::new();
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let first = std::sync::Arc::clone(&asked);
    let second = std::sync::Arc::clone(&asked);
    let third = std::sync::Arc::clone(&asked);
    let _a = ctx.on_serial::<ResolveDropped, DropAction, _>(move |_| {
        first.lock().unwrap().push("a");
        None
    });
    let _b = ctx.on_serial::<ResolveDropped, DropAction, _>(move |_| {
        second.lock().unwrap().push("b");
        Some(DropAction::ApplyLut)
    });
    let _c = ctx.on_serial::<ResolveDropped, DropAction, _>(move |_| {
        third.lock().unwrap().push("c");
        Some(DropAction::InstallSkill)
    });

    let claimed = ctx
        .events()
        .serial::<ResolveDropped, DropAction>(&ResolveDropped::path(PathBuf::from("anything")));
    assert_eq!(claimed, Some(DropAction::ApplyLut));
    assert_eq!(
        asked.lock().unwrap().as_slice(),
        &["a", "b"],
        "handlers are asked in registration order and stop at the first claim"
    );

    // A higher-priority handler registered later runs before lower-priority
    // handlers and claims first; equal priorities keep mount order.
    let asked_prio = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let p1 = std::sync::Arc::clone(&asked_prio);
    let p2 = std::sync::Arc::clone(&asked_prio);
    let p3 = std::sync::Arc::clone(&asked_prio);
    let ctx_prio = PluginContext::new();
    let _low = ctx_prio.on_serial::<ResolveDropped, DropAction, _>(move |_| {
        p1.lock().push("low-0");
        Some(DropAction::ImportMedia)
    });
    let _high_first =
        ctx_prio.on_serial_with_priority::<ResolveDropped, DropAction, _>(10, move |_| {
            p2.lock().push("high-10-a");
            None
        });
    let _high_second =
        ctx_prio.on_serial_with_priority::<ResolveDropped, DropAction, _>(10, move |_| {
            p3.lock().push("high-10-b");
            Some(DropAction::Handled)
        });
    let prio_claim = ctx_prio
        .events()
        .serial::<ResolveDropped, DropAction>(&ResolveDropped::url("https://example.invalid/feed"));
    assert_eq!(prio_claim, Some(DropAction::Handled));
    assert_eq!(
        asked_prio.lock().as_slice(),
        &["high-10-a", "high-10-b"],
        "higher priority runs first and equal priorities preserve mount order"
    );
}

#[test]
fn the_first_refusal_wins_and_the_rest_of_the_chain_does_not_run() {
    let ctx = PluginContext::new();
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let first = std::sync::Arc::clone(&asked);
    let second = std::sync::Arc::clone(&asked);
    let third = std::sync::Arc::clone(&asked);
    let _a = ctx.on_waterfall::<CommandProposed, _>(move |_| first.lock().unwrap().push("a"));
    let _b = ctx.on_waterfall::<CommandProposed, _>(move |proposed| {
        second.lock().unwrap().push("b");
        proposed.refuse("b said no");
    });
    let _c = ctx.on_waterfall::<CommandProposed, _>(move |proposed| {
        third.lock().unwrap().push("c");
        proposed.refuse("c said no");
    });

    let mut proposed = proposal(Command::Edit(EditCommand::SetTrackLocked {
        track: TrackId::from_raw(1),
        locked: true,
    }));
    ctx.events().emit_waterfall(&mut proposed);
    assert_eq!(proposed.veto.as_deref(), Some("b said no"));
    assert_eq!(
        asked.lock().unwrap().as_slice(),
        &["a", "b"],
        "handlers run in registration order and stop at the refusal"
    );
}

#[test]
fn a_handler_may_rewrite_the_command_instead_of_refusing_it() {
    let ctx = PluginContext::new();
    let _guard = ctx.on_waterfall::<CommandProposed, _>(|proposed| {
        if let Command::Edit(EditCommand::SetTrackLocked { locked, .. }) = &mut proposed.command {
            *locked = false;
        }
    });
    let mut proposed = proposal(Command::Edit(EditCommand::SetTrackLocked {
        track: TrackId::from_raw(1),
        locked: true,
    }));
    ctx.events().emit_waterfall(&mut proposed);
    assert!(proposed.veto.is_none());
    assert!(matches!(
        proposed.command,
        Command::Edit(EditCommand::SetTrackLocked { locked: false, .. })
    ));
}

#[test]
fn an_unclaimed_path_is_media_when_no_plugin_is_mounted_at_all() {
    // The default must not depend on the plugin set: a host with no claim
    // handlers still imports, rather than doing nothing with the drop.
    let ctx = PluginContext::new();
    let state = AppState::new(ctx, crate::scratch_pool::scratch("hooks-none")).expect("bare state");
    assert_eq!(
        state.classify_drop(Path::new("/tmp/warm.cube")),
        DropAction::ImportMedia,
        "with no effects plugin nothing claims a LUT, so import reports it"
    );
    assert_eq!(
        state.classify_drop(&fixture()),
        DropAction::ImportMedia,
        "a real media file with no decoder plugin is still media"
    );
}

/// A decoder plugin's extensions join the list every import dialog offers,
/// after the shipped ones, until its registration drops. Closes "a plugin
/// reads a format the File dialog will not let anyone pick".
#[test]
fn import_dialogs_offer_what_decoder_plugins_read() {
    use crate::app::media::import::{MEDIA_IMPORT_EXTENSIONS, media_import_extensions};

    struct Nothing;
    impl aria_plugin::ExternalDecoder for Nothing {
        fn probe(&self, _path: &Path) -> Result<aria_plugin::MediaProbe, aria_core::DecodeError> {
            Err(aria_core::DecodeError::unsupported("reads nothing"))
        }
        fn open_video(
            &self,
            _path: &Path,
        ) -> Result<Box<dyn aria_core::VideoDecoder>, aria_core::DecodeError> {
            Err(aria_core::DecodeError::unsupported("reads nothing"))
        }
    }

    let ctx = PluginContext::new();
    let held = ctx
        .register_decoder(
            aria_plugin::DecoderSpec {
                id: "nothing".into(),
                name: "Nothing".into(),
                extensions: vec!["AriaUiOnly".into()],
                priority: 0,
            },
            Nothing,
        )
        .expect("registers");
    let offered = media_import_extensions();
    assert_eq!(
        offered[..MEDIA_IMPORT_EXTENSIONS.len()],
        *MEDIA_IMPORT_EXTENSIONS
    );
    assert!(offered.contains(&"ariauionly".to_owned()), "{offered:?}");
    drop(held);
    assert!(!media_import_extensions().contains(&"ariauionly".to_owned()));
}

#[gpui::test]
fn dropped_url_goes_through_resolve_dropped_and_unclaimed_url_posts_notice(
    cx: &mut gpui::TestAppContext,
) {
    use crate::test_support::drawn_shell;
    use gpui::{point, px};

    let (state, visual) = drawn_shell(cx, "url-drops");
    visual.run_until_parked();

    visual.simulate_url_drop(
        point(px(200.), px(200.)),
        ["https://example.invalid/unclaimed.mp4"],
    );
    let notice = visual.read(|app| state.read(app).status_message.clone());
    assert_eq!(
        notice.as_deref(),
        Some("No plugin accepts https://example.invalid/unclaimed.mp4")
    );

    let seen = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
    let seen_in_handler = std::sync::Arc::clone(&seen);
    let _sub = visual.update(|_, app| {
        state.update(app, |state, _| {
            state.status_message = None;
            state
                .ctx
                .on_serial_with_priority::<ResolveDropped, DropAction, _>(5, move |event| {
                    let url = event.item.as_url()?;
                    if url.starts_with("https://example.invalid/claimed/") {
                        seen_in_handler.lock().push(url.to_owned());
                        Some(DropAction::Handled)
                    } else {
                        None
                    }
                })
        })
    });

    visual.simulate_url_drop(
        point(px(200.), px(200.)),
        ["https://example.invalid/claimed/asset-42"],
    );
    assert_eq!(
        seen.lock().as_slice(),
        &["https://example.invalid/claimed/asset-42".to_owned()]
    );
    let status_after_claim = visual.read(|app| state.read(app).status_message.clone());
    assert_eq!(
        status_after_claim, None,
        "a claimed URL must not post an unclaimed notice"
    );
}
