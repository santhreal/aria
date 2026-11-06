//! Sequence > Linked Selection.
//!
//! # Why
//!
//! Linked clips (a picture and its sound) select, move, and trim together while
//! linked selection is on. Turned off, each clip of the pair is its own clip:
//! a click, Shift+click, or single-lane Track Select selects one, a drag or a
//! nudge moves one, an edge drag trims one. The toggle is session state read at
//! every gesture, so these tests drive the gesture entry points
//! (`select_clip_linked`, `toggle_clip_in_selection`, `select_track_clips`,
//! `move_clip`, `trim_clip`, `trim_selected_edges`, `nudge_selected`,
//! `move_selection_by`) rather than the engine commands, and check both clips
//! of the pair afterwards. Grouped clips stay together with linked selection
//! off: a group is formed on purpose, a link by import. Every clip-selection
//! writer in `app/` is also enumerated from source to verify that selecting
//! clips clears any active edit-point (`selected_edge`) or gap (`selected_gap`)
//! selection.
//!
//! Not caught here: the pointer path from a drawn clip bar to these entry
//! points; the timeline gesture tests and the chord census in
//! `operator_surface_tests` drive those with the toggle in both states.

use std::collections::BTreeSet;
use std::path::Path;

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, LabelColor, MediaId, TrackId};

use super::AppState;
use super::fixtures::linked_pair;

/// The pair's two clips, picture first, and each one's timeline span.
fn pair(state: &AppState) -> [(ClipId, i64, i64); 2] {
    let spans: Vec<(ClipId, i64, i64)> = state
        .selection
        .iter()
        .map(|id| {
            let ui = state.ui_clip_ref(*id).expect("a clip of the pair");
            (*id, ui.timeline_start_ticks, ui.timeline_end_ticks)
        })
        .collect();
    [spans[0], spans[1]]
}

fn span(state: &AppState, clip: ClipId) -> (i64, i64) {
    let ui = state
        .ui_clip_ref(clip)
        .expect("the clip is on the timeline");
    (ui.timeline_start_ticks, ui.timeline_end_ticks)
}

#[test]
fn a_click_selects_the_pair_only_while_linked_selection_is_on() {
    let mut state = linked_pair();
    let [(picture, ..), (sound, ..)] = pair(&state);

    state.select_clip_linked(picture, true);
    assert_eq!(
        state.selection.len(),
        2,
        "on: the link selects with the clip"
    );

    state.toggle_linked_selection();
    assert!(!state.linked_selection);
    state.select_clip_linked(picture, true);
    assert_eq!(state.selection, vec![picture], "off: one clip");
    state.select_clip_linked(sound, true);
    assert_eq!(state.selection, vec![sound]);

    state.toggle_linked_selection();
    state.select_clip_linked(sound, true);
    assert_eq!(state.selection.len(), 2, "back on: the pair again");
}

#[test]
fn a_move_carries_the_link_only_while_linked_selection_is_on() {
    let mut state = linked_pair();
    let [(picture, p_start, _), (sound, s_start, _)] = pair(&state);
    let track = state.track_of(picture).expect("the picture's lane");

    state
        .move_clip(picture, track, p_start + 48)
        .expect("move on");
    assert_eq!(span(&state, picture).0, p_start + 48);
    assert_eq!(span(&state, sound).0, s_start + 48, "on: the sound follows");

    state.toggle_linked_selection();
    state
        .move_clip(picture, track, p_start + 96)
        .expect("move off");
    assert_eq!(span(&state, picture).0, p_start + 96);
    assert_eq!(span(&state, sound).0, s_start + 48, "off: the sound stays");
    assert!(state.undo(), "the solo move is one undo step");
    assert_eq!(span(&state, picture).0, p_start + 48);
    assert_eq!(span(&state, sound).0, s_start + 48);
}

#[test]
fn a_trim_carries_the_link_only_while_linked_selection_is_on() {
    let mut state = linked_pair();
    let [(picture, p_start, p_end), (sound, s_start, s_end)] = pair(&state);
    // Two frames off the tail: short enough that either clip survives it.
    let cut = 2;

    state
        .trim_clip(picture, p_start, p_end - cut)
        .expect("trim on");
    assert_eq!(span(&state, picture), (p_start, p_end - cut));
    assert_eq!(
        span(&state, sound),
        (s_start, s_end - cut),
        "on: the sound takes the same delta"
    );

    state.toggle_linked_selection();
    state
        .trim_clip(picture, p_start, p_end - 2 * cut)
        .expect("trim off");
    assert_eq!(span(&state, picture), (p_start, p_end - 2 * cut));
    assert_eq!(
        span(&state, sound),
        (s_start, s_end - cut),
        "off: the sound keeps its edge"
    );
}

#[test]
fn a_nudge_with_linked_selection_off_moves_only_the_selected_clip() {
    let mut state = linked_pair();
    let [(picture, p_start, _), (sound, s_start, _)] = pair(&state);
    state.toggle_linked_selection();
    state.select_clip_linked(picture, true);
    state.nudge_selected(5).expect("nudge");
    assert_eq!(span(&state, picture).0, p_start + 5);
    assert_eq!(span(&state, sound).0, s_start, "the sound stays");

    // The step chords move the whole selection by the same rule.
    state.move_selection_by(3).expect("step");
    assert_eq!(span(&state, picture).0, p_start + 8);
    assert_eq!(span(&state, sound).0, s_start);

    state.toggle_linked_selection();
    state.select_clip_linked(picture, true);
    state.move_selection_by(2).expect("step on");
    assert_eq!(span(&state, picture).0, p_start + 10);
    assert_eq!(span(&state, sound).0, s_start + 2, "on: the link steps too");
}

#[test]
fn a_group_moves_together_whatever_the_toggle() {
    let mut state = linked_pair();
    let [(picture, p_start, _), (sound, s_start, _)] = pair(&state);
    state.unlink_selected().expect("unlink the pair");
    state.group_selected().expect("group the pair");
    state.toggle_linked_selection();
    let track = state.track_of(picture).expect("the picture's lane");

    state.select_clip_linked(picture, true);
    assert_eq!(state.selection.len(), 2, "a click selects the group");

    state.move_clip(picture, track, p_start + 24).expect("move");
    assert_eq!(span(&state, picture).0, p_start + 24);
    assert_eq!(span(&state, sound).0, s_start + 24, "the group follows");

    state.move_selection_by(4).expect("step");
    assert_eq!(span(&state, picture).0, p_start + 28);
    assert_eq!(
        span(&state, sound).0,
        s_start + 28,
        "the step moves the group once, not once per member"
    );
}
#[test]
fn shift_click_and_track_select_follow_links_only_while_linked_selection_is_on() {
    use crate::tool_catalog::TrackSelectDir;

    let mut state = linked_pair();
    let [(picture, ..), (sound, ..)] = pair(&state);
    let v_track = state.track_of(picture).expect("picture lane");

    // Linked selection ON: Shift+click adds and removes both halves of the pair,
    // and Track Select Forward on the picture lane includes the linked sound.
    state.clear_selection();
    state.toggle_clip_in_selection(picture);
    assert_eq!(state.selection, vec![picture, sound]);
    state.toggle_clip_in_selection(picture);
    assert!(
        state.selection.is_empty(),
        "Shift+click on a selected linked clip must remove its companion too"
    );

    state.select_track_clips(v_track, 0, TrackSelectDir::Forward, false);
    assert_eq!(state.selection, vec![picture, sound]);

    // Linked selection OFF: Shift+click and single-lane Track Select take only
    // the clicked lane's clip.
    state.toggle_linked_selection();
    state.clear_selection();
    state.toggle_clip_in_selection(picture);
    assert_eq!(state.selection, vec![picture]);
    state.toggle_clip_in_selection(picture);
    assert!(state.selection.is_empty());

    state.select_track_clips(v_track, 0, TrackSelectDir::Forward, false);
    assert_eq!(
        state.selection,
        vec![picture],
        "with linked selection off, single-lane Track Select must not pull in linked companions"
    );

    // Explicit group: Shift+click and single-lane Track Select keep the group
    // together even while linked selection is off.
    state.select_all_unlocked();
    state.unlink_selected().expect("unlink");
    state.group_selected().expect("group");
    state.clear_selection();
    state.toggle_clip_in_selection(picture);
    assert_eq!(state.selection, vec![picture, sound]);
    state.toggle_clip_in_selection(picture);
    assert!(state.selection.is_empty());

    state.select_track_clips(v_track, 0, TrackSelectDir::Forward, false);
    assert_eq!(state.selection, vec![picture, sound]);
}

/// Verify the five timeline editing invariants on `state`:
/// 1. No two clips on a track overlap (`a.end <= b.start`).
/// 2. Every clip's source range stays inside `[0, media.duration]`.
/// 3. Selection (`selection`, `selected_edge`, `selected_gap`) never names a
///    deleted clip or track.
fn assert_timeline_invariants(state: &AppState) {
    let live_tracks: BTreeSet<u64> = state
        .store
        .sequence
        .tracks
        .iter()
        .map(|track| track.id)
        .collect();
    let mut live_clips = BTreeSet::new();
    for track in &state.store.sequence.tracks {
        for pair in track.clips.windows(2) {
            let [a, b] = pair else { continue };
            assert!(
                a.timeline_end_ticks <= b.timeline_start_ticks,
                "track {} has overlapping clips {} ({}..{}) and {} ({}..{})",
                track.id,
                a.id,
                a.timeline_start_ticks,
                a.timeline_end_ticks,
                b.id,
                b.timeline_start_ticks,
                b.timeline_end_ticks
            );
        }
        for clip in &track.clips {
            live_clips.insert(clip.id);
            if let Some(media_id) = clip.media_id
                && let Some(media) = state.store.media.iter().find(|m| m.id == media_id)
                && media.duration_ticks > 0
            {
                assert!(
                    clip.source_start_ticks >= 0
                        && clip.source_end_ticks >= clip.source_start_ticks
                        && clip.source_end_ticks <= media.duration_ticks,
                    "clip {} source range {}..{} exceeds media {} duration {}",
                    clip.id,
                    clip.source_start_ticks,
                    clip.source_end_ticks,
                    media_id,
                    media.duration_ticks
                );
            }
        }
    }
    for &clip in &state.selection {
        assert!(
            live_clips.contains(&clip.raw()),
            "selection names deleted clip {clip:?}"
        );
    }
    if let Some(edge) = state.selected_edge {
        assert!(
            live_clips.contains(&edge.clip),
            "selected_edge names deleted clip {}",
            edge.clip
        );
    }
    if let Some(gap) = state.selected_gap {
        assert!(
            live_tracks.contains(&gap.track),
            "selected_gap names deleted track {}",
            gap.track
        );
    }
}

#[test]
fn trim_selected_edges_moves_every_selected_clip_once_and_stops_at_zero() {
    let mut state = linked_pair();
    let [(first_pic, p_start, p_end), (first_snd, s_start, _)] = pair(&state);
    let v_track = state.track_of(first_pic).expect("picture lane");
    let media = MediaId::from_raw(
        state
            .ui_clip_ref(first_pic)
            .and_then(|clip| clip.media_id)
            .expect("media"),
    );
    // Shift the first pair forward by 24 ticks (preserving the L-cut offset
    // between `s_start` and `p_start`) and drop a second pair abutting `first_pic`.
    let len = p_end - p_start;
    state
        .move_clip(first_pic, v_track, p_start + 24)
        .expect("move first");
    let p0 = p_start + 24;
    let s0 = s_start + 24;
    let a_track = state.track_of(first_snd).expect("audio lane");
    state
        .drop_media_on_timeline(media, Some(v_track), p0 + len)
        .expect("drop second pair");
    let second_pic = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.id == v_track.raw())
        .and_then(|track| track.clips.iter().find(|c| c.id != first_pic.raw()))
        .map(|c| ClipId::from_raw(c.id))
        .expect("second picture clip");
    let second_snd = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.id == a_track.raw())
        .and_then(|track| track.clips.iter().find(|c| c.id != first_snd.raw()))
        .map(|c| ClipId::from_raw(c.id))
        .expect("second sound clip");
    state.select_clip_linked(first_pic, true);
    state.toggle_clip_in_selection(second_pic);
    assert_eq!(
        state.selection,
        vec![first_pic, first_snd, second_pic, second_snd]
    );

    // A zero-frame nudge is a no-op and must not claim the selection is at zero.
    state.status_message = None;
    let rev_before = state.engine().lock().revision();
    state.nudge_selected(0).expect("zero nudge");
    assert_eq!(state.engine().lock().revision(), rev_before);
    assert_eq!(state.status_message, None);

    // Ctrl+Right / Ctrl+Shift+Right (`trim_selected_edges`) moves both abutting
    // pairs in one undo step without colliding.
    let before_nudge =
        serde_json::to_value(state.engine().lock().project()).expect("serialize before");
    state.trim_selected_edges(12).expect("nudge both pairs +12");
    assert_timeline_invariants(&state);
    let after_nudge =
        serde_json::to_value(state.engine().lock().project()).expect("serialize after");
    assert_eq!(span(&state, first_pic), (p0 + 12, p0 + 12 + len));
    assert_eq!(span(&state, first_snd), (s0 + 12, s0 + 12 + len));
    assert_eq!(span(&state, second_pic), (p0 + len + 12, p0 + 2 * len + 12));
    assert_eq!(span(&state, second_snd), (p0 + len + 12, p0 + 2 * len + 12));

    assert!(state.undo(), "multi-clip nudge is one undo step");
    assert_timeline_invariants(&state);
    assert_eq!(
        serde_json::to_value(state.engine().lock().project()).expect("serialize undo"),
        before_nudge,
        "undo must restore the exact prior project"
    );
    assert!(state.redo(), "redo restores both pairs");
    assert_timeline_invariants(&state);
    assert_eq!(
        serde_json::to_value(state.engine().lock().project()).expect("serialize redo"),
        after_nudge,
        "redo must restore the exact post-command project"
    );

    // Nudging past the sequence start clamps to 0 preserving spacing, and a
    // further left nudge at 0 refuses without adding a no-op undo entry.
    state.trim_selected_edges(-100).expect("clamp to zero");
    assert_timeline_invariants(&state);
    assert_eq!(span(&state, first_snd), (0, len));
    assert_eq!(
        span(&state, first_pic),
        (p_start - s_start, p_start - s_start + len)
    );
    assert_eq!(
        span(&state, second_pic),
        (p_start - s_start + len, p_start - s_start + 2 * len)
    );

    let rev_at_zero = state.engine().lock().revision();
    state.trim_selected_edges(-5).expect("already at zero");
    assert_eq!(state.engine().lock().revision(), rev_at_zero);
    assert_eq!(
        state.status_message.as_deref(),
        Some("The selection is already at the sequence start")
    );
    assert!(state.undo(), "refused nudge at zero added no undo step");
    assert_timeline_invariants(&state);
    assert_eq!(
        serde_json::to_value(state.engine().lock().project()).expect("serialize after zero undo"),
        after_nudge
    );
}

fn clip_selection_writers_in_source() -> BTreeSet<String> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![src.join("app.rs")];
    let mut entries: Vec<_> = std::fs::read_dir(src.join("app"))
        .expect("read src/app")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("rs")
                && !path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with("_tests.rs") || n.contains("_tests_"))
        })
        .collect();
    entries.sort();
    files.extend(entries);

    let mutates_selection = |line: &str| {
        let trimmed = line.trim_start();
        trimmed
            .strip_prefix("self.selection =")
            .is_some_and(|rest| !rest.starts_with('='))
            || trimmed.starts_with("self.selection.clear(")
            || trimmed.starts_with("self.selection.push(")
            || trimmed.starts_with("self.selection.retain(")
            || trimmed.starts_with("self.selection.insert(")
            || trimmed.starts_with("self.selection.remove(")
            || trimmed.starts_with("self.selection.extend(")
            || trimmed.starts_with("self.selection.truncate(")
    };
    let fn_name = |line: &str| -> Option<String> {
        let trimmed = line.trim_start();
        for prefix in [
            "pub fn ",
            "pub(crate) fn ",
            "pub(super) fn ",
            "fn ",
            "pub async fn ",
            "async fn ",
        ] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let end = rest
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .unwrap_or(rest.len());
                if end > 0 {
                    return Some(rest[..end].to_owned());
                }
            }
        }
        None
    };

    let mut writers = BTreeSet::new();
    for path in files {
        let code = std::fs::read_to_string(&path).expect("read source");
        let lines: Vec<&str> = code
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect();
        let mut index = 0usize;
        while index < lines.len() {
            let Some(name) = fn_name(lines[index]) else {
                index += 1;
                continue;
            };
            let mut depth = 0i32;
            let mut seen_open = false;
            let mut body = String::new();
            while index < lines.len() {
                let line = lines[index];
                for ch in line.chars() {
                    match ch {
                        '{' => {
                            depth += 1;
                            seen_open = true;
                        }
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                body.push_str(line);
                body.push('\n');
                index += 1;
                if seen_open && depth <= 0 {
                    break;
                }
            }
            if body.lines().any(mutates_selection)
                && !body.contains("self.apply_command(")
                && !body.contains("self.host.replace_session(")
                && !matches!(
                    name.as_str(),
                    "clear_selection" | "select_edge" | "select_gap_at" | "each_peer"
                )
            {
                writers.insert(name);
            }
        }
    }
    writers
}

#[test]
fn every_clip_selection_writer_clears_selected_edge_and_selected_gap_and_skips_locked_clips() {
    use crate::tool_catalog::TrackSelectDir;
    use crate::types::TrimSide;

    type WriterStep = (
        &'static str,
        Box<dyn Fn(&mut AppState, TrackId, MediaId, ClipId, ClipId)>,
    );
    let steps: Vec<WriterStep> = vec![
        (
            "select_clip",
            Box::new(|state, _, _, clip_a, _| state.select_clip(clip_a)),
        ),
        (
            "select_track_clips",
            Box::new(|state, track, _, _, _| {
                state.select_track_clips(track, 0, TrackSelectDir::Forward, false);
            }),
        ),
        (
            "focus_or_select_clip",
            Box::new(|state, _, _, _, clip_b| state.focus_or_select_clip(clip_b, true)),
        ),
        (
            "toggle_clip_in_selection",
            Box::new(|state, _, _, _, clip_b| state.toggle_clip_in_selection(clip_b)),
        ),
        (
            "add_clip_to_selection",
            Box::new(|state, _, _, _, clip_b| state.add_clip_to_selection(clip_b)),
        ),
        (
            "select_at_playhead",
            Box::new(|state, _, _, _, _| {
                state.set_playhead(0);
                state.select_at_playhead();
            }),
        ),
        (
            "select_by_label",
            Box::new(|state, _, _, _, _| state.select_by_label(LabelColor::Rose)),
        ),
        (
            "commit_marquee",
            Box::new(|state, track, _, _, _| {
                state.begin_marquee(track.raw(), 0);
                state.drag_marquee(track.raw(), 48);
                assert!(state.commit_marquee() >= 1);
            }),
        ),
        (
            "select_media_clips",
            Box::new(|state, _, media, _, _| {
                assert!(state.select_media_clips(media) >= 1);
            }),
        ),
        (
            "select_clip_linked",
            Box::new(|state, _, _, clip_a, _| state.select_clip_linked(clip_a, true)),
        ),
        (
            "select_all_unlocked",
            Box::new(|state, _, _, _, _| state.select_all_unlocked()),
        ),
    ];

    let derived = clip_selection_writers_in_source();
    let covered: BTreeSet<String> = steps.iter().map(|(name, _)| (*name).to_owned()).collect();
    assert_eq!(
        covered, derived,
        "every clip-selection writer in `app/` must be covered here"
    );

    let (mut state, clip_a) = super::premiere_tests::with_clip();
    let track = TrackId::from_raw(state.store.sequence.tracks[0].id);
    let media = MediaId::from_raw(
        state.store.sequence.tracks[0].clips[0]
            .media_id
            .expect("fixture media"),
    );
    let end_a = span(&state, clip_a).1;
    state
        .drop_media_on_timeline(media, Some(track), end_a + 48)
        .expect("drop second clip after a gap");
    let clip_b = state.store.sequence.tracks[0]
        .clips
        .iter()
        .map(|clip| ClipId::from_raw(clip.id))
        .find(|&id| id != clip_a)
        .expect("second clip");
    state
        .apply_command(Command::Edit(EditCommand::SetClipLabel {
            clip: clip_a,
            label: Some(LabelColor::Rose),
        }))
        .expect("label clip_a");

    for (name, run) in &steps {
        state.select_edge(clip_a, TrimSide::Out);
        assert!(state.selected_edge.is_some());
        run(&mut state, track, media, clip_a, clip_b);
        assert!(
            state.selected_edge.is_none(),
            "{name} left `selected_edge` set after selecting clips"
        );

        let gap = state.select_gap_at(track, end_a + 1);
        assert!(gap.is_some(), "{name}: expected a gap after clip_a");
        run(&mut state, track, media, clip_a, clip_b);
        assert!(
            state.selected_gap.is_none(),
            "{name} left `selected_gap` set after selecting clips"
        );
    }

    // `select_all_unlocked` must skip a locked clip on an unlocked lane.
    std::sync::Arc::make_mut(&mut state.store).sequence.tracks[0]
        .clips
        .iter_mut()
        .find(|clip| clip.id == clip_b.raw())
        .expect("clip_b")
        .locked = true;
    state.select_all_unlocked();
    assert_eq!(
        state.selection,
        vec![clip_a],
        "select_all_unlocked included a locked clip"
    );
}
