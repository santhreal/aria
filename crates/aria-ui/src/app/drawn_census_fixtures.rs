//! Editor states dressed so a drawn control has something to act on.
//!
//! A census that only ever presses rows against one still image measures the
//! fixture, not the product: half the rows are offered on a clip with sound, a
//! second lane, a free pool item or a junction that already carries a blend. Each
//! state here exists because some row is only reachable from it, and each is
//! named after what it holds rather than after the row that wanted it.

use std::path::PathBuf;

use aria_commands::{Command, EditCommand, EditOutcome, SoundPlacement};
use aria_engine::ApplyOutcome;
use aria_models::{
    ClipId, ClipParam, Easing, MediaId, ParamValue, RationalTime, SpatialTangents, TimeRange,
    TrackId,
};
use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::app::mask_path::tests::masked_path;
use crate::app::premiere_tests::{with_clip, with_media};
use crate::surfaces::context_menu::{KeyframeMenu, MaskPathKeyMenu};
use crate::types::ContextTarget;

/// A file from the repository's fixture set.
pub(crate) fn asset(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// A project holding the video fixture, selected, its sound held inside the
/// picture clip the way a plugin's edit or an older project places it: a clip
/// with a picture and sound, so a freeze frame has a frame to take and an
/// audio extraction has something to detach. A drop puts the sound on a lane
/// of its own instead.
pub(super) fn with_video() -> (AppState, ClipId) {
    crate::test_support::catalogs::assert_catalogs_only_grew();
    let mut state = AppState::new(
        PluginContext::new(),
        crate::scratch_pool::scratch("census-video"),
    )
    .expect("state");
    state.new_project().expect("project");
    let media = state
        .import_media(asset("fixture-640x360-24fps-3s-av.mp4"))
        .expect("import the video fixture");
    let source = state
        .engine
        .lock()
        .project()
        .media(media)
        .map(|item| item.full_range())
        .expect("the video fixture is in the pool");
    let picture = TrackId::from_raw(state.store.sequence.tracks[0].id);
    let outcome = state
        .apply_command(Command::Edit(EditCommand::SourceEdit {
            media,
            source,
            at: RationalTime::new(0, state.playhead.rate),
            fill: None,
            ripple: false,
            track: Some(picture),
            sound: SoundPlacement::InPicture,
        }))
        .expect("place the video fixture");
    let ApplyOutcome::Edited(EditOutcome::Created(clip)) = outcome else {
        panic!("placing the video fixture produced {outcome:?}, not a clip");
    };
    state.select_clip(clip);
    (state, clip)
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

/// Two clips over the same span on two video lanes, the lower one selected, so
/// the Parent row offers the upper one. Returns the lower clip and the upper.
pub(super) fn stacked_pair() -> (AppState, ClipId, ClipId) {
    let (mut state, lower) = with_clip();
    let media = MediaId::from_raw(
        state
            .store
            .media
            .first()
            .expect("the fixture's pool item")
            .id,
    );
    state
        .add_track(aria_models::TrackKind::Video, "V2")
        .expect("a second lane");
    state.sync_from_engine();
    let lane = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|lane| matches!(lane.kind, crate::types::UiTrackKind::Video) && lane.clips.is_empty())
        .map(|lane| aria_models::TrackId::from_raw(lane.id))
        .expect("an empty video lane is projected");
    // Overwrite: a ripple drop opens a hole on every sync-locked lane, which
    // would push the lower clip past the end of the upper one.
    let mode = std::mem::replace(&mut state.edit_mode, crate::types::EditMode::Overwrite);
    state
        .drop_media_on_timeline(media, Some(lane), 0)
        .expect("place the upper clip");
    state.edit_mode = mode;
    let upper = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .find(|clip| *clip != lower)
        .expect("the upper clip");
    state.select_clip(lower);
    (state, lower, upper)
}

/// [`stacked_pair`] with the lower clip parented to the upper one and
/// selected, so the row that releases a parent has one to release.
pub(super) fn parented_pair() -> (AppState, ClipId) {
    let (mut state, lower, upper) = stacked_pair();
    state
        .set_clip_parent(lower, Some(upper), aria_models::ParentKeep::Place)
        .expect("the upper clip parents the lower one");
    state.select_clip(lower);
    (state, lower)
}

/// Key `param` of the selected clip at `tick` frames into the clip to `value`,
/// linear, with `tangents` shaping a motion path through the keyframe.
pub(super) fn key_point(
    state: &mut AppState,
    param: ClipParam,
    tick: i64,
    value: [f32; 2],
    tangents: Option<SpatialTangents>,
) {
    let clip = state.selected_clip().expect("a selected clip to key");
    let start = state
        .selected_model()
        .expect("the selected clip is in the project")
        .timeline
        .start;
    state
        .apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip,
            param,
            at: RationalTime::new(start.value + tick, start.rate),
            value: ParamValue::Vec2(value),
            easing: Easing::Linear,
            tangents,
        }))
        .expect("a keyframe inside the selected clip");
}

/// The selected clip's Position keyed three times, twelve frames apart: a
/// corner, a smooth keyframe, and a corner. A motion path is drawn with
/// keyframes to drag, tangents to bend, and segments to add a keyframe on.
pub(crate) fn keyed_path(state: &mut AppState) {
    let smooth = SpatialTangents {
        in_t: [-0.08, 0.0],
        out_t: [0.08, 0.0],
    };
    for (tick, value, tangents) in [
        (0, [-0.25, 0.0], None),
        (12, [0.0, 0.2], Some(smooth)),
        (24, [0.25, 0.0], None),
    ] {
        key_point(state, ClipParam::Position, tick, value, tangents);
    }
}

/// The timeline tick `frames` into the selected clip.
pub(crate) fn into_selected(state: &AppState, frames: i64) -> i64 {
    let start = state
        .selected_model()
        .expect("a selected clip in the project")
        .timeline
        .start;
    start.value + frames
}

/// Give the selected clip a path mask keyed at the clip's first frame and
/// twelve frames in, both linear.
pub(crate) fn keyed_mask_path(state: &mut AppState) {
    let clip = state.selected_clip().expect("a selected clip to mask");
    masked_path(state);
    let first = into_selected(state, 0);
    state.set_playhead(first);
    state
        .animate_mask_path(clip, 0)
        .expect("the Path stopwatch keys the first frame");
    let later = into_selected(state, 12);
    state.set_playhead(later);
    state
        .key_mask_path(clip, 0, false)
        .expect("a Path keyframe twelve frames in");
}

/// A still with Position keyed by [`keyed_path`] and a menu opened on its
/// middle keyframe, then `then` run with the clip and the keyframe's tick.
/// Returns the state and the facts the menu reads from it.
pub(super) fn keyframe_menu(then: fn(&mut AppState, ClipId, i64)) -> (AppState, KeyframeMenu) {
    let (mut state, clip) = with_clip();
    keyed_path(&mut state);
    let tick = into_selected(&state, 12);
    let (param, raw) = (ClipParam::Position, clip.raw());
    let target = ContextTarget::Keyframe {
        clip: raw,
        param,
        tick,
    };
    state.open_context(target, 0.0, 0.0);
    then(&mut state, clip, tick);
    let facts = KeyframeMenu::of(&state, raw, param, tick).expect("the Position row draws it");
    (state, facts)
}

/// A still with a path keyed by [`keyed_mask_path`] and a menu opened on its
/// first Path keyframe, then `then` run with the clip and the keyframe's tick.
/// Returns the state and the facts the menu reads from it.
pub(super) fn path_key_menu(then: fn(&mut AppState, ClipId, i64)) -> (AppState, MaskPathKeyMenu) {
    let (mut state, clip) = with_clip();
    keyed_mask_path(&mut state);
    let tick = into_selected(&state, 0);
    let raw = clip.raw();
    let target = ContextTarget::MaskPathKey {
        clip: raw,
        mask: 0,
        tick,
    };
    state.open_context(target, 0.0, 0.0);
    then(&mut state, clip, tick);
    let facts = MaskPathKeyMenu::of(&state, raw, 0, tick).expect("the Path row draws it");
    (state, facts)
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
    blend_right_junction(&mut state, first);
    (state, first)
}

/// Place a neighbour against the right edge of `first`, the only clip on the
/// first lane, and apply the catalog's first blend to the junction between
/// them, for a test whose state is already open in a drawn window.
pub(crate) fn blend_right_junction(state: &mut AppState, first: ClipId) {
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
        .expect("place a neighbor to blend into");
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

/// Three clips over the same span on three video lanes, each a quarter of the
/// frame and placed apart and unevenly, all selected, bottom to top: something
/// for every Align button to move and a middle clip for Distribute to move.
pub(super) fn spread_three() -> (AppState, [ClipId; 3]) {
    let (mut state, lower, upper) = stacked_pair();
    let media = MediaId::from_raw(
        state
            .store
            .media
            .first()
            .expect("the fixture's pool item")
            .id,
    );
    state
        .add_track(aria_models::TrackKind::Video, "V3")
        .expect("a third lane");
    state.sync_from_engine();
    let lane = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|lane| matches!(lane.kind, crate::types::UiTrackKind::Video) && lane.clips.is_empty())
        .map(|lane| aria_models::TrackId::from_raw(lane.id))
        .expect("an empty video lane is projected");
    let mode = std::mem::replace(&mut state.edit_mode, crate::types::EditMode::Overwrite);
    state
        .drop_media_on_timeline(media, Some(lane), 0)
        .expect("place the top clip");
    state.edit_mode = mode;
    let top = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .find(|clip| *clip != lower && *clip != upper)
        .expect("the top clip");
    let clips = [lower, upper, top];
    for (clip, position) in clips
        .into_iter()
        .zip([[-0.3, -0.2], [0.05, 0.1], [0.35, 0.25]])
    {
        state
            .apply_command(Command::Edit(EditCommand::SetClipTransform {
                clip,
                transform: aria_models::ClipTransform {
                    position,
                    scale: aria_models::Scale2::uniform(0.25),
                    ..aria_models::ClipTransform::IDENTITY
                },
                at: None,
            }))
            .expect("place a clip");
    }
    state.selection = clips.to_vec();
    state.sync_from_engine();
    (state, clips)
}

/// The fixture's clip carrying a Gaussian Blur with its radius turned up from
/// the default, so a reset has a value to put back and a copy has a value to
/// carry.
pub(super) fn blurred_clip() -> (AppState, ClipId) {
    let (mut state, clip) = with_clip();
    state
        .apply_command(Command::Edit(EditCommand::AddEffect {
            clip,
            effect_id: "gaussian_blur".into(),
        }))
        .expect("add a blur");
    state
        .apply_command(Command::Edit(EditCommand::SetEffectParam {
            clip,
            index: 0,
            param: 0,
            value: 12.0,
        }))
        .expect("turn the radius up");
    state.sync_from_engine();
    (state, clip)
}

/// The selected clip copied onto a new lane of its kind over the same span,
/// and both selected, the copy second: a clip beside the drawn one that holds
/// every value, effect, keyframe and driver it does, so an edit in Effect
/// Controls has a second clip to reach. A clip with a clip abutting it on the
/// right takes a copy of that clip too, and of the transition between them, so
/// the copy owns a junction as the drawn clip does. Returns the drawn clip and
/// the copy.
pub(super) fn twin_selected(state: &mut AppState) -> (ClipId, ClipId) {
    let drawn = state.selected_clip().expect("the dressing selects a clip");
    let (kind, neighbour, blend) = {
        let engine = state.engine().lock();
        let project = engine.project();
        let timeline = project.timeline();
        let track = timeline
            .track_of(drawn)
            .and_then(|id| timeline.track(id))
            .expect("the drawn clip is on a lane");
        let end = timeline
            .clip(drawn)
            .expect("the drawn clip")
            .timeline
            .end_tick();
        let neighbour = track
            .clips()
            .find(|clip| clip.id != drawn && clip.timeline.start.value == end)
            .map(|clip| clip.id);
        (track.kind, neighbour, project.transition_at(drawn).cloned())
    };
    state.add_track(kind, "Twin").expect("a lane for the copy");
    state.sync_from_engine();
    let lane = {
        let engine = state.engine().lock();
        engine
            .project()
            .timeline()
            .tracks_ordered()
            .find(|track| track.kind == kind && track.is_empty())
            .map(|track| track.id)
            .expect("an empty lane of the drawn clip's kind")
    };
    for clip in neighbour.into_iter().chain([drawn]) {
        state.select_clip(clip);
        state
            .duplicate_selection_to_lane(lane.raw())
            .expect("the copy lands over the same span");
    }
    let start = model_of(state, drawn).timeline.start;
    let twin = {
        let engine = state.engine().lock();
        engine
            .project()
            .timeline()
            .track(lane)
            .and_then(|track| track.clips().find(|clip| clip.timeline.start == start))
            .map(|clip| clip.id)
            .expect("the copy is on its lane")
    };
    if let Some(blend) = blend {
        state
            .apply_command(Command::Edit(EditCommand::AddTransition {
                clip: twin,
                transition_id: blend.transition_id.clone(),
            }))
            .expect("the copy's junction takes the drawn clip's transition");
        state
            .apply_command(Command::Edit(EditCommand::SetTransitionState {
                clip: twin,
                transition: blend,
            }))
            .expect("the copy's transition takes the drawn one's settings");
        state.sync_from_engine();
    }
    state.select_clip(drawn);
    state.add_clip_to_selection(twin);
    (drawn, twin)
}

/// `clip` as the model holds it.
pub(super) fn model_of(state: &AppState, clip: ClipId) -> aria_models::Clip {
    state
        .engine()
        .lock()
        .project()
        .timeline()
        .clip(clip)
        .cloned()
        .expect("the clip is in the project")
}

/// One side of [`the_pair`]: a clip as the model holds it and the transition
/// at its right junction.
pub(super) type Side = (aria_models::Clip, Option<aria_models::Transition>);

/// The models of `drawn` and `twin` and the transitions at their junctions,
/// with what a copy onto another lane changes put back to the drawn clip's:
/// its id, its place, the clips its transition joins, and its template fill
/// order, which marking several clips as slots numbers one by one. Two clips an
/// edit in Effect Controls left alike compare equal.
pub(super) fn the_pair(state: &AppState, drawn: ClipId, twin: ClipId) -> (Side, Side) {
    let engine = state.engine().lock();
    let project = engine.project();
    let side = |clip: ClipId| -> Side {
        let model = project
            .timeline()
            .clip(clip)
            .cloned()
            .expect("the clip is in the project");
        (model, project.transition_at(clip).cloned())
    };
    let (drawn, (mut twin, mut twin_blend)) = (side(drawn), side(twin));
    twin.id = drawn.0.id;
    twin.timeline = drawn.0.timeline;
    if let (Some(copy), Some(own)) = (twin.replaceable.as_mut(), drawn.0.replaceable.as_ref()) {
        copy.order = own.order;
    }
    if let (Some(copy), Some(own)) = (twin_blend.as_mut(), drawn.1.as_ref()) {
        copy.left = own.left;
        copy.right = own.right;
    }
    (drawn, (twin, twin_blend))
}
