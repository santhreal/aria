//! Regression tests for held tool keys, Selection-tool edge modifier overrides,
//! tool variants popover state, Pen tool clip-line keyframing, and Type tool
//! Viewer text creation.
//!
//! # Why
//!
//! Closes the class where tool shortcuts only switched permanently on key-down,
//! edge modifier overrides (`Ctrl` -> Ripple Trim, `Ctrl+Alt` -> Rolling Edit)
//! were not reflected in tool state, tool variants were unreachable, Pen clicks
//! on timeline clips did not keyframe Opacity/Volume, and Type clicks on the
//! Viewer did not create a positioned text clip. Does not catch GPUI window
//! event delivery ordering outside `Shell`.

use aria_models::ClipParam;

use super::*;
use crate::app::canvas_overlay::{FrameBox, Rect};
use crate::app::premiere_tests::with_clip;
use crate::keys::{KeyBody, KeyHost, KeyRoute, bindings, dispatch_key_up, dispatch_plain_key_down};
use crate::tool_catalog::tool_specs;

#[test]
fn every_tool_key_tap_switches_permanently_and_hold_restores_previous_tool() {
    let (mut state, _) = with_clip();
    let host = KeyHost::default();
    for binding in bindings::SHIPPED_BINDINGS {
        host.add(*binding);
    }

    for spec in tool_specs() {
        let shift = spec.key.starts_with("Shift+");
        let bare = spec.key.strip_prefix("Shift+").unwrap_or(spec.key);
        let key_lower = bare.to_ascii_lowercase();
        let binding = host
            .resolve(KeyRoute::Editor, &key_lower, false, shift, false)
            .unwrap_or_else(|| panic!("missing binding for {}", spec.id));
        let KeyBody::Plain(run) = binding.run else {
            panic!("tool binding {} must be Plain", spec.id);
        };

        // Pick a distinct starting tool so both switch and restore are observable.
        let origin = if spec.tool == EditorTool::Pointer {
            EditorTool::Razor
        } else {
            EditorTool::Pointer
        };

        // 1. Quick tap switches permanently on release.
        state.tool = origin;
        dispatch_plain_key_down(&mut state, &binding, false, run);
        assert_eq!(state.tool, spec.tool, "tap down on {}", spec.id);
        assert!(
            !dispatch_key_up(
                &mut state,
                KeyRoute::Editor,
                &key_lower,
                false,
                shift,
                false
            ),
            "tap release on {} should keep the new tool",
            spec.id
        );
        assert_eq!(state.tool, spec.tool, "tap on {} stays on tool", spec.id);

        // 2. Hold via auto-repeat (`is_held = true`) restores `origin` on release.
        state.tool = origin;
        dispatch_plain_key_down(&mut state, &binding, false, run);
        dispatch_plain_key_down(&mut state, &binding, true, run);
        assert_eq!(state.tool, spec.tool, "held repeat on {}", spec.id);
        assert!(
            dispatch_key_up(
                &mut state,
                KeyRoute::Editor,
                &key_lower,
                false,
                shift,
                false
            ),
            "hold release on {} must report tool restore",
            spec.id
        );
        assert_eq!(
            state.tool, origin,
            "hold release on {} restores previous tool",
            spec.id
        );

        // 3. Hold via >= 300 ms elapsed threshold restores `origin` on release.
        state.tool = origin;
        dispatch_plain_key_down(&mut state, &binding, false, run);
        state.mark_tool_hold_elapsed();
        assert!(
            dispatch_key_up(
                &mut state,
                KeyRoute::Editor,
                &key_lower,
                false,
                shift,
                false
            ),
            "300ms hold release on {} must restore previous tool",
            spec.id
        );
        assert_eq!(state.tool, origin);
    }
}

#[test]
fn selection_tool_ctrl_and_ctrl_alt_over_edge_switch_to_ripple_and_roll() {
    let (mut state, _) = with_clip();

    for spec in tool_specs() {
        let tool = spec.tool;
        if tool.selects() {
            assert_eq!(tool.edge_tool(false, false), EditorTool::Pointer);
            assert_eq!(tool.edge_tool(true, false), EditorTool::Ripple);
            assert_eq!(tool.edge_tool(true, true), EditorTool::Roll);
            assert_eq!(tool.edge_tool(false, true), EditorTool::Pointer);
        } else {
            assert_eq!(tool.edge_tool(true, false), tool);
            assert_eq!(tool.edge_tool(true, true), tool);
        }
    }

    state.tool = EditorTool::Pointer;
    assert!(state.update_edge_modifiers(true, true, false));
    assert_eq!(state.tool, EditorTool::Ripple);
    assert!(state.update_edge_modifiers(true, true, true));
    assert_eq!(state.tool, EditorTool::Roll);
    assert!(state.update_edge_modifiers(true, false, false));
    assert_eq!(state.tool, EditorTool::Pointer);

    // Leaving the edge while Ctrl is still down also restores Selection tool.
    assert!(state.update_edge_modifiers(true, true, false));
    assert_eq!(state.tool, EditorTool::Ripple);
    assert!(state.update_edge_modifiers(false, true, false));
    assert_eq!(state.tool, EditorTool::Pointer);
}

#[test]
fn tool_variants_open_on_triangle_or_300ms_hold_and_select_variant() {
    let (mut state, _) = with_clip();

    for spec in tool_specs() {
        let variants = spec.tool.variants();
        assert_eq!(
            !variants.is_empty(),
            spec.tool.has_variants(),
            "variants() and has_variants must agree for {}",
            spec.id
        );
    }

    // Releasing before 300 ms cancels the press-and-hold timer.
    let seq = state.begin_tool_press(EditorTool::Pen);
    state.end_tool_press();
    assert!(!state.hold_open_tool_variants(EditorTool::Pen, seq));
    assert_eq!(state.tool_variants_open(), None);

    // Holding for 300 ms opens the variants popover.
    let seq = state.begin_tool_press(EditorTool::Pen);
    assert!(state.hold_open_tool_variants(EditorTool::Pen, seq));
    assert_eq!(state.tool_variants_open(), Some(EditorTool::Pen));

    // Selecting a Pen variant switches tool + pen_variant and closes the popover.
    state.select_tool_variant(ToolVariant::Pen(PenVariant::ConvertVertex));
    assert_eq!(state.tool, EditorTool::Pen);
    assert_eq!(state.pen_variant(), PenVariant::ConvertVertex);
    assert_eq!(state.tool_variants_open(), None);

    // Selecting a Trim variant switches tool and closes the popover.
    state.open_tool_variants(EditorTool::Ripple);
    assert_eq!(state.tool_variants_open(), Some(EditorTool::Ripple));
    state.select_tool_variant(ToolVariant::Tool(EditorTool::Slip));
    assert_eq!(state.tool, EditorTool::Slip);
    assert_eq!(state.tool_variants_open(), None);
}

#[test]
fn pen_tool_click_on_clip_line_keys_the_property_the_line_draws() {
    let (mut state, video_clip) = with_clip();

    assert!(state.keyframes_of(ClipParam::Opacity).is_empty());
    let param = state
        .add_clip_line_keyframe(video_clip, 12)
        .expect("video clip line keyframe");
    assert_eq!(param, Some(ClipParam::Opacity));
    assert!(
        state
            .keyframes_of(ClipParam::Opacity)
            .iter()
            .any(|mark| mark.tick == 12)
    );

    // A property chosen for the line is the one the Pen keys.
    state
        .apply_command(aria_commands::Command::Edit(
            aria_commands::EditCommand::SetClipLineParam {
                clip: video_clip,
                param: Some(ClipParam::Rotation),
            },
        ))
        .expect("choose Rotation for the line");
    let param = state
        .add_clip_line_keyframe(video_clip, 20)
        .expect("rotation line keyframe");
    assert_eq!(param, Some(ClipParam::Rotation));
    assert!(
        state
            .keyframes_of(ClipParam::Rotation)
            .iter()
            .any(|mark| mark.tick == 20)
    );

    // Audio clip on an audio track keys Volume when clicked with the Pen tool.
    let (mut audio_state, audio_clip) = crate::app::premiere_tests::with_media(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/fixture-sine-48k-2s.wav"),
    );
    assert!(audio_state.keyframes_of(ClipParam::Volume).is_empty());
    let param = audio_state
        .add_clip_line_keyframe(audio_clip, 8)
        .expect("audio clip line keyframe");
    assert_eq!(param, Some(ClipParam::Volume));
    assert!(
        audio_state
            .keyframes_of(ClipParam::Volume)
            .iter()
            .any(|mark| mark.tick == 8)
    );
}

#[test]
fn type_tool_click_in_viewer_creates_positioned_text_clip_in_one_undo_step() {
    let (mut state, _) = with_clip();
    state.overlay_frame = FrameBox {
        container: Some(Rect::new(0.0, 0.0, 1920.0, 1080.0)),
        canvas: Some(Rect::new(0.0, 0.0, 1920.0, 1080.0)),
        layer: None,
    };
    state.tool = EditorTool::Type;

    // Clicking outside the canvas (on letterbox) does nothing.
    assert!(!state.select_layer_at([-20.0, 540.0], false));

    // Clicking at (75%, 25%) of the canvas creates a text clip at position (0.25, -0.25).
    assert!(state.select_layer_at([1440.0, 270.0], false));
    let clip_id = state
        .selected_clip()
        .expect("created text clip is selected");
    {
        let eng = state.engine.lock();
        let clip = eng.project().clip(clip_id).expect("text clip in project");
        assert!(matches!(
            clip.content,
            aria_models::ClipSource::Generated(aria_models::Generator::Text { .. })
        ));
        let pos = clip.transform.position.sample(0);
        assert!((pos[0] - 0.25).abs() < 1e-4);
        assert!((pos[1] - (-0.25)).abs() < 1e-4);
    }

    // One undo removes both the transform and the clip creation.
    assert!(state.undo());
    assert!(state.engine.lock().project().clip(clip_id).is_none());
}

#[test]
fn motion_path_ctrl_click_is_restricted_to_pen_tool() {
    let (mut state, _) = with_clip();
    state.overlay_frame = FrameBox {
        container: Some(Rect::new(0.0, 0.0, 1920.0, 1080.0)),
        canvas: Some(Rect::new(0.0, 0.0, 1920.0, 1080.0)),
        layer: Some(Rect::new(0.0, 0.0, 1920.0, 1080.0)),
    };
    let rate = state.store.sequence.frame_rate;
    let clip = state.selected_clip().expect("selected clip");
    for (tick, pos) in [(0, [-0.25f32, 0.0f32]), (12, [0.25f32, 0.0f32])] {
        state
            .apply_command(Command::Edit(EditCommand::SetParamKeyframe {
                clip,
                param: ClipParam::Position,
                at: RationalTime::new(tick, rate),
                value: aria_models::ParamValue::Vec2(pos),
                easing: Easing::Linear,
                tangents: None,
            }))
            .expect("set position keyframe");
    }
    let mid = state
        .motion_paths()
        .first()
        .and_then(|path| path.dots.get(path.dots.len() / 2).copied())
        .expect("motion path dot");

    for spec in tool_specs() {
        state.tool = spec.tool;
        assert_eq!(
            state.can_add_key_on_path(true, mid),
            spec.tool.pens(),
            "motion path Ctrl+click for {} must match tool.pens()",
            spec.id
        );
        assert!(
            !state.can_add_key_on_path(false, mid),
            "motion path click without Ctrl must not add keyframe for {}",
            spec.id
        );
    }
}
