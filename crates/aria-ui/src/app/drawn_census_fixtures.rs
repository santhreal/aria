//! Editor states dressed so a drawn control has something to act on.
//!
//! A census that only ever presses rows against one still image measures the
//! fixture, not the product: half the rows are offered on a clip with sound, a
//! second lane, a free pool item or a junction that already carries a blend. Each
//! state here exists because some row is only reachable from it, and each is
//! named after what it holds rather than after the row that wanted it.

use std::path::PathBuf;

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, MediaId, TimeRange};

use crate::app::AppState;
use crate::app::premiere_tests::{with_clip, with_media};

/// A file from the repository's fixture set.
pub(super) fn asset(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// A project holding the video fixture, selected: a clip with a picture and
/// sound, so a freeze frame has a frame to take and an audio extraction has
/// something to detach.
pub(super) fn with_video() -> (AppState, ClipId) {
    with_media(asset("fixture-640x360-24fps-3s-av.mp4"))
}

/// A project holding the audio fixture, selected: the clip lands on an audio
/// lane, which is the only lane a duck source can be.
pub(super) fn with_audio() -> (AppState, ClipId) {
    with_media(asset("fixture-sine-48k-2s.wav"))
}

/// A selection of two clips, linked, so the rows that join and separate a
/// selection are both offered.
pub(super) fn linked_pair() -> AppState {
    let (mut state, first) = with_video();
    let media = state
        .import_media(asset("fixture-sine-48k-2s.wav"))
        .expect("import the audio fixture");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place the audio clip");
    let second = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .find(|clip| *clip != first)
        .expect("a second clip");
    state.selection = vec![first, second];
    state.sync_from_engine();
    state.link_selected().expect("link the pair");
    state.sync_from_engine();
    state
}

/// A lane with a lane above it and a lane below it, so the rows that move a lane
/// through the stack have somewhere to move it, and the id of that middle lane.
pub(super) fn middle_lane() -> (AppState, u64) {
    let (mut state, _) = with_clip();
    for name in ["V2", "V3"] {
        state
            .add_track(aria_models::TrackKind::Video, name.to_string())
            .expect("a lane to move past");
    }
    state.sync_from_engine();
    let lanes: Vec<u64> = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|lane| matches!(lane.kind, crate::types::UiTrackKind::Video))
        .map(|lane| lane.id)
        .collect();
    let middle = lanes
        .get(1)
        .copied()
        .expect("three video lanes, so a middle one");
    (state, middle)
}

/// A pool item no clip holds, so the row that drops one from the project is
/// offered, and its id.
pub(super) fn free_pool_item() -> (AppState, u64) {
    let (mut state, _) = with_clip();
    let media = state
        .import_media(asset("fixture-sine-48k-2s.wav"))
        .expect("import a second pool item");
    state.sync_from_engine();
    (state, media.raw())
}

/// The source monitor open over a project that already has a lane, with a range
/// marked in the source, and the pool id it is showing.
///
/// The monitor is what the bar's rows act on, and every one of them wants a
/// range: a mark press moves an existing mark, and an edit press lands what the
/// marks name. The range is set by parking the monitor's own tick, because that
/// is what the In and Out buttons read.
pub(super) fn marked_source() -> (AppState, u64) {
    let (mut state, _) = with_video();
    let media = state
        .store
        .media
        .first()
        .expect("the video fixture is in the pool")
        .id;
    state.select_pool_item(media);
    assert!(state.toggle_source_monitor(), "the monitor opens");
    state.set_source_in();
    state.hover_pool_item(media, 12);
    state.set_source_out();
    (state, media)
}

/// The path of the first pool item, which every fixture here imports first.
pub(super) fn media_path(state: &AppState) -> PathBuf {
    PathBuf::from(&*state.store.media.first().expect("imported media").path)
}

/// Write detector records into the session's own index, as a run would.
///
/// Two censuses and the analysis tests all want moments to exist without running
/// a detector: a shot boundary to split at, a silent span to cut out, or a beat
/// grid to clear. Each span is `(start, end, confidence)` in seconds, and a point
/// event is a span whose ends are equal.
pub(super) fn store_moments(
    state: &AppState,
    kind: crate::app::analysis::AnalysisKind,
    spans: &[(f64, f64, f32)],
) -> aria_analysis::moments::MediaContentKey {
    use aria_analysis::moments::{
        MomentConfidence, MomentPayload, MomentRecord, MomentReplacementBatch,
    };
    let content_key = crate::app::analysis::content_key_for(&media_path(state))
        .expect("the fixture has a content key");
    let index = state.analysis_index().expect("open the index");
    let records: Vec<MomentRecord> = spans
        .iter()
        .enumerate()
        .map(|(index, (start, end, confidence))| {
            MomentRecord::new(
                content_key,
                aria_analysis::TimeSpan::new(*start, *end).expect("a valid span"),
                kind.moment_kind(),
                MomentConfidence::new(*confidence).expect("a valid confidence"),
                kind.analyzer_identity(),
                kind.analyzer_version(),
                Some(
                    MomentPayload::new(&format!("{{\"v\":1,\"index\":{index}}}"))
                        .expect("a valid payload"),
                ),
            )
        })
        .collect();
    let batch =
        MomentReplacementBatch::new(kind.batch_key(content_key), records).expect("a valid batch");
    index.replace_batch(&batch).expect("write the batch");
    content_key
}

/// A pool item already decoding through a proxy, and its id.
///
/// The proxy is a copy of the video fixture put where the naming rule says the
/// proxy for that source lives, so `use_media_proxy` registers the file on disk
/// instead of spawning a render: a census must never start an encode.
pub(super) fn proxied_pool_item() -> (AppState, u64) {
    let (mut state, _) = with_video();
    let media = state
        .store
        .media
        .first()
        .expect("the video fixture is in the pool")
        .id;
    let dest = state
        .proxy_file(MediaId::from_raw(media))
        .expect("a proxy destination");
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).expect("the proxies cache directory");
    }
    std::fs::copy(asset("fixture-640x360-24fps-3s.mp4"), &dest).expect("a proxy on disk");
    state
        .use_media_proxy(MediaId::from_raw(media))
        .expect("registering a proxy that is already on disk");
    state.sync_from_engine();
    (state, media)
}

/// Two clips abutting on one lane, the junction between them carrying a blend, so
/// the row that takes one off has one to take. Returns the clip on the left,
/// which is the one that owns the junction.
///
/// A blend needs a neighbour: a lone clip has no junction, and the model is inert
/// on a request to blend into nothing.
pub(super) fn blended_junction() -> (AppState, ClipId) {
    let (mut state, first) = with_clip();
    let lane = state
        .store
        .sequence
        .tracks
        .first()
        .expect("the fixture projects a lane");
    let track = aria_models::TrackId::from_raw(lane.id);
    let end = lane
        .clips
        .iter()
        .map(|clip| clip.timeline_end_ticks)
        .max()
        .expect("the lane holds the fixture clip");
    let media = state
        .store
        .media
        .first()
        .expect("the fixture imports one pool item")
        .id;
    state
        .drop_media_on_timeline(MediaId::from_raw(media), Some(track), end)
        .expect("place a neighbour to blend into");
    state.sync_from_engine();
    let blend = crate::app::clip_transitions::transition_choices()
        .first()
        .expect("the catalog offers a blend")
        .id
        .clone();
    state
        .set_clip_transition(first, &blend)
        .expect("apply a blend to the junction");
    state.sync_from_engine();
    assert!(
        state.clip_transition_id(first).is_some(),
        "the junction did not take a blend, so nothing here has one to remove"
    );
    (state, first)
}

/// A project remembered on the recents list, and the file it points at.
///
/// The file is real and stays on disk: the row that deletes one asks first, and a
/// census never answers.
pub(super) fn remembered_project() -> (AppState, PathBuf) {
    let (mut state, _) = with_clip();
    let path = crate::scratch_pool::scratch("menu-census").join("recent.aria");
    std::fs::write(&path, b"{}").expect("a scratch project file");
    state.remember_recent(&path);
    (state, path)
}

/// A 24 fps, 3 second file, which is what a render of a clip would look like.
pub(super) fn rendered_file() -> PathBuf {
    asset("fixture-640x360-24fps-3s.mp4")
}

/// The fixture clip, shortened to fit inside that file and given something for a
/// render to fold in. The project is left unsaved: where a render goes is
/// decided from the project's own name, and one caller asserts the refusal that
/// comes of having none.
pub(super) fn bakeable() -> (AppState, ClipId) {
    let (mut state, clip) = with_clip();
    let rate = state.playhead.rate;
    state
        .apply_command(Command::Edit(EditCommand::TrimClip {
            clip,
            timeline: TimeRange::at_rate(0, 48, rate),
        }))
        .expect("trim the clip to fit the rendered file");
    state
        .apply_command(Command::Edit(EditCommand::AddEffect {
            clip,
            effect_id: "gaussian_blur".into(),
        }))
        .expect("something for the render to fold in");
    state.sync_from_engine();
    (state, clip)
}

/// Where the render of `clip` would go, asked the same way the verb asks.
pub(super) fn bake_destination(state: &AppState, clip: ClipId) -> PathBuf {
    let pass = {
        let engine = state.engine.lock();
        aria_engine::bake_project(engine.project(), clip).expect("a bake pass")
    };
    state.bake_file(clip, &pass).expect("a destination")
}

/// A saved project whose selected clip already has its render on disk, so the
/// row that renders it adopts the file instead of starting an encode. A census
/// never spends a minute of GPU on a fixture.
pub(super) fn adopted_render(name: &str) -> (AppState, ClipId) {
    let (mut state, clip) = bakeable();
    let dir = crate::scratch_pool::scratch(name);
    state.project_path = Some(dir.join("adopted.aria"));
    let dest = bake_destination(&state, clip);
    std::fs::create_dir_all(dest.parent().expect("a parent")).expect("the renders directory");
    std::fs::copy(rendered_file(), &dest).expect("stand a render in");
    (state, clip)
}
