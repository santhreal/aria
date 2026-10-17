//! The agent-facing wire format: the JSON surface the LLM sees and emits.
//!
//! Deliberately *not* serde derives on `aria-commands` — the wire layer is
//! shaped for LLM ergonomics (times in fractional seconds, ids as plain
//! integers, flat tagged objects) and keeps internal refactors from silently
//! changing the prompt-visible schema. Lowering to real engine commands (and
//! every guardrail) lives in [`mod@crate::validate`].
//!
//! The vocabulary is closed by construction: project commands (open / save /
//! export / import) are not representable here, and [`WireGenerator`] carries
//! only the generator kinds the compositor actually renders — the phantom
//! sticker/effect/filter/adjustment variants cannot be expressed.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Bumped whenever the prompt-visible tool surface changes shape.
/// The snapshot test in `tests/tool_schema.rs` makes drift a reviewed diff.
///
/// 2: M2 keyframe commands (`set_param_keyframe`, `remove_param_keyframe`,
///    `set_param_constant`).
/// 3: M1 clip speed (`set_clip_speed`).
/// 4: M1 clip audio mix (`set_clip_audio`).
/// 5: M1 timeline markers (`add_marker`, `remove_marker`, `set_marker`).
/// 6: M1 crop + flip (`set_clip_crop`).
/// 7: subschemas inlined (no `$defs`/`$ref`) + `generator` field examples,
///    so small local models stop guessing nested argument shapes.
/// 8: M1 canvas settings (`set_canvas`).
/// 9: M4 effects (`add_effect`, `remove_effect`, `set_effect_param`).
/// 10: M4 transitions (`add_transition`, `remove_transition`, `set_transition`).
/// 11: M2 speed ramps (`set_speed_curve`).
/// 12: M8 volume envelopes (`volume` joins the keyframe param enum).
/// 13: M8 varispeed pitch lock (`set_clip_pitch`); retimed-audio descriptions
///     drop the "muted" language now that speed/reverse/ramp clips sound.
/// 14: M8 sidechain ducking (`duck`).
/// 15: shape generators gain optional width/height.
/// 16: optional `anchor_x`/`anchor_y` on `set_clip_transform`.
/// 17: M8 beat detection (`detect_beats`).
/// 18: M8 noise reduction (`set_denoise`).
/// 19: video clips carry their own audio (CapCut embedded audio) — the audio
///     tool descriptions drop the "linked audio companion" steering.
/// 20: look tools (mask/chroma/stabilize/filter/adjust/animation/audio_role);
///     removed unsupported `duck` and `detect_beats`.
/// 21: prompt extensions add the read-only `read_skill` tool.
/// 22: complete-group unlinking (`unlink_clips`) and bounded link-group lists.
/// 23: effect-chain reordering (`move_effect`).
/// 24: explicit-target audio extraction (`extract_audio`).
/// 25: explicit-target, property-preserving clip duplication (`duplicate_clip`).
/// 26: named easing presets (snappy/overshoot/anticipate) and raw bezier on
///     `set_param_keyframe`.
/// 27: `hold` (step) easing on `set_param_keyframe`.
/// 28: clip blend mode (`set_clip_blend_mode`).
/// 29: layer styles (`set_layer_styles`) and style params on keyframes.
/// 30: mask geometry (`center` / `size` / `rotation` / `roundness`) on
///     `set_clip_mask`.
/// 31: six new adjust sliders (tint/hue/highlights/shadows/sharpness/vignette)
///     on `set_clip_adjustments` and look keyframe params.
/// 32: typed effect params — Color via `rgba`, Vec2 via `position` on
///     `set_effect_param` / `set_param_keyframe` / `set_param_constant`.
/// 33: animatable crop — `crop` on `WireClipParam` with `rect: [x,y,w,h]`
///     on `set_param_keyframe` / `set_param_constant`.
/// 34: animatable pan — `pan` on `WireClipParam` (−1…+1 stereo balance).
/// 35: animatable text background radius/color on text keyframe params.
/// 36: per-axis scale — `set_clip_transform.scale` accepts a number or
///     `[x, y]`; scale keyframes accept `value` (uniform) or `position`.
/// 37: spatial motion-path tangents — optional `tangent_out` / `tangent_in`
///     on `set_param_keyframe` (position only, canvas-fraction handles).
/// 38: multi-keyframe easing presets — `apply_easing_preset` (bounce_out /
///     elastic_out / back_out) on scalar/vec2 animated params.
/// 39: per-clip transform motion blur (`set_motion_blur`).
/// 40: explicit canvas size — `set_canvas.resolution` (`[w, h]` in pixels,
///     `[0, 0]` derives it again from the aspect or the footage).
/// 41: marker notes — `comment` on `set_marker` (`""` clears the note,
///     omitted keeps it).
/// 42: overwrite and four-point edits — `overwrite_clip` places a clip over what
///     is already in the span it lands on, and `fit_to_fill` retimes a source
///     range to fill an exact timeline duration. Neither moves anything
///     downstream unless asked.
/// 43: per-lane routing — `set_track_targeted` picks which lane of a kind an
///     edit or a media drop lands on, and `set_track_sync_lock` says which
///     other lanes ride along with an insert.
/// 44: colour wheels — `adjust_lift_*`, `adjust_gamma_*`, and `adjust_gain_*`
///     on `WireLookParam`, so lift/gamma/gain (per channel and master) take
///     keyframes and constants by the same route as every other adjustment.
/// 45: a mixer to route: `Submix` joins the lane kinds `add_track` creates.
/// 46: mixer routing — `set_track_gain`, `set_track_output`, `add_send`,
///     `set_send` and `remove_send`, so a lane's fader, its output and the
///     copies other buses take are all reachable; every grade slider on
///     `set_clip_adjustments` (the lift, gamma and gain wheels included), and
///     an unknown adjustment argument refused by name rather than ignored.
/// 47: render and replace — `render_and_replace` starts a render of one clip
///     and reports that it started, because the file takes longer to write
///     than a turn lasts; `restore_unrendered` puts the clip back. The first
///     is the one call in the vocabulary that is not an edit: it lowers to
///     queued work the shell owns.
/// 48: `describe_project` takes an optional `from_seconds` / `to_seconds`
///     stretch, because a project longer than one description is now
///     described around the playhead and says how many clips it left out
///     rather than serializing every clip on every turn.
/// 49: `set_param_driver` attaches an `aria.expressions` driver to any
///     animatable property, or removes one.
/// 50: the grade gains whites, blacks, vibrance, faded film, the RGB curves,
///     the hue curves and the HSL secondary, as `set_clip_adjustments`
///     arguments and as animatable look properties.
/// 51: mask stacks — a clip holds up to 32 masks, each with a `mode`,
///     `opacity` and `expansion`. `add_clip_mask`, `set_clip_mask` (by
///     `index`), `remove_clip_mask` and `move_clip_mask` edit the stack, and
///     the `mask` keyframe param (`index` plus `param`) replaces the look's
///     `mask_*` params.
pub const TOOL_SCHEMA_VERSION: u32 = 51;

mod dtos;
mod tools;

pub(crate) use dtos::MAX_MULTI_CLIP_REFS;
pub use dtos::{
    AddClip, AddClipMask, AddEffect, AddGenerated, AddMarker, AddSend, AddTrack, AddTransition,
    ApplyEasingPreset, DuplicateClip, ExtractAudio, FitToFill, LinkClips, MoveClip, MoveClipMask,
    MoveEffect, OverwriteClip, RemoveClip, RemoveClipMask, RemoveEffect, RemoveMarker,
    RemoveParamKeyframe, RemoveSend, RemoveTrack, RemoveTransition, RenderAndReplace,
    RestoreUnrendered, RippleDelete, RippleInsert, SetAudioRole, SetCanvas, SetClipAdjustments,
    SetClipAnimation, SetClipAudio, SetClipBlendMode, SetClipChroma, SetClipCrop, SetClipFilter,
    SetClipLayerStyles, SetClipMask, SetClipMatte, SetClipPitch, SetClipSpeed, SetClipStabilize,
    SetClipTransform, SetDenoise, SetEffectParam, SetGenerator, SetMarker, SetMotionBlur,
    SetParamConstant, SetParamDriver, SetParamKeyframe, SetSend, SetSpeedCurve, SetTrackEnabled,
    SetTrackGain, SetTrackLocked, SetTrackMuted, SetTrackOutput, SetTrackSyncLock,
    SetTrackTargeted, SetTransition, ShiftClips, SplitClip, TrimClip, UnlinkClips,
    WireAnimationSlot, WireAudioRole, WireBlendMode, WireCanvasAspect, WireChromaKey,
    WireClipParam, WireEasing, WireEasingPreset, WireFilter, WireGenerator, WireLayerBackground,
    WireLayerGlow, WireLayerOutline, WireLayerShadow, WireLayerStyles, WireLookParam,
    WireMarkerColor, WireMask, WireMaskKind, WireMaskMode, WireMaskParam, WireMatteSource,
    WireScale, WireShape, WireShapeParam, WireStabilizeLevel, WireStyleParam, WireTextParam,
    WireTrackKind, WireTrackMatte,
};
pub use tools::{ToolGroup, ToolSpec, describe_project_spec};

/// Declare the complete assistant command surface.
///
/// Each row generates the command variant, DTO binding, group membership,
/// schema input, name decoder, reverse name, and test-case group hook.
macro_rules! wire_tool_registry {
    (
        $(
            $group:ident {
                $( $name:literal => $variant:ident ( $args:ty ); )+
            }
        )+
    ) => {
        /// Every timeline edit the agent may request, as one tagged value.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
        #[serde(tag = "command", rename_all = "snake_case")]
        pub enum WireCommand {
            $( $( $variant($args), )+ )+
        }

        const TOOL_GROUPS: &[ToolGroup] = &[
            $(
                ToolGroup {
                    name: stringify!($group),
                    tools: &[ $( $name ),+ ],
                },
            )+
        ];

        /// Every tool group, in the order its rows are offered.
        pub fn tool_groups() -> &'static [ToolGroup] {
            TOOL_GROUPS
        }

        /// Every tool name, in registry order.
        pub fn tool_names() -> impl Iterator<Item = &'static str> {
            tool_groups()
                .iter()
                .flat_map(|group| group.tools.iter().copied())
        }

        fn generate_tool_spec(name: &str) -> Option<ToolSpec> {
            match name {
                $(
                    $(
                        $name => Some(tools::spec::<$args>(
                            $name,
                            tools::description($name)
                                .expect("every registry row has a description"),
                        )),
                    )+
                )+
                _ => None,
            }
        }

        static CACHED_TOOL_SPECS: std::sync::LazyLock<Vec<ToolSpec>> =
            std::sync::LazyLock::new(|| {
                tool_names()
                    .map(|name| generate_tool_spec(name).expect("a registry name has a spec"))
                    .collect()
            });

        static AVAILABLE_TOOLS_STR: std::sync::LazyLock<String> =
            std::sync::LazyLock::new(|| {
                tool_names().collect::<Vec<_>>().join(", ")
            });

        /// Precomputed comma-separated list of all available tool names.
        pub fn available_tools_str() -> &'static str {
            &AVAILABLE_TOOLS_STR
        }

        /// The full tool surface, in registry order.
        pub fn tool_specs() -> Vec<ToolSpec> {
            CACHED_TOOL_SPECS.clone()
        }

        /// The full tool surface, in registry order, borrowed from static cache.
        pub fn tool_specs_slice() -> &'static [ToolSpec] {
            &CACHED_TOOL_SPECS
        }

        /// Look up a cached tool specification by name.
        pub fn tool_spec(name: &str) -> Option<&'static ToolSpec> {
            tool_specs_slice().iter().find(|spec| spec.name == name)
        }

        impl WireCommand {
            /// The provider-facing name of this command.
            pub fn tool_name(&self) -> &'static str {
                match self {
                    $( $( Self::$variant(_) => $name, )+ )+
                }
            }

            /// Decode a provider tool call.
            pub fn from_tool_call(
                name: &str,
                arguments: serde_json::Value,
            ) -> Result<WireCommand, String> {
                match name {
                    $(
                        $(
                            $name => tools::decode_args::<$args>($name, &arguments)
                                .map(Self::$variant),
                        )+
                    )+
                    _ => Err(format!(
                        "unknown tool '{name}'; available tools: {}",
                        available_tools_str()
                    )),
                }
            }
        }
    };
}

wire_tool_registry! {
    tracks {
        "add_track" => AddTrack(AddTrack);
        "remove_track" => RemoveTrack(RemoveTrack);
        "set_track_enabled" => SetTrackEnabled(SetTrackEnabled);
        "set_track_muted" => SetTrackMuted(SetTrackMuted);
        "set_track_locked" => SetTrackLocked(SetTrackLocked);
        "set_track_targeted" => SetTrackTargeted(SetTrackTargeted);
        "set_track_sync_lock" => SetTrackSyncLock(SetTrackSyncLock);
    }
    clips {
        "add_clip" => AddClip(AddClip);
        "extract_audio" => ExtractAudio(ExtractAudio);
        "duplicate_clip" => DuplicateClip(DuplicateClip);
        "add_generated" => AddGenerated(AddGenerated);
        "set_generator" => SetGenerator(SetGenerator);
        "split_clip" => SplitClip(SplitClip);
        "trim_clip" => TrimClip(TrimClip);
        "move_clip" => MoveClip(MoveClip);
        "remove_clip" => RemoveClip(RemoveClip);
    }
    timeline {
        "ripple_delete" => RippleDelete(RippleDelete);
        "shift_clips" => ShiftClips(ShiftClips);
        "ripple_insert" => RippleInsert(RippleInsert);
        "overwrite_clip" => OverwriteClip(OverwriteClip);
        "fit_to_fill" => FitToFill(FitToFill);
        "link_clips" => LinkClips(LinkClips);
        "unlink_clips" => UnlinkClips(UnlinkClips);
    }
    effects {
        "add_effect" => AddEffect(AddEffect);
        "remove_effect" => RemoveEffect(RemoveEffect);
        "move_effect" => MoveEffect(MoveEffect);
        "set_effect_param" => SetEffectParam(SetEffectParam);
    }
    transitions {
        "add_transition" => AddTransition(AddTransition);
        "remove_transition" => RemoveTransition(RemoveTransition);
        "set_transition" => SetTransition(SetTransition);
    }
    animation {
        "set_param_keyframe" => SetParamKeyframe(SetParamKeyframe);
        "remove_param_keyframe" => RemoveParamKeyframe(RemoveParamKeyframe);
        "set_param_constant" => SetParamConstant(SetParamConstant);
        "set_param_driver" => SetParamDriver(SetParamDriver);
        "apply_easing_preset" => ApplyEasingPreset(ApplyEasingPreset);
    }
    retime {
        "set_clip_speed" => SetClipSpeed(SetClipSpeed);
        "set_speed_curve" => SetSpeedCurve(SetSpeedCurve);
        "set_clip_pitch" => SetClipPitch(SetClipPitch);
    }
    audio {
        "set_clip_audio" => SetClipAudio(SetClipAudio);
        "set_denoise" => SetDenoise(SetDenoise);
        "set_audio_role" => SetAudioRole(SetAudioRole);
    }
    routing {
        "set_track_gain" => SetTrackGain(SetTrackGain);
        "set_track_output" => SetTrackOutput(SetTrackOutput);
        "add_send" => AddSend(AddSend);
        "set_send" => SetSend(SetSend);
        "remove_send" => RemoveSend(RemoveSend);
    }
    look {
        "set_clip_transform" => SetClipTransform(SetClipTransform);
        "set_clip_crop" => SetClipCrop(SetClipCrop);
        "add_clip_mask" => AddClipMask(AddClipMask);
        "set_clip_mask" => SetClipMask(SetClipMask);
        "remove_clip_mask" => RemoveClipMask(RemoveClipMask);
        "move_clip_mask" => MoveClipMask(MoveClipMask);
        "set_clip_chroma" => SetClipChroma(SetClipChroma);
        "set_clip_matte" => SetClipMatte(SetClipMatte);
        "set_clip_stabilize" => SetClipStabilize(SetClipStabilize);
        "set_clip_filter" => SetClipFilter(SetClipFilter);
        "set_clip_blend_mode" => SetClipBlendMode(SetClipBlendMode);
        "set_motion_blur" => SetMotionBlur(SetMotionBlur);
        "set_layer_styles" => SetClipLayerStyles(SetClipLayerStyles);
        "set_clip_adjustments" => SetClipAdjustments(SetClipAdjustments);
        "set_clip_animation" => SetClipAnimation(SetClipAnimation);
    }
    markers {
        "add_marker" => AddMarker(AddMarker);
        "remove_marker" => RemoveMarker(RemoveMarker);
        "set_marker" => SetMarker(SetMarker);
    }
    bake {
        "render_and_replace" => RenderAndReplace(RenderAndReplace);
        "restore_unrendered" => RestoreUnrendered(RestoreUnrendered);
    }
    project {
        "set_canvas" => SetCanvas(SetCanvas);
    }
}

impl WireCommand {
    /// Rewrite clip/track/marker references through the given maps (ids
    /// absent from a map pass through unchanged).
    ///
    /// This is what makes plan replay work: a plan is recorded against a
    /// sandbox where `add_track`/`split_clip` allocated sandbox-local ids;
    /// when the live engine replays the plan, each created entity gets a
    /// fresh id, and later steps that referenced the sandbox id must be
    /// remapped onto the real one.
    pub fn remap_ids(
        &mut self,
        clip_map: &std::collections::HashMap<u64, u64>,
        track_map: &std::collections::HashMap<u64, u64>,
        marker_map: &std::collections::HashMap<u64, u64>,
    ) {
        let clip = |id: &mut u64| {
            if let Some(mapped) = clip_map.get(id) {
                *id = *mapped;
            }
        };
        let track = |id: &mut u64| {
            if let Some(mapped) = track_map.get(id) {
                *id = *mapped;
            }
        };
        let marker = |id: &mut u64| {
            if let Some(mapped) = marker_map.get(id) {
                *id = *mapped;
            }
        };
        match self {
            WireCommand::AddTrack(_) => {}
            WireCommand::AddClip(a) => track(&mut a.track),
            WireCommand::ExtractAudio(a) => {
                clip(&mut a.clip);
                track(&mut a.track);
            }
            WireCommand::DuplicateClip(a) => {
                clip(&mut a.clip);
                track(&mut a.to_track);
            }
            WireCommand::AddGenerated(a) => track(&mut a.track),
            WireCommand::SetGenerator(a) => clip(&mut a.clip),
            WireCommand::SetClipTransform(a) => clip(&mut a.clip),
            WireCommand::SetClipCrop(a) => clip(&mut a.clip),
            WireCommand::AddEffect(a) => clip(&mut a.clip),
            WireCommand::RemoveEffect(a) => clip(&mut a.clip),
            WireCommand::MoveEffect(a) => clip(&mut a.clip),
            WireCommand::SetEffectParam(a) => clip(&mut a.clip),
            WireCommand::AddTransition(a) => clip(&mut a.clip),
            WireCommand::RemoveTransition(a) => clip(&mut a.clip),
            WireCommand::SetTransition(a) => clip(&mut a.clip),
            WireCommand::SetParamKeyframe(a) => clip(&mut a.clip),
            WireCommand::RemoveParamKeyframe(a) => clip(&mut a.clip),
            WireCommand::SetParamConstant(a) => clip(&mut a.clip),
            WireCommand::SetParamDriver(a) => clip(&mut a.clip),
            WireCommand::ApplyEasingPreset(a) => clip(&mut a.clip),
            WireCommand::SetClipSpeed(a) => clip(&mut a.clip),
            WireCommand::SetSpeedCurve(a) => clip(&mut a.clip),
            WireCommand::SetClipPitch(a) => clip(&mut a.clip),
            WireCommand::SetClipAudio(a) => clip(&mut a.clip),
            WireCommand::SetDenoise(a) => clip(&mut a.clip),
            WireCommand::AddClipMask(a) => clip(&mut a.clip),
            WireCommand::SetClipMask(a) => clip(&mut a.clip),
            WireCommand::RemoveClipMask(a) => clip(&mut a.clip),
            WireCommand::MoveClipMask(a) => clip(&mut a.clip),
            WireCommand::SetClipChroma(a) => clip(&mut a.clip),
            WireCommand::SetClipMatte(a) => clip(&mut a.clip),
            WireCommand::SetClipStabilize(a) => clip(&mut a.clip),
            WireCommand::SetClipFilter(a) => clip(&mut a.clip),
            WireCommand::SetClipBlendMode(a) => clip(&mut a.clip),
            WireCommand::SetMotionBlur(a) => clip(&mut a.clip),
            WireCommand::SetClipLayerStyles(a) => clip(&mut a.clip),
            WireCommand::SetClipAdjustments(a) => clip(&mut a.clip),
            WireCommand::SetClipAnimation(a) => clip(&mut a.clip),
            WireCommand::SetAudioRole(a) => clip(&mut a.clip),
            WireCommand::SplitClip(a) => clip(&mut a.clip),
            WireCommand::TrimClip(a) => clip(&mut a.clip),
            WireCommand::MoveClip(a) => {
                clip(&mut a.clip);
                track(&mut a.to_track);
            }
            WireCommand::RemoveClip(a) => clip(&mut a.clip),
            WireCommand::RemoveTrack(a) => track(&mut a.track),
            WireCommand::SetTrackEnabled(a) => track(&mut a.track),
            WireCommand::SetTrackMuted(a) => track(&mut a.track),
            WireCommand::SetTrackLocked(a) => track(&mut a.track),
            WireCommand::SetTrackTargeted(a) => track(&mut a.track),
            WireCommand::SetTrackSyncLock(a) => track(&mut a.track),
            WireCommand::SetTrackGain(a) => track(&mut a.track),
            WireCommand::SetTrackOutput(a) => {
                track(&mut a.track);
                if let Some(to) = a.to.as_mut() {
                    track(to);
                }
            }
            WireCommand::AddSend(a) => {
                track(&mut a.track);
                track(&mut a.to);
            }
            WireCommand::SetSend(a) => track(&mut a.track),
            WireCommand::RemoveSend(a) => track(&mut a.track),
            WireCommand::RippleDelete(a) => clip(&mut a.clip),
            WireCommand::ShiftClips(a) => track(&mut a.track),
            WireCommand::RippleInsert(a) => track(&mut a.track),
            WireCommand::OverwriteClip(a) => track(&mut a.track),
            WireCommand::FitToFill(a) => track(&mut a.track),
            WireCommand::LinkClips(a) => a.clips.iter_mut().for_each(clip),
            WireCommand::UnlinkClips(a) => a.clips.iter_mut().for_each(clip),
            WireCommand::AddMarker(_) => {}
            WireCommand::RemoveMarker(a) => marker(&mut a.marker),
            WireCommand::SetMarker(a) => marker(&mut a.marker),
            WireCommand::SetCanvas(_) => {}
            WireCommand::RenderAndReplace(a) => clip(&mut a.clip),
            WireCommand::RestoreUnrendered(a) => clip(&mut a.clip),
        }
    }
}

#[cfg(test)]
mod tests;
