//! Look tools: how a clip sits on the canvas and how it is graded — the
//! constants behind the inspector, each animatable through the keyframe
//! tools.


tool_group! {
    "set_clip_transform" => SetClipTransform(SetClipTransform),
        "Change a clip's placement on the canvas: position, scale (number for uniform or [x, y] for per-axis), rotation, opacity. Omitted fields keep their current value. Not valid on audio tracks.";
    "set_clip_crop" => SetClipCrop(SetClipCrop),
        "Crop a clip to a sub-region of its frame (fractions trimmed off each edge; 0 restores an edge) and/or mirror it with flip_h / flip_v. Omitted fields keep their current value. Not valid on audio tracks.";
    "set_clip_mask" => SetClipMask(SetClipMask),
        "Set or clear a shaped alpha mask on a media-backed visual clip. Mask kinds: linear, mirror, circle, rectangle, heart, star. Optional constants: feather (0..1), invert, center/size as fractions of the layer, rotation in degrees clockwise, roundness (0..1, rectangle only). Omitted geometry uses defaults. Constants only — animate with set_param_keyframe using look params (mask_feather, mask_center, mask_size, mask_rotation, mask_roundness). Pass null for mask to clear.";
    "set_clip_chroma" => SetClipChroma(SetClipChroma),
        "Set or clear chroma keying (green screen) on a media-backed visual clip. Pass null for chroma to clear.";
    "set_clip_matte" => SetClipMatte(SetClipMatte),
        "Set or clear a track matte on a visual clip: the layer directly above cuts this clip. Sources: alpha (the layer above's own transparency) or luma (white keeps, black cuts). Optional invert swaps what is kept; optional softness (0..1) blurs the matte edge. Animate softness with set_param_keyframe using the look param matte_softness. A clip with nothing above it draws unmatted. Pass null for matte to clear.";
    "set_clip_stabilize" => SetClipStabilize(SetClipStabilize),
        "Set or clear video stabilization on a media-backed video clip (not still images). Levels: recommended, smooth, max_smooth. Pass null for level to clear.";
    "set_clip_filter" => SetClipFilter(SetClipFilter),
        "Set or clear a color-grade filter preset on any visual clip. Filter ids include vivid, warm, cool, noir, sunset, and others from the catalog. Pass null for filter to clear.";
    "set_clip_blend_mode" => SetClipBlendMode(SetClipBlendMode),
        "Set how a visual clip composites over the layers below it (CapCut Blend). Modes: normal, darken, multiply, color_burn, lighten, screen, color_dodge, add, overlay, soft_light, hard_light, difference, exclusion. Visual clips only; normal resets to plain source-over.";
    "set_motion_blur" => SetMotionBlur(SetMotionBlur),
        "Enable or configure transform motion blur on a visual clip: when the clip's transform is animated, export supersamples its placement across the shutter and averages the samples (texture stays at the frame time — transform-only, like After Effects). Fields: enabled (bool), optional shutter_deg (0..720, default 180), optional samples (2..32, default 8; render clamps to 16). Plain values — not animatable. Visual clips only; requires an animated transform to have any visible effect.";
    "set_layer_styles" => SetClipLayerStyles(SetClipLayerStyles),
        "Set layer-quad shadow/glow/outline/background styles on any visual clip (distinct from text glyph treatments). Each block is optional constants in reference pixels (1080p baseline); omitted blocks are removed. Empty styles clears every block. Constants only — animate with set_param_keyframe using style params (shadow_blur, glow_color, …).";
    "set_clip_adjustments" => SetClipAdjustments(SetClipAdjustments),
        "Set manual color adjustments on any visual clip. Each argument is one grade slider; the schema lists them. Signed sliders are -1..1, sharpness and vignette are 0..1, and the lift/gamma/gain wheels are 0..4 around unity. Omitted sliders keep their current value.";
    "set_clip_animation" => SetClipAnimation(SetClipAnimation),
        "Set or clear a look animation preset on any visual clip. Slots: in (entrance), out (exit), combo (looping). Animation ids include fade_in, slide_up, pulse, typewriter, wave, etc. Optional speed (0.25–4), intensity (0–2), and stagger (0–2, text presets). Pass null for animation to clear the slot.";
}

#[cfg(test)]
use super::cases::{Expect, ToolCase};

#[cfg(test)]
pub(super) const CASES: &[ToolCase] = &[
    ToolCase {
        tool: "set_clip_transform",
        args: |f| serde_json::json!({ "clip": f.clip, "position_x": 0.25, "opacity": 0.5 }),
        // Omitted axes keep the clip's current placement.
        expect: Expect::Edit(|f| {
            let mut transform = f.clip_state(f.clip).transform.sample(0);
            transform.position[0] = 0.25;
            transform.opacity = 0.5;
            aria_commands::EditCommand::SetClipTransform {
                clip: f.clip_id(f.clip),
                transform,
                at: None,
            }
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "opacity": 5.0 }),
        refusal: "invalid transform",
    },
    ToolCase {
        tool: "set_clip_crop",
        args: |f| serde_json::json!({ "clip": f.clip, "left": 0.1 }),
        // Insets are trimmed off each edge; the command carries the kept
        // region, so a left inset of 0.1 keeps 0.9 of the width.
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetClipCrop {
            clip: f.clip_id(f.clip),
            crop: aria_models::CropRect {
                x: 0.1,
                y: 0.0,
                w: 1.0 - 0.1,
                h: 1.0,
            },
            flip_h: false,
            flip_v: false,
            at: None,
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "left": 1.5 }),
        refusal: "left must be a fraction between 0 and 1",
    },
    ToolCase {
        tool: "set_clip_mask",
        args: |f| {
            serde_json::json!({
                "clip": f.clip,
                "mask": { "kind": "circle", "feather": 0.25 },
            })
        },
        expect: Expect::Edit(|f| {
            let mut mask = aria_models::Mask::new(aria_models::MaskKind::Circle);
            mask.feather = aria_models::Param::Constant(0.25);
            aria_commands::EditCommand::SetClipMask {
                clip: f.clip_id(f.clip),
                mask: Some(mask),
            }
        }),
        invalid: |f| {
            serde_json::json!({
                "clip": f.clip,
                "mask": { "kind": "circle", "feather": 4.0 },
            })
        },
        refusal: "mask feather must be between 0 and 1",
    },
    ToolCase {
        tool: "set_clip_chroma",
        args: |f| {
            serde_json::json!({
                "clip": f.clip,
                "chroma": { "rgb": [0, 255, 0], "strength": 0.5 },
            })
        },
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetClipChroma {
            clip: f.clip_id(f.clip),
            chroma: Some(aria_models::ChromaKey {
                rgb: [0, 255, 0],
                strength: 0.5f32.into(),
                shadow: 0.0f32.into(),
            }),
        }),
        invalid: |f| {
            serde_json::json!({
                "clip": f.clip,
                "chroma": { "rgb": [0, 255, 0], "strength": 3.0 },
            })
        },
        refusal: "chroma strength must be between 0 and 1",
    },
    ToolCase {
        tool: "set_clip_matte",
        args: |f| {
            serde_json::json!({
                "clip": f.clip,
                "matte": { "source": "luma", "softness": 0.25 },
            })
        },
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetClipMatte {
            clip: f.clip_id(f.clip),
            matte: Some(aria_models::TrackMatte {
                source: aria_models::MatteSource::Luma,
                invert: false,
                softness: 0.25f32.into(),
            }),
        }),
        invalid: |f| {
            serde_json::json!({
                "clip": f.clip,
                "matte": { "source": "luma", "softness": 2.0 },
            })
        },
        refusal: "matte softness must be between 0 and 1",
    },
    ToolCase {
        tool: "set_clip_stabilize",
        args: |f| serde_json::json!({ "clip": f.clip, "level": "smooth" }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetClipStabilize {
            clip: f.clip_id(f.clip),
            stabilize: Some(aria_models::StabilizeLevel::Smooth),
        }),
        // A generated clip has no camera motion to solve.
        invalid: |f| serde_json::json!({ "clip": f.title, "level": "smooth" }),
        refusal: "set_clip_stabilize only works on media",
    },
    ToolCase {
        tool: "set_clip_filter",
        args: |f| serde_json::json!({ "clip": f.clip, "filter": { "id": "vivid" } }),
        // An omitted intensity takes the catalog's default blend.
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetClipFilter {
            clip: f.clip_id(f.clip),
            filter: Some(aria_models::Filter {
                id: "vivid".into(),
                intensity: 0.8f32.into(),
            }),
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "filter": { "id": "technicolor" } }),
        refusal: "unknown filter 'technicolor'",
    },
    ToolCase {
        tool: "set_clip_blend_mode",
        args: |f| serde_json::json!({ "clip": f.clip, "mode": "multiply" }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetClipBlendMode {
            clip: f.clip_id(f.clip),
            mode: aria_models::BlendMode::Multiply,
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "mode": "divide" }),
        refusal: "unknown variant `divide`",
    },
    ToolCase {
        tool: "set_motion_blur",
        args: |f| {
            serde_json::json!({
                "clip": f.clip,
                "enabled": true,
                "shutter_deg": 270.0,
                "samples": 12,
            })
        },
        expect: Expect::Edit(|f| {
            let mut motion_blur = f.clip_state(f.clip).motion_blur;
            motion_blur.enabled = true;
            motion_blur.shutter_deg = 270.0;
            motion_blur.samples = 12;
            aria_commands::EditCommand::SetClipMotionBlur {
                clip: f.clip_id(f.clip),
                motion_blur,
            }
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "enabled": true, "samples": 1 }),
        refusal: "out of range [2, 32]",
    },
    ToolCase {
        tool: "set_layer_styles",
        args: |f| {
            serde_json::json!({
                "clip": f.clip,
                "styles": {
                    "shadow": { "rgba": [0, 0, 0, 128], "offset": [4.0, 4.0], "blur": 8.0 },
                },
            })
        },
        // Omitted blocks are removed rather than kept.
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetClipLayerStyles {
            clip: f.clip_id(f.clip),
            styles: aria_models::LayerStyles {
                shadow: Some(aria_models::LayerShadow {
                    rgba: aria_models::Param::Constant([0, 0, 0, 128]),
                    offset: aria_models::Param::Constant([4.0, 4.0]),
                    blur: aria_models::Param::Constant(8.0),
                }),
                glow: None,
                outline: None,
                background: None,
            },
        }),
        invalid: |f| {
            serde_json::json!({
                "clip": f.clip,
                "styles": { "shadow": { "rgba": [0, 0, 0, 128], "blur": 8.0 } },
            })
        },
        refusal: "missing field `offset`",
    },
    ToolCase {
        tool: "set_clip_adjustments",
        args: |f| serde_json::json!({ "clip": f.clip, "brightness": 0.2, "sharpness": 0.5 }),
        expect: Expect::Edit(|f| {
            let mut adjust = f.clip_state(f.clip).adjust.clone();
            adjust.brightness = 0.2f32.into();
            adjust.sharpness = 0.5f32.into();
            aria_commands::EditCommand::SetClipAdjustments {
                clip: f.clip_id(f.clip),
                adjust,
            }
        }),
        // Signed sliders stop at ±1; the refusal names the range.
        invalid: |f| serde_json::json!({ "clip": f.clip, "brightness": 2.0 }),
        refusal: "brightness must be between -1 and 1",
    },
    ToolCase {
        tool: "set_clip_animation",
        args: |f| serde_json::json!({ "clip": f.clip, "slot": "in", "animation": "fade_in" }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetClipAnimation {
            clip: f.clip_id(f.clip),
            slot: aria_models::AnimationSlot::In,
            animation: Some(aria_models::AnimationRef {
                id: "fade_in".into(),
                speed: aria_models::ANIMATION_PARAM_DEFAULT,
                intensity: aria_models::ANIMATION_PARAM_DEFAULT,
                stagger: aria_models::ANIMATION_PARAM_DEFAULT,
            }),
        }),
        invalid: |f| {
            serde_json::json!({ "clip": f.clip, "slot": "in", "animation": "warp_drive" })
        },
        refusal: "unknown animation 'warp_drive'",
    },
];

#[cfg(test)]
mod tests {
    use super::super::cases;

    #[test]
    fn look_tools_lower_remap_and_refuse() {
        cases::check_group("look", super::CASES);
    }
}
