//! The group-test harness: one fixture project, and the runner every
//! group's case table goes through.
//!
//! The class closed here is "a tool reaches the model with nothing driving
//! it". A registry row used to be enough to offer a tool to a provider, so
//! a row could ship with a validation arm that rejected every call, a remap
//! arm that dropped an id, or a refusal that named the wrong field, and the
//! suite stayed green.
//!
//! A case is data: the arguments a provider sends, the engine command they
//! must lower to, arguments that must be refused, and the fragment of the
//! refusal the model reads. [`check_group`] runs a group's cases and then
//! compares them against the group's registry rows in both directions, so a
//! row with no case and a case with no row are both red.
//!
//! What this does not catch: whether the engine accepts the lowered command.
//! That is `tests/agent_eval.rs` and `tests/engine_roundtrip.rs`, which
//! drive the same commands through a real engine.

use std::collections::HashMap;

use aria_commands::{Command, EditCommand};
use aria_models::{
    Generator, LinkId, Marker, MarkerColor, Mask, MaskKind, MediaSource, Project, Rational,
    RationalTime, Shape, TimeRange, TrackKind,
};

use crate::validate::{Lowered, QueuedWork, validate};
use crate::wire::WireCommand;

pub(super) const R24: Rational = Rational::FPS_24;

/// What a row's arguments must lower to.
///
/// Two shapes because lowering has two: almost every row becomes an engine
/// command, and a render becomes work the shell owns. A row that produced the
/// wrong kind would otherwise read as a passing case, because both kinds come
/// back from one call.
pub(super) enum Expect {
    /// The engine command those arguments must lower to.
    Edit(fn(&Fixture) -> EditCommand),
    /// The work the shell must be handed, for a row no turn can finish.
    Queued(fn(&Fixture) -> QueuedWork),
}

/// One tool row under test.
pub(super) struct ToolCase {
    /// The registry row this case exercises.
    pub tool: &'static str,
    /// Arguments as a provider sends them.
    pub args: fn(&Fixture) -> serde_json::Value,
    /// What those arguments must become.
    pub expect: Expect,
    /// Arguments the tool must refuse.
    pub invalid: fn(&Fixture) -> serde_json::Value,
    /// A fragment of the refusal the model gets back.
    pub refusal: &'static str,
}

/// A 24 fps project wide enough to validate every tool against:
///
/// - `V1` (the main lane): `clip` 0-10s with a gaussian_blur + vignette
///   chain and a crossfade at its right cut, then `next` 10-15s abutting it,
///   carrying a circle mask over a rectangle mask.
/// - `V2`: empty, so placements and moves have somewhere to land.
/// - `V3`: `baked` 30-35s, a clip a render already replaced, so a restore has
///   something to put back and a second render has something to refuse.
/// - `A1`: `sound` 20-25s, leaving `clip`'s range free for extract_audio.
/// - `Titles`: `title` 2-5s, linked to `shape` on `Shapes` 0-3s.
/// - `Bus 1`: a submix bus, with `A1` sending to it at unity so a send has an
///   index to address and the main lane still has none.
/// - One marker at 2s.
pub(super) struct Fixture {
    pub project: Project,
    pub media: u64,
    /// The main video lane, which cannot be removed.
    pub main: u64,
    pub overlay: u64,
    pub audio: u64,
    /// The submix bus routing destinations point at.
    pub bus: u64,
    pub text: u64,
    pub clip: u64,
    pub next: u64,
    /// A clip standing in for its own render.
    pub baked: u64,
    pub sound: u64,
    pub title: u64,
    pub marker: u64,
}

impl Fixture {
    pub fn new() -> Self {
        let mut project = Project::new("tool-registry", R24);
        let media = project.add_media(MediaSource::new(
            "/tmp/aria-tool-registry.mp4",
            1920,
            1080,
            R24,
            60 * 24,
            true,
        ));
        let main = project.add_track(TrackKind::Video, "V1");
        let overlay = project.add_track(TrackKind::Video, "V2");
        let audio = project.add_track(TrackKind::Audio, "A1");
        let text = project.add_track(TrackKind::Text, "Titles");
        let sticker = project.add_track(TrackKind::Sticker, "Shapes");
        let bus = project.add_track(TrackKind::Submix, "Bus 1");
        let renders = project.add_track(TrackKind::Video, "V3");
        project
            .timeline_mut()
            .add_send(audio, bus, 1.0, false)
            .expect("an audio lane may send to a bus");

        let clip = project
            .add_clip(
                main,
                media,
                TimeRange::at_rate(0, 240, R24),
                RationalTime::new(0, R24),
            )
            .expect("clip fits the empty main lane");
        let next = project
            .add_clip(
                main,
                media,
                TimeRange::at_rate(240, 120, R24),
                RationalTime::new(240, R24),
            )
            .expect("second clip abuts the first");
        let sound = project
            .add_clip(
                audio,
                media,
                TimeRange::at_rate(480, 120, R24),
                RationalTime::new(480, R24),
            )
            .expect("audio-lane clip clears the extract_audio range");
        let title = project
            .add_generated(
                text,
                Generator::text("INTRO"),
                TimeRange::at_rate(48, 72, R24),
            )
            .expect("title fits the text lane");
        let shape = project
            .add_generated(
                sticker,
                Generator::shape(Shape::Rectangle, [0, 0, 0, 255]),
                TimeRange::at_rate(0, 72, R24),
            )
            .expect("shape fits the sticker lane");

        for effect in ["gaussian_blur", "vignette"] {
            project.add_effect(clip, effect).expect("catalog effect");
        }
        project
            .add_transition(clip, "crossfade")
            .expect("clip abuts next");
        // A two-mask stack on the clip after it, so the mask-stack rows have an
        // index to address and a second position to move to.
        for kind in [MaskKind::Circle, MaskKind::Rectangle] {
            project
                .add_clip_mask(next, Mask::new(kind))
                .expect("a video clip takes masks");
        }

        // A clip that has already been through a render: dressed so a render was
        // worth making, then swapped for a pool item covering its whole span.
        let baked = project
            .add_clip(
                renders,
                media,
                TimeRange::at_rate(0, 120, R24),
                RationalTime::new(720, R24),
            )
            .expect("a lane of its own has room");
        let rendered = project.add_media(MediaSource::new(
            "/tmp/aria-tool-registry-render.mp4",
            1920,
            1080,
            R24,
            120,
            true,
        ));
        project
            .set_clip_denoise(baked, true)
            .expect("something a render would fold in");
        project
            .bake_clip(baked, rendered)
            .expect("the render covers the clip it stands in for");

        // One link group that is not the clip under test: extract_audio
        // refuses an already-linked clip, and unlink_clips needs a member.
        let link = LinkId::next();
        for id in [title, shape] {
            project
                .timeline_mut()
                .clip_mut(id)
                .expect("generated clip")
                .link = Some(link);
        }

        let marker = project
            .timeline_mut()
            .add_marker(Marker::new(
                RationalTime::new(48, R24),
                "beat",
                MarkerColor::Teal,
            ))
            .expect("marker sits at a valid tick");

        Self {
            media: media.raw(),
            main: main.raw(),
            overlay: overlay.raw(),
            audio: audio.raw(),
            bus: bus.raw(),
            text: text.raw(),
            clip: clip.raw(),
            next: next.raw(),
            baked: baked.raw(),
            sound: sound.raw(),
            title: title.raw(),
            marker: marker.raw(),
            project,
        }
    }

    /// The model's view of a fixture clip, for expectations that start from
    /// the clip's current state (transform, crop, adjustments, motion blur).
    pub fn clip_state(&self, raw: u64) -> &aria_models::Clip {
        self.project
            .clip(aria_models::ClipId::from_raw(raw))
            .expect("fixture clip")
    }

    pub fn clip_id(&self, raw: u64) -> aria_models::ClipId {
        aria_models::ClipId::from_raw(raw)
    }

    pub fn track_id(&self, raw: u64) -> aria_models::TrackId {
        aria_models::TrackId::from_raw(raw)
    }

    pub fn marker_id(&self, raw: u64) -> aria_models::MarkerId {
        aria_models::MarkerId::from_raw(raw)
    }

    pub fn media_id(&self, raw: u64) -> aria_models::MediaId {
        aria_models::MediaId::from_raw(raw)
    }

    /// A timeline position in whole frames at the fixture rate.
    pub fn at(&self, seconds: f64) -> RationalTime {
        RationalTime::new((seconds * 24.0).round() as i64, R24)
    }
}

/// Run a group's cases, then check them against the group's registry rows in
/// both directions.
pub(super) fn check_group(group: &str, cases: &[ToolCase]) {
    let fixture = Fixture::new();
    let rows = super::super::tool_groups()
        .iter()
        .find(|candidate| candidate.name == group)
        .unwrap_or_else(|| panic!("{group} is not a registry group"))
        .tools;

    for case in cases {
        assert!(
            rows.contains(&case.tool),
            "{group} has a case for {}, which is not one of its rows",
            case.tool
        );
        check(case, &fixture);
    }
    for tool in rows {
        assert!(
            cases.iter().any(|case| case.tool == *tool),
            "registry row {tool} in group {group} has no test case"
        );
    }
}

fn check(case: &ToolCase, fixture: &Fixture) {
    let tool = case.tool;
    let decoded = WireCommand::from_tool_call(tool, (case.args)(fixture))
        .unwrap_or_else(|e| panic!("{tool}: valid arguments were refused: {e}"));
    assert_eq!(
        decoded.tool_name(),
        tool,
        "{tool} decoded to a command that reports another tool name"
    );

    let lowered = validate(&decoded, &fixture.project)
        .unwrap_or_else(|e| panic!("{tool}: valid arguments were rejected: {e}"));
    match (&case.expect, lowered) {
        (Expect::Edit(expected), Lowered::Edit(Command::Edit(edit))) => assert_eq!(
            edit,
            expected(fixture),
            "{tool} lowered to the wrong engine command"
        ),
        (Expect::Queued(expected), Lowered::Queued(work)) => assert_eq!(
            work,
            expected(fixture),
            "{tool} handed the shell the wrong work"
        ),
        (_, other) => {
            panic!("{tool} lowered to {other:?}, which is not the kind of thing its case expects")
        }
    }

    check_remap(&decoded);

    let refusal = refuse(fixture, tool, (case.invalid)(fixture));
    assert!(
        refusal.contains(case.refusal),
        "{tool}: refusal {refusal:?} does not name {:?}",
        case.refusal
    );
}

/// The refusal a call earns, from whichever gate stops it: argument decode
/// or validation. Accepting the call is the failure.
fn refuse(fixture: &Fixture, tool: &'static str, arguments: serde_json::Value) -> String {
    match WireCommand::from_tool_call(tool, arguments) {
        Err(message) => message,
        Ok(command) => match validate(&command, &fixture.project) {
            Err(rejection) => rejection.message,
            Ok(accepted) => panic!("{tool}: invalid arguments were accepted as {accepted:?}"),
        },
    }
}

/// Plan replay rewrites sandbox ids onto live ones, so every id a command
/// carries must follow its own map and nothing else may move. Each kind gets
/// a distinct offset: an arm that reads a clip id through the track map
/// leaves the value unchanged and fails here.
fn check_remap(command: &WireCommand) {
    const CLIP_OFFSET: u64 = 1_000;
    const TRACK_OFFSET: u64 = 2_000;
    const MARKER_OFFSET: u64 = 3_000;

    let json = serde_json::to_value(command).expect("wire commands serialize");
    let fields = json.as_object().expect("wire commands are tagged objects");

    let mut clip_map = HashMap::new();
    let mut track_map = HashMap::new();
    let mut marker_map = HashMap::new();
    let mut expected = fields.clone();

    for (field, value) in fields {
        let (map, offset) = match field.as_str() {
            "clip" | "clips" => (&mut clip_map, CLIP_OFFSET),
            "track" | "to_track" => (&mut track_map, TRACK_OFFSET),
            "marker" => (&mut marker_map, MARKER_OFFSET),
            _ => continue,
        };
        let mut shift = |id: &serde_json::Value| -> serde_json::Value {
            let id = id.as_u64().expect("id fields are unsigned integers");
            map.insert(id, id + offset);
            serde_json::json!(id + offset)
        };
        let shifted = match value {
            serde_json::Value::Array(ids) => {
                serde_json::Value::Array(ids.iter().map(&mut shift).collect())
            }
            id => shift(id),
        };
        expected.insert(field.clone(), shifted);
    }

    let mut remapped = command.clone();
    remapped.remap_ids(&clip_map, &track_map, &marker_map);
    assert_eq!(
        serde_json::to_value(&remapped).expect("wire commands serialize"),
        serde_json::Value::Object(expected),
        "{} does not remap every id it carries",
        command.tool_name()
    );
}
