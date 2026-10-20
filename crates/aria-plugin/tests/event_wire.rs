//! The event wire: what a native or WASM plugin receives for each kind it
//! subscribes to, and how its reply is read.
//!
//! The class closed here is "the two bridges disagree". Each bridge held its own
//! translation, and the two drifted: a WASM plugin received `"image-sequence"`
//! where a native one received `"image_sequence"`, `{}` where a native one
//! received the closed project's path, a drop payload with other fields, no
//! target lists on a proposed command, and a reply grammar without `rewrite`
//! or `claim_drop`. A path that was not UTF-8 panicked the handler in both.
//! Both bridges now call `event_wire::subscribe`, and these tests hold its
//! output to exact payloads and to the table in `docs/plugins/events.md`.
//!
//! A kind added to `WireKind` fails to compile here until `fire` has a sample
//! for it, and fails the chapter test until the table lists it.
//!
//! Not covered: the ABI calls into a plugin, which the native contract test and
//! the WASM end-to-end test exercise.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use aria_commands::{Command, EditCommand, ProjectCommand};
use aria_core::{Rational, RationalTime};
use aria_engine::ApplyOutcome;
use aria_models::{ClipId, MediaId, Project, TrackId};
use aria_plugin::event_wire::{self, Subscription, WireKind};
use aria_plugin::{
    CommandApplied, CommandProposed, DropAction, ExportFinished, ExportStarted, InOutChanged,
    JobFinished, JobOutcome, JobProgress, MediaImported, MediaOffline, MediaRelinked, Origin,
    PlaybackStarted, PlaybackStopped, PlayheadMoved, Plugin, PluginContext, PluginError,
    PluginManifest, ProjectClosed, ProjectOpened, ProjectSaved, RenderFinished, RenderKind,
    RenderResult, ResolveDropped, SelectionChanged, WaterfallResult,
};
use parking_lot::Mutex;
use serde_json::{Value, json};

const FLUSH: Duration = Duration::from_secs(10);

/// A plugin that subscribes through the wire while mounting, records each
/// delivery as `(kind, payload)`, and answers each with `reply`.
struct Listener {
    manifest: PluginManifest,
    subscriptions: Vec<String>,
    reply: Arc<Mutex<Option<String>>>,
    heard: Arc<Mutex<Vec<(String, Value)>>>,
}

impl Listener {
    fn new<S: Into<String>>(id: &'static str, subscriptions: impl IntoIterator<Item = S>) -> Self {
        Self {
            manifest: PluginManifest::new(id, "Listener", "0.1.0", &[], &[]),
            subscriptions: subscriptions.into_iter().map(Into::into).collect(),
            reply: Arc::default(),
            heard: Arc::default(),
        }
    }

    fn answering(self, reply: &str) -> Self {
        *self.reply.lock() = Some(reply.to_owned());
        self
    }

    fn take(&self) -> Vec<(String, Value)> {
        std::mem::take(&mut *self.heard.lock())
    }
}

impl Plugin for Listener {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        for raw in &self.subscriptions {
            let heard = Arc::clone(&self.heard);
            let reply = Arc::clone(&self.reply);
            event_wire::subscribe(ctx, raw, move |_, kind, payload| {
                let payload = serde_json::from_str(payload).expect("every payload is JSON");
                heard.lock().push((kind.to_owned(), payload));
                reply.lock().clone()
            })
            .map_err(|err| PluginError::MountFailed(self.manifest.id.clone(), err))?;
        }
        Ok(())
    }
}

fn mounted(listener: &Listener) -> PluginContext {
    let ctx = PluginContext::new();
    ctx.mount(listener).expect("the listener mounts");
    ctx
}

fn at(value: i64) -> RationalTime {
    RationalTime::new(value, Rational::FPS_24)
}

/// [`at`] as it appears on the wire, written out so the time encoding is held
/// too.
fn at_json(value: i64) -> Value {
    json!({ "value": value, "rate": { "num": 24, "den": 1 } })
}

fn project() -> Arc<Project> {
    Arc::new(Project::new("wire", Rational::FPS_24))
}

fn marker(name: &str) -> Command {
    Command::Edit(EditCommand::AddMarker {
        at: at(10),
        name: name.to_owned(),
        color: None,
    })
}

/// Fire the samples of `kind` on `ctx`, and return the payload each must
/// deliver, in firing order.
fn fire(ctx: &PluginContext, kind: WireKind) -> Vec<Value> {
    let bus = ctx.events();
    match kind {
        WireKind::CommandProposed => {
            let edit = EditCommand::MoveClip {
                clip: ClipId::from_raw(7),
                to_track: TrackId::from_raw(3),
                start: at(48),
            };
            let targets = edit.targets();
            assert!(
                !targets.clips.is_empty() && !targets.tracks.is_empty(),
                "a move names its clip and its lane"
            );
            let moved = Command::Edit(edit);
            let saved = Command::Project(ProjectCommand::Save {
                path: "/p/cut.aria".into(),
            });
            let expected = vec![
                json!({
                    "command": moved,
                    "origin": "plugin example.mover",
                    "target_tracks": targets.tracks,
                    "target_clips": targets.clips,
                }),
                json!({
                    "command": saved,
                    "origin": "the editor",
                    "target_tracks": [],
                    "target_clips": [],
                }),
            ];
            let origin = Origin::plugin("example.mover");
            bus.emit_waterfall(&mut CommandProposed::new(moved, project(), origin));
            bus.emit_waterfall(&mut CommandProposed::new(saved, project(), Origin::Editor));
            expected
        }
        WireKind::ResolveDropped => {
            for event in [
                ResolveDropped::path("/media/clip.MOV"),
                ResolveDropped::path("/media/README"),
                ResolveDropped::url("https://example.com/a.mov"),
            ] {
                let _: Option<DropAction> = bus.serial(&event);
            }
            vec![
                json!({ "kind": "path", "path": "/media/clip.MOV", "extension": "MOV" }),
                json!({ "kind": "path", "path": "/media/README", "extension": "" }),
                json!({ "kind": "url", "url": "https://example.com/a.mov" }),
            ]
        }
        WireKind::CommandApplied => {
            let command = marker("applied");
            let expected = json!({
                "command": command,
                "outcome": { "type": "Saved" },
                "revision": 9,
                "origin": "the assistant",
            });
            bus.post(CommandApplied {
                command,
                outcome: ApplyOutcome::Saved,
                revision: 9,
                origin: Origin::Assistant,
            });
            vec![expected]
        }
        WireKind::ProjectOpened => {
            bus.post(ProjectOpened {
                path: Some("/p/cut.aria".into()),
                project: project(),
            });
            bus.post(ProjectOpened {
                path: None,
                project: project(),
            });
            vec![json!({ "path": "/p/cut.aria" }), json!({ "path": null })]
        }
        WireKind::ProjectClosed => {
            bus.post(ProjectClosed {
                path: Some("/p/old.aria".into()),
            });
            bus.post(ProjectClosed { path: None });
            vec![json!({ "path": "/p/old.aria" }), json!({ "path": null })]
        }
        WireKind::ProjectSaved => {
            bus.post(ProjectSaved {
                path: "/p/cut.aria".into(),
            });
            vec![json!({ "path": "/p/cut.aria" })]
        }
        WireKind::SelectionChanged => {
            bus.post(SelectionChanged {
                clips: vec![ClipId::from_raw(7), ClipId::from_raw(2)],
            });
            vec![json!({ "clips": [7, 2] })]
        }
        WireKind::PlayheadMoved => {
            bus.post(PlayheadMoved { at: at(60) });
            vec![json!({ "at": at_json(60) })]
        }
        WireKind::InOutChanged => {
            bus.post(InOutChanged {
                mark_in: Some(at(12)),
                mark_out: None,
            });
            vec![json!({ "mark_in": at_json(12), "mark_out": null })]
        }
        WireKind::PlaybackStarted => {
            bus.post(PlaybackStarted {
                at: at(60),
                speed: -2,
            });
            vec![json!({ "at": at_json(60), "speed": -2 })]
        }
        WireKind::PlaybackStopped => {
            bus.post(PlaybackStopped { at: at(61) });
            vec![json!({ "at": at_json(61) })]
        }
        WireKind::MediaImported => {
            bus.post(MediaImported {
                media: MediaId::from_raw(4),
                path: "/media/a.mov".into(),
            });
            vec![json!({ "media": 4, "path": "/media/a.mov" })]
        }
        WireKind::MediaRelinked => {
            bus.post(MediaRelinked {
                media: MediaId::from_raw(4),
                path: "/media/b.mov".into(),
            });
            vec![json!({ "media": 4, "path": "/media/b.mov" })]
        }
        WireKind::MediaOffline => {
            bus.post(MediaOffline {
                media: MediaId::from_raw(5),
                path: "/media/c.mov".into(),
            });
            vec![json!({ "media": 5, "path": "/media/c.mov" })]
        }
        WireKind::RenderFinished => {
            bus.post(RenderFinished {
                kind: RenderKind::Clip(ClipId::from_raw(7)),
                path: "/r/clip.mov".into(),
                result: RenderResult::Written,
            });
            bus.post(RenderFinished {
                kind: RenderKind::ImageSequence,
                path: "/r/seq".into(),
                result: RenderResult::Failed("disk full".to_owned()),
            });
            vec![
                json!({
                    "kind": { "clip": 7 },
                    "path": "/r/clip.mov",
                    "result": { "status": "written" },
                }),
                json!({
                    "kind": "image-sequence",
                    "path": "/r/seq",
                    "result": { "status": "failed", "reason": "disk full" },
                }),
            ]
        }
        WireKind::ExportStarted => {
            bus.post(ExportStarted {
                path: "/e/out.mp4".into(),
            });
            vec![json!({ "path": "/e/out.mp4" })]
        }
        WireKind::ExportFinished => {
            bus.post(ExportFinished {
                path: "/e/out.mp4".into(),
                result: RenderResult::Cancelled,
            });
            vec![json!({ "path": "/e/out.mp4", "result": { "status": "cancelled" } })]
        }
        WireKind::JobProgress => {
            bus.post(JobProgress {
                job: 5,
                label: "Proxy".to_owned(),
                progress: Some(0.5),
                detail: "half".to_owned(),
            });
            bus.post(JobProgress {
                job: 6,
                label: "Scan".to_owned(),
                progress: None,
                detail: String::new(),
            });
            vec![
                json!({ "job": 5, "label": "Proxy", "progress": 0.5, "detail": "half" }),
                json!({ "job": 6, "label": "Scan", "progress": null, "detail": "" }),
            ]
        }
        WireKind::JobFinished => {
            let outcomes = [
                (JobOutcome::Done, "done"),
                (JobOutcome::Failed, "failed"),
                (JobOutcome::Cancelled, "cancelled"),
            ];
            outcomes
                .into_iter()
                .map(|(outcome, name)| {
                    bus.post(JobFinished {
                        job: 8,
                        label: "Render".to_owned(),
                        outcome,
                        detail: "end".to_owned(),
                    });
                    json!({ "job": 8, "label": "Render", "outcome": name, "detail": "end" })
                })
                .collect()
        }
    }
}

/// Every kind delivers its payload, and nothing else, under its wire name, whether the
/// plugin subscribed with the wire name or the event type name.
#[test]
fn every_kind_delivers_its_payload_under_its_wire_name() {
    let spellings: [fn(WireKind) -> &'static str; 2] = [WireKind::name, WireKind::event_type];
    for spelling in spellings {
        let listener = Listener::new(
            "example.listener",
            WireKind::ALL.iter().map(|k| spelling(*k)),
        );
        let ctx = mounted(&listener);
        for &kind in WireKind::ALL {
            let expected: Vec<(String, Value)> = fire(&ctx, kind)
                .into_iter()
                .map(|payload| (kind.name().to_owned(), payload))
                .collect();
            assert!(ctx.events().flush(FLUSH), "{kind:?} delivered in time");
            assert_eq!(
                listener.take(),
                expected,
                "{kind:?} subscribed as {:?}",
                spelling(kind)
            );
        }
    }
}

/// The table under "External plugins" lists each kind once, with its event
/// type and the fields its payloads carry, no more and no fewer.
#[test]
fn the_events_chapter_lists_every_kind_with_its_payload_fields() {
    let chapter = include_str!("../../../docs/plugins/events.md");
    let section = chapter
        .split_once("\n## External plugins\n")
        .expect("events.md has an External plugins section")
        .1;
    let rows: Vec<Vec<&str>> = section
        .lines()
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| line.trim_matches('|').split('|').map(str::trim).collect())
        .collect();
    let ticked = |cell: &str| -> Vec<String> {
        cell.split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    };

    let listed: Vec<String> = rows.iter().map(|row| ticked(row[0]).concat()).collect();
    let kinds: Vec<String> = WireKind::ALL.iter().map(|k| k.name().to_owned()).collect();
    assert_eq!(
        listed, kinds,
        "the table lists each kind once, in WireKind order"
    );

    let ctx = PluginContext::new();
    for (row, &kind) in rows.iter().zip(WireKind::ALL) {
        assert_eq!(ticked(row[1]), [kind.event_type()], "{kind:?} event column");
        let fields: BTreeSet<String> = fire(&ctx, kind)
            .iter()
            .flat_map(|payload| payload.as_object().expect("a payload is an object").keys())
            .cloned()
            .collect();
        let documented: BTreeSet<String> = ticked(row[2]).into_iter().collect();
        assert_eq!(documented, fields, "{kind:?} payload column");
        let replies = matches!(kind, WireKind::CommandProposed | WireKind::ResolveDropped);
        assert_eq!(row[3] != "Ignored", replies, "{kind:?} reply column");
    }
}

/// `subscribe` takes the wire name, the event type name, and the
/// `resolve-dropped-path` alias; only `resolve-dropped` takes a priority.
#[test]
fn a_subscription_parses_its_kind_and_priority() {
    for &kind in WireKind::ALL {
        let plain = Subscription { kind, priority: 0 };
        assert_eq!(Subscription::parse(kind.name()), Ok(plain));
        assert_eq!(Subscription::parse(kind.event_type()), Ok(plain));
        let ranked = Subscription::parse(&format!("{}:5", kind.name()));
        if kind == WireKind::ResolveDropped {
            assert_eq!(ranked, Ok(Subscription { kind, priority: 5 }));
        } else {
            let err = ranked.expect_err("only resolve-dropped takes a priority");
            assert!(err.contains("takes no priority"), "{err}");
        }
    }
    let dropped = |priority| {
        Ok(Subscription {
            kind: WireKind::ResolveDropped,
            priority,
        })
    };
    assert_eq!(Subscription::parse("resolve-dropped-path"), dropped(0));
    assert_eq!(Subscription::parse("ResolveDroppedPath"), dropped(0));
    assert_eq!(Subscription::parse("resolve-dropped-path: -3"), dropped(-3));
    assert_eq!(
        Subscription::parse("ResolveDropped:2147483647"),
        dropped(i32::MAX)
    );
    for bad in [
        "",
        "command_applied",
        "Command-Applied",
        "resolve-dropped:",
        "resolve-dropped:x",
        "resolve-dropped:2147483648",
    ] {
        assert!(Subscription::parse(bad).is_err(), "{bad:?} is refused");
    }
    let err = Subscription::parse("no-such-kind").expect_err("unknown kind");
    for &kind in WireKind::ALL {
        assert!(err.contains(kind.name()), "the error lists {kind:?}: {err}");
    }
}

/// A higher priority runs first whatever the mount order, the first claim
/// ends the chain, and the alias delivers under the wire name.
#[test]
fn a_dropped_item_goes_to_the_higher_priority_first() {
    let low = Listener::new("example.low", ["resolve-dropped"]).answering("\"import-media\"");
    let high = Listener::new("example.high", ["resolve-dropped-path:5"]).answering("handled");
    let ctx = mounted(&low);
    ctx.mount(&high).expect("the second listener mounts");

    let claimed: Option<DropAction> = ctx.events().serial(&ResolveDropped::path("/m/a.srt"));
    assert_eq!(claimed, Some(DropAction::Handled));
    assert!(low.take().is_empty(), "the claim ends the chain");
    let heard = high.take();
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].0, "resolve-dropped");
}

/// A reply to `command-proposed` refuses the command, replaces it, or leaves
/// it as proposed.
#[test]
fn a_reply_refuses_or_replaces_a_proposed_command() {
    let replacement = marker("replaced");
    let bare = serde_json::to_string(&replacement).expect("a command serializes");
    let kept = marker("proposed");
    let cases = [
        (
            json!({ "veto": "Locked lane" }).to_string(),
            Some("Locked lane"),
            &kept,
        ),
        (
            json!({ "refuse": "Too long" }).to_string(),
            Some("Too long"),
            &kept,
        ),
        (
            json!({ "rewrite": replacement }).to_string(),
            None,
            &replacement,
        ),
        (bare.clone(), None, &replacement),
        (format!("  {bare}\n"), None, &replacement),
        (String::new(), None, &kept),
        ("not json".to_owned(), None, &kept),
        ("null".to_owned(), None, &kept),
        (json!({ "rewrite": "nonsense" }).to_string(), None, &kept),
        (json!({ "veto": 3 }).to_string(), None, &kept),
    ];
    let listener = Listener::new("example.policy", ["command-proposed"]);
    let ctx = mounted(&listener);
    for (reply, veto, command) in cases {
        *listener.reply.lock() = Some(reply.clone());
        let mut proposed = CommandProposed::new(kept.clone(), project(), Origin::Editor);
        let result = ctx.events().emit_waterfall(&mut proposed);
        assert_eq!(proposed.veto.as_deref(), veto, "reply {reply:?}");
        assert_eq!(
            result == WaterfallResult::Stopped,
            veto.is_some(),
            "{reply:?}"
        );
        assert_eq!(&proposed.command, command, "reply {reply:?}");
    }
}

/// A reply to `resolve-dropped` claims in either spelling, bare, quoted, or
/// under `claim_drop`; any other reply claims nothing.
#[test]
fn a_reply_claims_a_dropped_item_in_every_spelling() {
    let actions = [
        (DropAction::OpenProject, "open-project", "OpenProject"),
        (DropAction::ImportMedia, "import-media", "ImportMedia"),
        (DropAction::ApplyLut, "apply-lut", "ApplyLut"),
        (DropAction::InstallSkill, "install-skill", "InstallSkill"),
        (
            DropAction::ImportTimeline,
            "import-timeline",
            "ImportTimeline",
        ),
        (DropAction::Handled, "handled", "Handled"),
    ];
    let listener = Listener::new("example.claims", ["resolve-dropped"]);
    let ctx = mounted(&listener);
    let claim = |reply: &str| -> Option<DropAction> {
        *listener.reply.lock() = Some(reply.to_owned());
        ctx.events()
            .serial(&ResolveDropped::url("https://example.com/x"))
    };
    for (action, wire, variant) in actions {
        for name in [wire, variant] {
            for reply in [
                name.to_owned(),
                format!(" {name}\n"),
                json!(name).to_string(),
                json!({ "claim_drop": name }).to_string(),
            ] {
                assert_eq!(claim(&reply), Some(action), "reply {reply:?}");
            }
        }
    }
    for reply in [
        "",
        "nope",
        "\"nope\"",
        "null",
        "[\"handled\"]",
        "{\"claim_drop\": 3}",
        "{\"handled\": true}",
    ] {
        assert_eq!(claim(reply), None, "reply {reply:?}");
    }
    *listener.reply.lock() = None;
    let unanswered: Option<DropAction> = ctx.events().serial(&ResolveDropped::path("/m/a"));
    assert_eq!(unanswered, None);
}

/// A path that is not UTF-8 reaches the plugin with U+FFFD in place of each
/// invalid sequence; a command that has no JSON form is not delivered, and
/// the chain goes on without it.
#[cfg(unix)]
#[test]
fn a_path_that_is_not_utf8_still_reaches_the_plugin() {
    use std::os::unix::ffi::OsStrExt;

    let path = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(b"/media/\xffclip.mov"));
    let shown = "/media/\u{fffd}clip.mov";
    let listener = Listener::new(
        "example.paths",
        ["media-imported", "resolve-dropped", "command-proposed"],
    );
    let ctx = mounted(&listener);

    ctx.events().post(MediaImported {
        media: MediaId::from_raw(1),
        path: path.clone(),
    });
    assert!(ctx.events().flush(FLUSH));
    let imported = json!({ "media": 1, "path": shown });
    assert_eq!(listener.take(), [("media-imported".to_owned(), imported)]);

    let _: Option<DropAction> = ctx.events().serial(&ResolveDropped::path(path.clone()));
    let dropped = json!({ "kind": "path", "path": shown, "extension": "mov" });
    assert_eq!(listener.take(), [("resolve-dropped".to_owned(), dropped)]);

    let import = Command::Project(ProjectCommand::Import { path });
    let mut proposed = CommandProposed::new(import.clone(), project(), Origin::Editor);
    let result = ctx.events().emit_waterfall(&mut proposed);
    assert_eq!(result, WaterfallResult::Completed);
    assert_eq!(proposed.command, import);
    assert!(
        listener.take().is_empty(),
        "a command with no JSON form is not sent"
    );
}
