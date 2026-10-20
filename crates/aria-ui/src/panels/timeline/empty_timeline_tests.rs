//! Regression tests for timeline virtualization, split V/A lanes, linked clip
//! sync, ruler/zoom/trim feedback, Track Select tools, edge selection, gap
//! selection, and empty-timeline interactions.

use crate::app::AppState;
use crate::panels::timeline::clip::VisibleLane;
use crate::panels::timeline::clip::hit::{HitTarget, hit_test};
use crate::panels::timeline::media::visible_clips_slice;
use crate::panels::timeline::overview::coalesce_overview_spans;
use crate::panels::timeline::{BASE_PPS, ticks_to_px};
use crate::surfaces::timeline_gesture::{Drag, Gesture, Span};
use crate::tool_catalog::{BodyDrag, EdgeDrag, TrackSelectDir};
use crate::types::{EditorTool, SelectedGap, TrimSide, UiClipType};
use aria_models::{ClipId, Rational, RationalTime, TimeRange, TrackId, TrackKind};
use aria_plugin::PluginContext;

fn empty_state(tag: &str) -> AppState {
    let ctx = PluginContext::new();
    let dir = crate::scratch_pool::scratch(tag);
    let mut state = AppState::new(ctx, dir).expect("state");
    state.new_project().expect("new project");
    state
}

fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

#[test]
fn empty_timeline_gestures_do_not_panic() {
    let mut state = empty_state("empty-timeline-test");

    assert_eq!(state.store.sequence.tracks.len(), 1);
    assert!(state.store.sequence.tracks[0].clips.is_empty());
    assert_eq!(state.store.sequence.duration_ticks, 0);

    state.set_playhead(100);
    assert_eq!(state.store.playhead_ticks, 0);

    let split_res = state.split_at_playhead();
    assert!(split_res.is_ok());

    let ripple_res = state.ripple_delete_selected();
    assert!(ripple_res.is_ok());

    state.select_all_unlocked();
    assert!(state.selection.is_empty());
    state.clear_selection();

    state.tool = EditorTool::Razor;
    state.tool = EditorTool::Hand;
    state.tool = EditorTool::Pointer;

    state.set_zoom(2.0, false);
    state.zoom_fit_sequence();
    state.store_mut().scroll_x = 500.0;
    state.rebuild_store();

    assert!(!state.marquee_covers_row(0));
    assert!(!state.marquee_covers_row(5));

    let trim_res = state.ripple_trim(
        aria_models::ClipId::from_raw(999),
        TrackId::from_raw(1),
        0,
        24,
        false,
        1,
    );
    assert!(trim_res.is_err());
}

#[test]
fn thousand_clips_cull_via_binary_search_and_hit_test_accurately() {
    let mut state = empty_state("timeline-1500-clips");
    let media = state.import_media(fixture()).expect("import fixture");
    let v1_id = state.store.sequence.tracks[0].id;

    // Populate 1,500 clips along V1 (each 24 ticks long: 0..36_000 ticks).
    {
        let mut eng = state.engine().lock();
        let item_rate = eng.project().media(media).expect("media").duration.rate;
        let source = TimeRange::at_rate(0, i64::from(item_rate.num), item_rate);
        for i in 0..1_500i64 {
            let start = RationalTime::new(i * 24, Rational::FPS_24);
            eng.apply(aria_commands::Command::Edit(
                aria_commands::EditCommand::AddClip {
                    track: TrackId::from_raw(v1_id),
                    media,
                    source,
                    start,
                },
            ))
            .expect("add clip");
        }
    }
    state.sync_from_engine();
    assert_eq!(state.store.sequence.tracks[0].clips.len(), 1_500);

    // Binary-search culling over a 10-second viewport [2_400, 2_640) returns
    // only the 10 clips inside that window, never scanning all 1,500 clips.
    let all_clips = &state.store.sequence.tracks[0].clips;
    let slice = visible_clips_slice(all_clips, 2_400, 2_640);
    assert_eq!(slice.len(), 10);
    assert_eq!(slice.first().expect("first").timeline_start_ticks, 2_400);
    assert_eq!(slice.last().expect("last").timeline_end_ticks, 2_640);

    // Overview span coalescing caps 1,500 clips to at most 64 segments (1 contiguous run).
    let overview_spans =
        coalesce_overview_spans(&state.store, state.store.sequence.duration_ticks, 800.0);
    assert_eq!(overview_spans.len(), 1);

    // Hit-testing inside that scrolled window resolves the exact clip body and edges.
    let fps = 24.0;
    let pps = BASE_PPS;
    let scroll_x = ticks_to_px(2_400, fps, pps);
    let lane = VisibleLane {
        row: 0,
        track_id: v1_id,
        y_top: 0.0,
        clip_y_bottom: 52.0,
        y_bottom: 52.0,
        collapsed: false,
        locked: false,
        is_drop_row: false,
        band: None,
        drop_tick: None,
        clips: slice.to_vec(),
        outline_rows: Vec::new(),
    };
    let target_clip = &slice[3]; // Clip 103 at [2472, 2496) -> local x [240.0, 320.0]
    let mid_x = ticks_to_px(target_clip.timeline_start_ticks + 12, fps, pps) - scroll_x;
    let hit_body = hit_test(std::slice::from_ref(&lane), mid_x, 26.0, fps, pps, scroll_x);
    assert_eq!(
        hit_body,
        Some(HitTarget::ClipBody {
            clip_id: target_clip.id,
            track_id: v1_id,
            start_ticks: 2_472,
            end_ticks: 2_496,
            locked: false,
            in_name_band: false,
        })
    );

    let in_edge_x = ticks_to_px(target_clip.timeline_start_ticks, fps, pps) - scroll_x + 2.0;
    let hit_in = hit_test(
        std::slice::from_ref(&lane),
        in_edge_x,
        26.0,
        fps,
        pps,
        scroll_x,
    );
    assert_eq!(
        hit_in,
        Some(HitTarget::ClipEdge {
            clip_id: target_clip.id,
            track_id: v1_id,
            left: true,
            start_ticks: 2_472,
            end_ticks: 2_496,
        })
    );
}

#[gpui::test]
fn drawn_shell_with_1200_clips_bounds_canvas_elements_and_clicks_target_clip(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual) = crate::test_support::drawn_shell(cx, "virt-1200");
    let state_in = state.clone();
    let add_range =
        |window: &mut gpui::Window, app: &mut gpui::App, range: std::ops::Range<i64>| {
            state_in.update(app, |state, _| {
                let media = state.import_media(fixture()).expect("import");
                if state.store.sequence.tracks.len() < 2 {
                    state
                        .add_track(TrackKind::Audio, "A1".to_owned())
                        .expect("add A1");
                }
                let v1_id = state.store.sequence.tracks[0].id;
                let a1_id = state.store.sequence.tracks[1].id;
                {
                    let mut eng = state.engine().lock();
                    let item_rate = eng.project().media(media).expect("media").duration.rate;
                    let source = TimeRange::at_rate(0, i64::from(item_rate.num), item_rate);
                    for track_id in [v1_id, a1_id] {
                        for i in range.clone() {
                            let start = RationalTime::new(i * 24, Rational::FPS_24);
                            eng.apply(aria_commands::Command::Edit(
                                aria_commands::EditCommand::AddClip {
                                    track: TrackId::from_raw(track_id),
                                    media,
                                    source,
                                    start,
                                },
                            ))
                            .expect("add clip");
                        }
                    }
                }
                state.sync_from_engine();
            });
            window.refresh();
        };

    visual.update(|window, app| add_range(window, app, 0..20));
    visual.run_until_parked();
    let baseline = visual.read(|app| state.read(app).timeline_paint_stats);
    assert_eq!(baseline.total_clips, 40, "40 clips in baseline sequence");

    visual.update(|window, app| add_range(window, app, 20..600));
    visual.run_until_parked();

    let stats = visual.read(|app| state.read(app).timeline_paint_stats);
    assert_eq!(stats.total_clips, 1_200, "1,200 clips in sequence");
    assert_eq!(
        stats.canvas_elements, 2,
        "exactly one canvas element for the video group and one for the audio group"
    );
    assert!(
        stats.visible_clips < 60,
        "only clips in the visible time window are visited, got {}",
        stats.visible_clips
    );
    assert_eq!(
        stats.visible_clips, baseline.visible_clips,
        "adding 1,160 off-screen clips visits zero extra clips"
    );
    assert_eq!(
        stats.paint_calls, baseline.paint_calls,
        "adding 1,160 off-screen clips issues zero extra paint calls"
    );
    assert!(
        stats.paint_calls <= stats.visible_clips * 12,
        "paint calls ({}) stay bounded by visible clips ({})",
        stats.paint_calls,
        stats.visible_clips
    );

    // Click inside the second clip on V1 ([24, 48) ticks -> x = origin.x + 120px),
    // at the height V1's header is drawn, and verify the click hits and selects
    // that exact clip.
    let v_bounds = visual
        .debug_bounds("timeline-video-canvas")
        .expect("timeline-video-canvas bounds");
    let v1_row = visual
        .debug_bounds("track-header-V1")
        .expect("track-header-V1 bounds");
    let second_clip_id = visual.read(|app| state.read(app).store.sequence.tracks[0].clips[1].id);
    let click_pt = gpui::point(v_bounds.origin.x + gpui::px(120.0), v1_row.center().y);
    visual.simulate_click(click_pt, gpui::Modifiers::none());
    visual.run_until_parked();

    let selected = visual.read(|app| state.read(app).selection.clone());
    assert_eq!(
        selected,
        vec![ClipId::from_raw(second_clip_id)],
        "clicking the virtualized canvas selects the exact clip under the pointer"
    );
}

#[test]
fn video_and_audio_lanes_split_across_divider_with_upward_v_and_downward_a_badges() {
    let mut state = empty_state("timeline-av-split");
    state
        .add_track(TrackKind::Video, "Video 2".to_owned())
        .expect("V2");
    state
        .add_track(TrackKind::Video, "Video 3".to_owned())
        .expect("V3");
    state
        .add_track(TrackKind::Audio, "Audio 1".to_owned())
        .expect("A1");
    state
        .add_track(TrackKind::Audio, "Audio 2".to_owned())
        .expect("A2");

    let row_count = state.store.sequence.tracks.len();
    assert_eq!(state.visual_lane_count(), 3);
    let badges: Vec<String> = (0..row_count).map(|row| state.lane_badge_at(row)).collect();
    // Video tracks stack upward from the divider (V3, V2, V1); audio tracks stack downward (A1, A2).
    assert_eq!(badges, vec!["V3", "V2", "V1", "A1", "A2"]);

    // Divider split clamps cleanly to [0.15, 0.85].
    state.set_timeline_av_split(0.65);
    assert!((state.timeline_av_split - 0.65).abs() < 1e-5);
    state.set_timeline_av_split(0.01);
    assert!((state.timeline_av_split - 0.15).abs() < 1e-5);
    state.set_timeline_av_split(0.99);
    assert!((state.timeline_av_split - 0.85).abs() < 1e-5);
}

#[test]
fn linked_video_and_audio_clips_trim_slip_speed_and_ripple_in_sync_with_single_undo() {
    let mut state = empty_state("timeline-linked-sync");
    let media = state.import_media(fixture()).expect("import");
    state
        .add_track(TrackKind::Audio, "A1".to_owned())
        .expect("add A1");
    let v_track = TrackId::from_raw(state.store.sequence.tracks[0].id);
    let a_track = TrackId::from_raw(state.store.sequence.tracks[1].id);

    {
        let mut eng = state.engine().lock();
        let item_rate = eng.project().media(media).expect("media").duration.rate;
        let source = TimeRange::at_rate(0, i64::from(item_rate.num) * 5, item_rate);
        for (track, start_tick) in [
            (v_track, 0i64),
            (a_track, 0i64),
            (v_track, 120i64),
            (a_track, 120i64),
        ] {
            eng.apply(aria_commands::Command::Edit(
                aria_commands::EditCommand::AddClip {
                    track,
                    media,
                    source,
                    start: RationalTime::new(start_tick, Rational::FPS_24),
                },
            ))
            .expect("add clip");
        }
    }
    state.sync_from_engine();
    let v1_clip = ClipId::from_raw(state.store.sequence.tracks[0].clips[0].id);
    let a1_clip = ClipId::from_raw(state.store.sequence.tracks[1].clips[0].id);
    let v2_clip = ClipId::from_raw(state.store.sequence.tracks[0].clips[1].id);
    let a2_clip = ClipId::from_raw(state.store.sequence.tracks[1].clips[1].id);

    state.selection = vec![v1_clip, a1_clip];
    state.link_selected().expect("link pair 1");
    state.selection = vec![v2_clip, a2_clip];
    state.link_selected().expect("link pair 2");

    // 1. TrimClip on V1 tail trims A1 tail in the same undo step.
    state.trim_clip(v1_clip, 0, 96).expect("trim v1 tail to 96");
    assert_eq!(
        state.store.sequence.tracks[0].clips[0].timeline_end_ticks,
        96
    );
    assert_eq!(
        state.store.sequence.tracks[1].clips[0].timeline_end_ticks, 96,
        "linked audio companion trims with video"
    );
    assert!(state.undo());
    assert_eq!(
        state.store.sequence.tracks[0].clips[0].timeline_end_ticks,
        120
    );
    assert_eq!(
        state.store.sequence.tracks[1].clips[0].timeline_end_ticks,
        120
    );
    assert!(state.redo());
    assert_eq!(
        state.store.sequence.tracks[1].clips[0].timeline_end_ticks,
        96
    );
    assert!(state.undo());

    // 2. Ripple trim on V1 tail ripples both V2 and A2 in one undo entry.
    state
        .ripple_trim(v1_clip, v_track, 0, 120, false, -20)
        .expect("ripple trim v1 by -20");
    assert_eq!(
        state.store.sequence.tracks[0].clips[0].timeline_end_ticks,
        100
    );
    assert_eq!(
        state.store.sequence.tracks[1].clips[0].timeline_end_ticks,
        100
    );
    assert_eq!(
        state.store.sequence.tracks[0].clips[1].timeline_start_ticks,
        100
    );
    assert_eq!(
        state.store.sequence.tracks[1].clips[1].timeline_start_ticks, 100,
        "ripple trim shifts downstream clips on the linked audio lane too"
    );
    assert!(state.undo());
    assert_eq!(
        state.store.sequence.tracks[0].clips[1].timeline_start_ticks,
        120
    );
    assert_eq!(
        state.store.sequence.tracks[1].clips[1].timeline_start_ticks,
        120
    );

    // 3. SetClipSpeed on V1 retimes A1 too in one undo step.
    state.rate_stretch(v1_clip, 120, 60).expect("2x speed");
    assert_eq!(
        state.store.sequence.tracks[0].clips[0].timeline_end_ticks,
        60
    );
    assert_eq!(
        state.store.sequence.tracks[1].clips[0].timeline_end_ticks, 60,
        "linked audio companion retimes to 2x with video"
    );
    assert!(state.undo());
    assert_eq!(
        state.store.sequence.tracks[1].clips[0].timeline_end_ticks,
        120
    );
}

#[test]
fn zoom_around_playhead_and_zoom_bar_handles_update_zoom_and_scroll() {
    let mut state = empty_state("timeline-zoom-bar");
    let media = state.import_media(fixture()).expect("import");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("drop 1");
    state
        .drop_media_on_timeline(media, None, 120)
        .expect("drop 2");
    state.set_playhead(120);
    state.scroll_timeline_to(100.0, false);

    let fps = state.store.sequence.ticks_per_second();
    let before_screen_x = ticks_to_px(120, fps, BASE_PPS * state.zoom) - state.scroll_x;
    state.zoom_around_playhead(2.0);
    let after_screen_x = ticks_to_px(120, fps, BASE_PPS * state.zoom) - state.scroll_x;
    assert!((state.zoom - 2.0).abs() < 1e-4);
    assert!(
        (after_screen_x - before_screen_x).abs() < 0.5,
        "zoom_around_playhead keeps the playhead at the same screen pixel"
    );

    // Dragging the right zoom handle to 0.25 narrows the visible fraction (zooms in).
    let zoom_before = state.zoom;
    state.zoom_bar_drag_handle(false, -0.2);
    assert!(
        state.zoom > zoom_before,
        "dragging the right zoom bar handle inward zooms in"
    );

    // Panning via zoom_bar_pan moves scroll_x proportionally.
    state.zoom_bar_pan(0.5);
    assert!(state.scroll_x > 0.0);
}

#[test]
fn trim_feedback_formats_live_deltas_for_all_trim_and_move_gestures() {
    let mut state = empty_state("timeline-trim-hud");
    let media = state.import_media(fixture()).expect("import");
    let track_id = TrackId::from_raw(state.store.sequence.tracks[0].id);
    state
        .drop_media_on_timeline(media, Some(track_id), 0)
        .expect("drop 1");
    state
        .drop_media_on_timeline(media, Some(track_id), 120)
        .expect("drop 2");
    let c1 = ClipId::from_raw(state.store.sequence.tracks[0].clips[0].id);
    let span = Span {
        start: 0,
        end: 120,
        threshold: 2,
        magnet: false,
    };
    let drag = Drag {
        clip: c1,
        delta: -12,
    };

    let ripple_fb = state
        .compute_trim_feedback(
            Gesture::Edge {
                drag: Some(EdgeDrag::Ripple),
                left: false,
                span,
            },
            drag,
        )
        .expect("ripple feedback");
    assert_eq!(ripple_fb.tool_label.as_ref(), "Ripple Trim");
    assert!(ripple_fb.delta_label.contains("-12f"));
    assert!(ripple_fb.detail_label.contains("00:00:04:12"));

    let rate_fb = state
        .compute_trim_feedback(
            Gesture::Edge {
                drag: Some(EdgeDrag::Rate),
                left: false,
                span,
            },
            Drag {
                clip: c1,
                delta: -60,
            },
        )
        .expect("rate feedback");
    assert_eq!(rate_fb.tool_label.as_ref(), "Rate Stretch");
    assert!(rate_fb.detail_label.contains("200.0%"));

    let slip_fb = state
        .compute_trim_feedback(
            Gesture::Body {
                drag: Some(BodyDrag::Slip),
                track: track_id,
                span,
            },
            drag,
        )
        .expect("slip feedback");
    assert_eq!(slip_fb.tool_label.as_ref(), "Slip");
    assert!(slip_fb.delta_label.contains("+12f"));
}

#[test]
fn track_select_forward_and_backward_select_one_or_all_tracks() {
    let mut state = empty_state("timeline-track-select");
    let media = state.import_media(fixture()).expect("import");
    state
        .add_track(TrackKind::Video, "V2".to_owned())
        .expect("add V2");
    let top_track = TrackId::from_raw(state.store.sequence.tracks[0].id);
    let bot_track = TrackId::from_raw(state.store.sequence.tracks[1].id);

    {
        let mut eng = state.engine().lock();
        let item_rate = eng.project().media(media).expect("media").duration.rate;
        let source = TimeRange::at_rate(0, i64::from(item_rate.num) * 2, item_rate);
        for (track, start_tick) in [
            (top_track, 0i64),
            (top_track, 120i64),
            (bot_track, 20i64),
            (bot_track, 150i64),
        ] {
            eng.apply(aria_commands::Command::Edit(
                aria_commands::EditCommand::AddClip {
                    track,
                    media,
                    source,
                    start: RationalTime::new(start_tick, Rational::FPS_24),
                },
            ))
            .expect("add clip");
        }
    }
    state.sync_from_engine();

    let top_c0 = ClipId::from_raw(state.store.sequence.tracks[0].clips[0].id);
    let top_c1 = ClipId::from_raw(state.store.sequence.tracks[0].clips[1].id);
    let bot_c0 = ClipId::from_raw(state.store.sequence.tracks[1].clips[0].id);
    let bot_c1 = ClipId::from_raw(state.store.sequence.tracks[1].clips[1].id);

    // Track Select Forward on all tracks (`all_tracks = true`) from tick 100 selects top_c1 (120..168) and bot_c1 (150..198).
    state.select_track_clips(top_track, 100, TrackSelectDir::Forward, true);
    assert_eq!(state.selection.len(), 2);
    assert!(state.selection.contains(&top_c1));
    assert!(state.selection.contains(&bot_c1));

    // Single-track Track Select Forward (`all_tracks = false`) on top_track from tick 100 selects only top_c1.
    state.select_track_clips(top_track, 100, TrackSelectDir::Forward, false);
    assert_eq!(state.selection, vec![top_c1]);

    // Track Select Backward on all tracks (`all_tracks = true`) up to tick 100 selects top_c0 (starts 0) and bot_c0 (starts 20).
    state.select_track_clips(top_track, 100, TrackSelectDir::Backward, true);
    assert_eq!(state.selection.len(), 2);
    assert!(state.selection.contains(&top_c0));
    assert!(state.selection.contains(&bot_c0));
}

#[test]
fn edge_selection_trims_via_nudge_and_gap_selection_ripple_deletes_with_undo() {
    let mut state = empty_state("timeline-edge-and-gap");
    let media = state.import_media(fixture()).expect("import");
    let track_id = TrackId::from_raw(state.store.sequence.tracks[0].id);
    state
        .drop_media_on_timeline(media, Some(track_id), 0)
        .expect("clip 1 at 0..120");
    state
        .drop_media_on_timeline(media, Some(track_id), 180)
        .expect("clip 2 at 180..300 (leaving a 60-tick gap at 120..180)");

    let clip1 = ClipId::from_raw(state.store.sequence.tracks[0].clips[0].id);

    // 1. Select the Out edge of clip 1 and nudge it by -10 frames via nudge_selected.
    state.select_edge(clip1, TrimSide::Out);
    assert!(state.selected_edge.is_some());
    state.nudge_selected(-10).expect("trim selected edge -10");
    assert_eq!(
        state.store.sequence.tracks[0].clips[0].timeline_end_ticks, 110,
        "nudging a selected Out edge trims the clip tail by -10 ticks"
    );
    assert!(state.undo());
    assert_eq!(
        state.store.sequence.tracks[0].clips[0].timeline_end_ticks,
        120
    );

    // 2. Click inside the gap [120, 180) -> selects the gap, and delete_selected closes it!
    let gap = state.select_gap_at(track_id, 150);
    assert_eq!(
        gap,
        Some(SelectedGap {
            track: track_id.raw(),
            start_ticks: 120,
            end_ticks: 180,
        })
    );
    state.clear_selected().expect("ripple delete selected gap");
    assert_eq!(state.selected_gap, None);
    assert_eq!(
        state.store.sequence.tracks[0].clips[1].timeline_start_ticks, 120,
        "closing the [120, 180) gap shifts the downstream clip from 180 to 120"
    );

    // Undo restores the 60-tick gap; redo closes it again.
    assert!(state.undo());
    assert_eq!(
        state.store.sequence.tracks[0].clips[1].timeline_start_ticks,
        180
    );
    assert!(state.redo());
    assert_eq!(
        state.store.sequence.tracks[0].clips[1].timeline_start_ticks,
        120
    );
}

#[test]
fn clip_type_tint_prefers_label_then_clip_type_colour_then_kind() {
    let mut state = empty_state("timeline-clip-type-tint");
    let media = state.import_media(fixture()).expect("import");
    let track_id = TrackId::from_raw(state.store.sequence.tracks[0].id);
    state
        .drop_media_on_timeline(media, Some(track_id), 0)
        .expect("drop clip");
    let mut clip = state.store.sequence.tracks[0].clips[0].clone();
    let theme = crate::theme::Theme::dark();
    let label = |color| theme.label(color, false);

    // 1. A picture clip with no label draws Teal.
    assert_eq!(clip.label, None);
    assert_eq!(clip.default_label, aria_models::LabelColor::VIDEO);
    clip.clip_type = None;
    clip.selected = false;
    assert_eq!(
        crate::panels::timeline::clip::tint(&theme, &clip),
        label(aria_models::LabelColor::Teal)
    );

    // 2. When `clip_type.colour` is set and `label` is None, `clip_type.colour` wins.
    clip.clip_type = Some(UiClipType {
        id: "com.example.fx".into(),
        label: "Sparkle FX".into(),
        colour: Some(aria_models::MarkerColor::Purple),
        icon: Some("icons/sparkle.svg"),
        missing: Some("Missing plugin: com.example.fx".into()),
        handles: Vec::new(),
    });
    assert_eq!(
        crate::panels::timeline::clip::tint(&theme, &clip),
        label(aria_models::LabelColor::Purple)
    );

    // 3. Explicit clip `label` colour takes precedence over `clip_type.colour`.
    clip.label = Some(aria_models::LabelColor::Caribbean);
    assert_eq!(
        crate::panels::timeline::clip::tint(&theme, &clip),
        label(aria_models::LabelColor::Caribbean)
    );

    // 4. Selecting the clip draws the label's selected fill.
    clip.selected = true;
    assert_eq!(
        crate::panels::timeline::clip::tint(&theme, &clip),
        theme.label(aria_models::LabelColor::Caribbean, true)
    );
}
#[gpui::test]
fn property_rows_and_keyframes_hit_test_and_draw_in_virtualized_timeline(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual) = crate::test_support::drawn_shell(cx, "prop-rows-draw");
    let state_in = state.clone();
    visual.update(|_window, app| {
        state_in.update(app, |state, _| {
            let media = state.import_media(fixture()).expect("import");
            let track_id = TrackId::from_raw(state.store.sequence.tracks[0].id);
            state
                .drop_media_on_timeline(media, Some(track_id), 0)
                .expect("drop clip");
            let clip_id = ClipId::from_raw(state.store.sequence.tracks[0].clips[0].id);
            state.select_clip(clip_id);
            state
                .set_keyframe_scalar(aria_models::ClipParam::Opacity, 0, 0.25)
                .expect("kf 0");
            state
                .set_keyframe_scalar(aria_models::ClipParam::Opacity, 24, 1.0)
                .expect("kf 24");
            state.set_playhead(24);
            state
                .easy_ease_selected_or_playhead()
                .expect("easy ease at 24");
            state.toggle_clip_outline(clip_id.raw());
            state.toggle_timeline_graph_editor();
        });
    });
    visual.run_until_parked();

    visual.update(|window, _app| {
        window.refresh();
    });
    visual.run_until_parked();

    visual.read(|app_cx| {
        let s = state.read(app_cx);
        let v1_id = s.store.sequence.tracks[0].id;
        let clip_id = ClipId::from_raw(s.store.sequence.tracks[0].clips[0].id);
        let outline_rows = s.track_outline_rows(v1_id);
        assert!(
            outline_rows.len() >= 6,
            "Transform group header + 6 transform rows should be visible"
        );
        let lane = VisibleLane {
            row: 0,
            track_id: v1_id,
            y_top: 0.0,
            clip_y_bottom: 52.0,
            y_bottom: 52.0 + outline_rows.len() as f32 * crate::app::property_rows::PROPERTY_ROW_H,
            collapsed: false,
            locked: false,
            is_drop_row: false,
            band: None,
            drop_tick: None,
            clips: s.store.sequence.tracks[0].clips.to_vec(),
            outline_rows: outline_rows.clone(),
        };
        // Opacity is the last row in Transform (index 6: 0 is GroupHeader, 1..=6 are PosX, PosY, ScaleX, ScaleY, Rotation, Opacity).
        let opacity_idx = outline_rows
            .iter()
            .position(|r| {
                matches!(
                    r,
                    crate::app::property_rows::TimelineOutlineRow::Property(p)
                        if p.param == aria_models::ClipParam::Opacity
                )
            })
            .expect("opacity row");
        let row_mid_y =
            52.0 + (opacity_idx as f32 + 0.5) * crate::app::property_rows::PROPERTY_ROW_H;
        let kf_24_x = ticks_to_px(24, 24.0, BASE_PPS);
        let hit_kf = hit_test(
            std::slice::from_ref(&lane),
            kf_24_x,
            row_mid_y,
            24.0,
            BASE_PPS,
            0.0,
        );
        assert_eq!(
            hit_kf,
            Some(HitTarget::Keyframe {
                clip: clip_id,
                param: aria_models::ClipParam::Opacity,
                tick: 24,
                interior: false,
                hold: false,
                roving: false,
            })
        );
        let hit_row = hit_test(
            std::slice::from_ref(&lane),
            kf_24_x + 30.0,
            row_mid_y,
            24.0,
            BASE_PPS,
            0.0,
        );
        assert!(matches!(
            hit_row,
            Some(HitTarget::PropertyRow {
                clip,
                param: aria_models::ClipParam::Opacity,
                ..
            }) if clip == clip_id
        ));
    });
}

/// A mask's Path row on the drawn timeline: its keyframes and its band
/// hit-test as the path's, a press on a menu row eases the keyframe the menu
/// opened on, Escape closes the menu, and its stopwatch and a property row's
/// name themselves on hover.
///
/// The row sits among the catalog rows but is none of them, so a hit test or a
/// menu that only knows catalog rows passes it by and the press lands on the
/// lane under it.
#[gpui::test]
fn a_mask_path_row_hit_tests_its_keys_and_its_menu_eases_and_closes(cx: &mut gpui::TestAppContext) {
    use crate::app::property_rows::{MaskPathKeyMenu, PROPERTY_ROW_H, TimelineOutlineRow};

    let executor = cx.executor();
    let (state, visual) = crate::test_support::drawn_shell(cx, "mask-path-row");
    let state_in = state.clone();
    let clip_id = visual.update(|_window, app| {
        state_in.update(app, |state, _| {
            let media = state.import_media(fixture()).expect("import");
            let track_id = TrackId::from_raw(state.store.sequence.tracks[0].id);
            state
                .drop_media_on_timeline(media, Some(track_id), 0)
                .expect("drop clip");
            let clip_id = ClipId::from_raw(state.store.sequence.tracks[0].clips[0].id);
            state.select_clip(clip_id);
            let triangle = [[0.2, 0.2], [0.8, 0.2], [0.5, 0.8]]
                .map(aria_models::ShapePathPoint::corner)
                .to_vec();
            state.add_selected_mask_path(triangle).expect("a path mask");
            state.set_playhead(0);
            state
                .animate_mask_path(clip_id, 0)
                .expect("key the path at 0");
            state.set_playhead(24);
            state
                .key_mask_path(clip_id, 0, false)
                .expect("key the path at 24");
            state.reveal_properties(crate::app::property_rows::RevealAction::Masks, false);
            clip_id
        })
    });
    // The rows twirl open over the lane beat, on the clock a test moves: one
    // frame starts the glide and one past its end draws every row.
    for _ in 0..2 {
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        executor.advance_clock(std::time::Duration::from_secs(1));
    }

    visual.read(|app_cx| {
        let s = state.read(app_cx);
        let v1_id = s.store.sequence.tracks[0].id;
        let outline_rows = s.track_outline_rows(v1_id);
        let path_idx = outline_rows
            .iter()
            .position(|row| matches!(row, TimelineOutlineRow::MaskPath(path) if path.mask == 0))
            .expect("Alt+M opens the mask's Path row");
        let lane = VisibleLane {
            row: 0,
            track_id: v1_id,
            y_top: 0.0,
            clip_y_bottom: 52.0,
            y_bottom: 52.0 + outline_rows.len() as f32 * PROPERTY_ROW_H,
            collapsed: false,
            locked: false,
            is_drop_row: false,
            band: None,
            drop_tick: None,
            clips: s.store.sequence.tracks[0].clips.to_vec(),
            outline_rows,
        };
        let row_mid_y = 52.0 + (path_idx as f32 + 0.5) * PROPERTY_ROW_H;
        let key_x = ticks_to_px(24, 24.0, BASE_PPS);
        let hit = |x: f32| {
            hit_test(
                std::slice::from_ref(&lane),
                x,
                row_mid_y,
                24.0,
                BASE_PPS,
                0.0,
            )
        };
        assert_eq!(
            hit(key_x),
            Some(HitTarget::MaskPathKey {
                clip: clip_id,
                mask: 0,
                tick: 24,
                hold: false,
            })
        );
        assert_eq!(
            hit(key_x + 30.0),
            Some(HitTarget::MaskPathRow {
                clip: clip_id,
                mask: 0,
            })
        );
    });

    let open = |visual: &mut gpui::VisualTestContext| {
        visual.update(|window, app| {
            state.update(app, |state, _| {
                state.open_mask_path_key_menu(MaskPathKeyMenu {
                    clip: clip_id,
                    mask: 0,
                    tick: 24,
                    hold: false,
                    x: 40.0,
                    y: 100.0,
                });
            });
            window.refresh();
        });
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("mask-path-key-menu").is_some(),
            "the timeline did not draw the Path keyframe menu"
        );
    };
    let menu = |visual: &mut gpui::VisualTestContext| {
        visual.read(|app| state.read(app).mask_path_key_menu())
    };

    // Row 4 is Hold.
    open(visual);
    let hold = visual
        .debug_bounds("kf-menu-row-4")
        .expect("the menu draws its Hold row");
    visual.simulate_click(hold.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    assert_eq!(menu(visual), None, "a press on a row left the menu open");
    let easing = visual.read(|app| {
        state
            .read(app)
            .engine()
            .lock()
            .project()
            .clip(clip_id)
            .and_then(|clip| clip.masks[0].path.easing_at(24))
    });
    assert_eq!(easing, Some(aria_models::Easing::Hold));

    open(visual);
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    assert_eq!(menu(visual), None, "Escape left the menu open");

    // A stopwatch is a glyph, so the Path row's and a property row's each
    // name themselves on hover.
    for watch in ["path-watch", "prop-watch"] {
        let at = visual
            .debug_bounds(watch)
            .unwrap_or_else(|| panic!("the rows draw no {watch}"))
            .center();
        visual.simulate_mouse_move(at, None, gpui::Modifiers::none());
        executor.advance_clock(crate::tooltip::SHOW_DELAY * 2);
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("tooltip").is_some(),
            "{watch} has no tip"
        );
        let away = gpui::point(gpui::px(2.0), gpui::px(2.0));
        visual.simulate_mouse_move(away, None, gpui::Modifiers::none());
        visual.run_until_parked();
        assert_eq!(
            visual.debug_bounds("tooltip"),
            None,
            "{watch}'s tip outlived the hover"
        );
    }
}
