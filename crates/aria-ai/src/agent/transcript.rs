//! The transcript line for one applied command, in editor language.
//!
//! One arm per wire command, and the same string is the chat entry, the undo
//! tooltip, and the format the eval tests assert against. Writing it in editor
//! language rather than in the wire shape is the point: the operator reads
//! "split clip 7 at 12.40s", not a JSON call, and a command whose line reads
//! wrong is a command whose meaning was wrong.

use aria_commands::EditOutcome;

use crate::wire::{self, WireCommand};

/// Transcript line for one host call: the first line of its output,
/// capped so the panel never renders a wall of tool text.
pub(crate) fn host_action_summary(text: &str) -> String {
    const MAX_CHARS: usize = 120;
    let line = text.lines().next().unwrap_or("").trim();
    let mut summary: String = line.chars().take(MAX_CHARS).collect();
    if line.chars().count() > MAX_CHARS {
        summary.push('…');
    }
    summary
}

// --- action log ---------------------------------------------------------

pub(crate) fn secs(v: f64) -> String {
    format!("{v:.2}s")
}

pub(crate) fn rgba(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
}

/// `[1, 2, 3]` → `"1, 2, 3"`. The `map(to_string).collect::<Vec<_>>().join()`
/// this replaces allocated one `String` per id plus the `Vec` holding them,
/// all to build a string this writes in one buffer.
pub(crate) fn id_list(ids: &[u64]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(ids.len() * 4);
    for (index, id) in ids.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{id}");
    }
    out
}

pub(crate) fn param_name(param: &wire::WireClipParam) -> String {
    match param {
        wire::WireClipParam::Position => "position".into(),
        wire::WireClipParam::AnchorPoint => "anchor point".into(),
        wire::WireClipParam::Scale => "scale".into(),
        wire::WireClipParam::Rotation => "rotation".into(),
        wire::WireClipParam::Opacity => "opacity".into(),
        wire::WireClipParam::Crop => "crop".into(),
        wire::WireClipParam::Volume => "volume".into(),
        wire::WireClipParam::Pan => "pan".into(),
        wire::WireClipParam::Speed => "speed".into(),
        wire::WireClipParam::Effect { index, param } => format!("effect {index} {param}"),
        wire::WireClipParam::Shape { param } => format!("shape {param:?}").to_lowercase(),
        wire::WireClipParam::Text { param } => format!("text {param:?}").to_lowercase(),
        wire::WireClipParam::Look { param } => format!("look {param:?}").to_lowercase(),
        wire::WireClipParam::Style { param } => format!("style {param:?}").to_lowercase(),
    }
}

/// The keyframed value in editor language: "scale 150%", "rotation 90°",
/// "[0.25, -0.10]". Falls back to "?" when the call omitted the value (the
/// validation rejection carries the real message).
pub(crate) fn param_value_phrase(
    param: &wire::WireClipParam,
    value: Option<f64>,
    position: Option<[f64; 2]>,
    color: Option<[u8; 4]>,
    rect: Option<[f64; 4]>,
) -> String {
    // Shape first, read off the payload that arrived rather than off a table of
    // which selector carries which kind. That table was the stale part: it
    // named every colour and every coordinate property by hand, so the next
    // colour property added to the wire would have printed as a bare number
    // under the fallthrough. A value cannot be wrong about its own shape.
    if let Some(r) = rect {
        return format!("[{:.2}, {:.2}, {:.2}, {:.2}]", r[0], r[1], r[2], r[3]);
    }
    if let Some(c) = color {
        return rgba(c);
    }
    if let Some(p) = position {
        // Scale is a factor pair, not a normalised coordinate: 1.5 rounded to
        // two places reads as a position, so it keeps the numbers it was given.
        return if matches!(param, wire::WireClipParam::Scale) {
            format!("[{}, {}]", p[0], p[1])
        } else {
            format!("[{:.2}, {:.2}]", p[0], p[1])
        };
    }
    let Some(v) = value else {
        return "?".into();
    };
    // What is left is the unit a scalar reads in, and it is exhaustive with no
    // `_` arm: a new selector has to say whether it is a percentage, an angle,
    // or a plain number, and until it does this file will not compile.
    match param {
        wire::WireClipParam::Scale | wire::WireClipParam::Opacity | wire::WireClipParam::Volume => {
            format!("{:.0}%", v * 100.0)
        }
        wire::WireClipParam::Rotation => format!("{v:.0}°"),
        wire::WireClipParam::Pan => format!("{v:.2}"),
        wire::WireClipParam::Position
        | wire::WireClipParam::AnchorPoint
        | wire::WireClipParam::Crop
        | wire::WireClipParam::Speed
        | wire::WireClipParam::Effect { .. }
        | wire::WireClipParam::Shape { .. }
        | wire::WireClipParam::Text { .. }
        | wire::WireClipParam::Look { .. }
        | wire::WireClipParam::Style { .. } => v.to_string(),
    }
}

pub(crate) fn generator_phrase(generator: &wire::WireGenerator) -> String {
    match generator {
        wire::WireGenerator::Text { content } => format!("text '{content}'"),
        wire::WireGenerator::Solid { rgba: c } => format!("solid {}", rgba(*c)),
        wire::WireGenerator::Shape {
            shape,
            rgba: c,
            width,
            height,
        } => {
            let name = match shape {
                wire::WireShape::Rectangle => "rectangle",
                wire::WireShape::Ellipse => "ellipse",
            };
            let size = match (width, height) {
                (Some(w), Some(h)) => format!(" {w:.0}×{h:.0} ref px"),
                _ => String::new(),
            };
            format!("{} {}{}", rgba(*c), name, size)
        }
    }
}

/// One transcript line per command: what happened, in editor language.
/// `outcome` is `None` for dry-run (planned, not applied).
pub fn describe_action(command: &WireCommand, outcome: Option<&EditOutcome>) -> String {
    let mut line = match command {
        WireCommand::AddTrack(a) => format!("added {:?} track '{}'", a.kind, a.name).to_lowercase(),
        WireCommand::AddClip(a) => format!(
            "placed media {} ({}–{} of source) at {} on track {}",
            a.media,
            secs(a.source_start),
            secs(a.source_start + a.source_duration),
            secs(a.start),
            a.track,
        ),
        WireCommand::ExtractAudio(a) => {
            format!(
                "extracted audio from clip {} onto track {}",
                a.clip, a.track
            )
        }
        WireCommand::DuplicateClip(a) => format!(
            "duplicated clip {} onto track {} at {}",
            a.clip,
            a.to_track,
            secs(a.start),
        ),
        WireCommand::AddGenerated(a) => format!(
            "added {} at {} for {} on track {}",
            generator_phrase(&a.generator),
            secs(a.start),
            secs(a.duration),
            a.track,
        ),
        WireCommand::SetGenerator(a) => format!(
            "changed clip {} to {}",
            a.clip,
            generator_phrase(&a.generator)
        ),
        WireCommand::SetClipTransform(a) => {
            let mut parts = Vec::new();
            if a.position_x.is_some() || a.position_y.is_some() {
                parts.push("position".to_string());
            }
            if let Some(s) = a.scale {
                parts.push(match s {
                    wire::WireScale::Uniform(u) => format!("scale {:.0}%", u * 100.0),
                    wire::WireScale::Axes([x, y]) => format!("scale: [{x}, {y}]"),
                });
            }
            if let Some(r) = a.rotation {
                parts.push(format!("rotation {r:.0}°"));
            }
            if let Some(o) = a.opacity {
                parts.push(format!("opacity {:.0}%", o * 100.0));
            }
            format!("set clip {} {}", a.clip, parts.join(", "))
        }
        WireCommand::SetClipCrop(a) => {
            let mut parts = Vec::new();
            let edges: Vec<String> = [
                ("left", a.left),
                ("top", a.top),
                ("right", a.right),
                ("bottom", a.bottom),
            ]
            .iter()
            .filter_map(|(name, v)| v.map(|v| format!("{name} {:.0}%", v * 100.0)))
            .collect();
            if !edges.is_empty() {
                parts.push(format!("cropped {}", edges.join(", ")));
            }
            if let Some(h) = a.flip_h {
                parts.push(
                    if h {
                        "flipped horizontally"
                    } else {
                        "unflipped horizontally"
                    }
                    .into(),
                );
            }
            if let Some(v) = a.flip_v {
                parts.push(
                    if v {
                        "flipped vertically"
                    } else {
                        "unflipped vertically"
                    }
                    .into(),
                );
            }
            if parts.is_empty() {
                parts.push("framing unchanged".into());
            }
            format!("set clip {} {}", a.clip, parts.join(", "))
        }
        WireCommand::SetParamKeyframe(a) => format!(
            "keyframed clip {} {} = {} at {}",
            a.clip,
            param_name(&a.param),
            param_value_phrase(&a.param, a.value, a.position, a.rgba, a.rect),
            secs(a.at),
        ),
        WireCommand::RemoveParamKeyframe(a) => format!(
            "removed clip {} {} keyframe at {}",
            a.clip,
            param_name(&a.param),
            secs(a.at),
        ),
        WireCommand::SetParamConstant(a) => format!(
            "set clip {} {} to {} (animation cleared)",
            a.clip,
            param_name(&a.param),
            param_value_phrase(&a.param, a.value, a.position, a.rgba, a.rect),
        ),
        WireCommand::ApplyEasingPreset(a) => format!(
            "applied {} easing preset on clip {} {} from {}",
            match a.preset {
                crate::wire::WireEasingPreset::BounceOut => "bounce_out",
                crate::wire::WireEasingPreset::ElasticOut => "elastic_out",
                crate::wire::WireEasingPreset::BackOut => "back_out",
            },
            a.clip,
            param_name(&a.param),
            secs(a.from_tick),
        ),
        WireCommand::SetClipSpeed(a) => {
            let mut parts = Vec::new();
            if let Some(s) = a.speed {
                parts.push(format!("speed {s}x"));
            }
            if let Some(r) = a.reversed {
                parts.push(if r {
                    "reversed".into()
                } else {
                    "forward".to_string()
                });
            }
            if parts.is_empty() {
                parts.push("retiming unchanged".into());
            }
            format!("set clip {} {}", a.clip, parts.join(", "))
        }
        WireCommand::SetSpeedCurve(a) => match &a.preset {
            Some(preset) => format!("applied {preset} speed ramp to clip {}", a.clip),
            None => format!("cleared speed ramp on clip {}", a.clip),
        },
        WireCommand::SetClipPitch(a) => format!(
            "set clip {} pitch to {}",
            a.clip,
            if a.preserve_pitch {
                "preserved"
            } else {
                "follow speed"
            }
        ),
        WireCommand::SetDenoise(a) => format!(
            "turned noise reduction {} on clip {}",
            if a.denoise { "on" } else { "off" },
            a.clip
        ),
        WireCommand::SetClipMask(a) => match &a.mask {
            Some(mask) => format!(
                "set {} mask on clip {}",
                match mask.kind {
                    crate::wire::WireMaskKind::Linear => "linear",
                    crate::wire::WireMaskKind::Mirror => "mirror",
                    crate::wire::WireMaskKind::Circle => "circle",
                    crate::wire::WireMaskKind::Rectangle => "rectangle",
                    crate::wire::WireMaskKind::Heart => "heart",
                    crate::wire::WireMaskKind::Star => "star",
                },
                a.clip
            ),
            None => format!("cleared mask on clip {}", a.clip),
        },
        WireCommand::SetClipChroma(a) => match &a.chroma {
            Some(_) => format!("set chroma key on clip {}", a.clip),
            None => format!("cleared chroma key on clip {}", a.clip),
        },
        WireCommand::SetClipMatte(a) => match &a.matte {
            Some(m) => format!(
                "set {} track matte on clip {}",
                match m.source {
                    crate::wire::WireMatteSource::Alpha => "alpha",
                    crate::wire::WireMatteSource::Luma => "luma",
                },
                a.clip
            ),
            None => format!("cleared track matte on clip {}", a.clip),
        },
        WireCommand::SetClipStabilize(a) => match a.level {
            Some(level) => format!(
                "set {} stabilization on clip {}",
                match level {
                    crate::wire::WireStabilizeLevel::Recommended => "recommended",
                    crate::wire::WireStabilizeLevel::Smooth => "smooth",
                    crate::wire::WireStabilizeLevel::MaxSmooth => "max smooth",
                },
                a.clip
            ),
            None => format!("cleared stabilization on clip {}", a.clip),
        },
        WireCommand::SetClipFilter(a) => match &a.filter {
            Some(filter) => format!("set {} filter on clip {}", filter.id, a.clip),
            None => format!("cleared filter on clip {}", a.clip),
        },
        WireCommand::SetClipBlendMode(a) => format!(
            "set clip {} blend mode to {}",
            a.clip,
            match a.mode {
                crate::wire::WireBlendMode::Normal => "normal",
                crate::wire::WireBlendMode::Darken => "darken",
                crate::wire::WireBlendMode::Multiply => "multiply",
                crate::wire::WireBlendMode::ColorBurn => "color_burn",
                crate::wire::WireBlendMode::Lighten => "lighten",
                crate::wire::WireBlendMode::Screen => "screen",
                crate::wire::WireBlendMode::ColorDodge => "color_dodge",
                crate::wire::WireBlendMode::Add => "add",
                crate::wire::WireBlendMode::Overlay => "overlay",
                crate::wire::WireBlendMode::SoftLight => "soft_light",
                crate::wire::WireBlendMode::HardLight => "hard_light",
                crate::wire::WireBlendMode::Difference => "difference",
                crate::wire::WireBlendMode::Exclusion => "exclusion",
            }
        ),
        WireCommand::SetMotionBlur(a) => {
            if a.enabled {
                let mut parts = vec![format!("enable motion blur on clip {}", a.clip)];
                if let Some(s) = a.shutter_deg {
                    parts.push(format!("shutter={s}"));
                }
                if let Some(n) = a.samples {
                    parts.push(format!("samples={n}"));
                }
                parts.join(" ")
            } else {
                format!("disable motion blur on clip {}", a.clip)
            }
        }
        WireCommand::SetClipLayerStyles(a) => {
            let mut blocks = Vec::new();
            if a.styles.shadow.is_some() {
                blocks.push("shadow");
            }
            if a.styles.glow.is_some() {
                blocks.push("glow");
            }
            if a.styles.outline.is_some() {
                blocks.push("outline");
            }
            if a.styles.background.is_some() {
                blocks.push("background");
            }
            if blocks.is_empty() {
                format!("cleared layer styles on clip {}", a.clip)
            } else {
                format!(
                    "set layer styles ({}) on clip {}",
                    blocks.join(", "),
                    a.clip
                )
            }
        }
        WireCommand::SetClipAdjustments(a) => {
            // One phrase per slider the call actually names, in row order.
            let mut parts: Vec<String> = a
                .writes()
                .map(|(slider, value)| format!("{} {value:.2}", slider.name))
                .collect();
            if parts.is_empty() {
                parts.push("adjustments unchanged".into());
            }
            format!("set clip {} {}", a.clip, parts.join(", "))
        }
        WireCommand::SetClipAnimation(a) => {
            let slot = match a.slot {
                crate::wire::WireAnimationSlot::In => "in",
                crate::wire::WireAnimationSlot::Out => "out",
                crate::wire::WireAnimationSlot::Combo => "combo",
            };
            match &a.animation {
                Some(id) => format!("set {} animation on clip {} ({} slot)", id, a.clip, slot),
                None => format!("cleared {} animation on clip {}", slot, a.clip),
            }
        }
        WireCommand::SetAudioRole(a) => match a.role {
            Some(role) => format!(
                "tagged clip {} as {}",
                a.clip,
                match role {
                    crate::wire::WireAudioRole::Music => "music",
                    crate::wire::WireAudioRole::Sfx => "sfx",
                    crate::wire::WireAudioRole::Voiceover => "voiceover",
                    crate::wire::WireAudioRole::Extracted => "extracted",
                }
            ),
            None => format!("cleared audio role on clip {}", a.clip),
        },
        WireCommand::SetClipAudio(a) => {
            let mut parts = Vec::new();
            if let Some(v) = a.volume {
                parts.push(if v == 0.0 {
                    "muted".to_string()
                } else {
                    format!("volume {:.0}%", v * 100.0)
                });
            }
            if let Some(f) = a.fade_in {
                parts.push(format!("fade in {}", secs(f)));
            }
            if let Some(f) = a.fade_out {
                parts.push(format!("fade out {}", secs(f)));
            }
            if parts.is_empty() {
                parts.push("audio unchanged".into());
            }
            format!("set clip {} {}", a.clip, parts.join(", "))
        }
        WireCommand::AddEffect(a) => format!("added {} effect to clip {}", a.effect, a.clip),
        WireCommand::RemoveEffect(a) => {
            format!("removed effect {} from clip {}", a.index, a.clip)
        }
        WireCommand::MoveEffect(a) => format!(
            "moved effect {} to {} on clip {}",
            a.from_index, a.to_index, a.clip
        ),
        WireCommand::SetEffectParam(a) => {
            let shown = if let Some(c) = a.rgba {
                rgba(c)
            } else if let Some(p) = a.position {
                format!("[{:.2}, {:.2}]", p[0], p[1])
            } else {
                a.value.map(|v| v.to_string()).unwrap_or_else(|| "?".into())
            };
            format!(
                "set clip {} effect {} {} = {}",
                a.clip, a.index, a.param, shown
            )
        }
        WireCommand::AddTransition(a) => {
            format!("added {} transition after clip {}", a.transition, a.clip)
        }
        WireCommand::RemoveTransition(a) => {
            format!("removed transition after clip {}", a.clip)
        }
        WireCommand::SetTransition(a) => {
            format!("set transition after clip {} to {}s", a.clip, a.seconds)
        }
        WireCommand::SplitClip(a) => format!("split clip {} at {}", a.clip, secs(a.at)),
        WireCommand::TrimClip(a) => format!(
            "trimmed clip {} to {}–{}",
            a.clip,
            secs(a.start),
            secs(a.start + a.duration)
        ),
        WireCommand::MoveClip(a) => {
            format!(
                "moved clip {} to {} on track {}",
                a.clip,
                secs(a.start),
                a.to_track
            )
        }
        WireCommand::RemoveClip(a) => format!("removed clip {}", a.clip),
        WireCommand::RemoveTrack(a) => format!("removed track {}", a.track),
        WireCommand::SetTrackEnabled(a) => format!(
            "{} track {}",
            if a.enabled { "showed" } else { "hid" },
            a.track
        ),
        WireCommand::SetTrackMuted(a) => format!(
            "{} track {}",
            if a.muted { "muted" } else { "unmuted" },
            a.track
        ),
        WireCommand::SetTrackLocked(a) => format!(
            "{} track {}",
            if a.locked { "locked" } else { "unlocked" },
            a.track
        ),
        WireCommand::SetTrackTargeted(a) => format!(
            "{} track {}",
            if a.targeted { "targeted" } else { "untargeted" },
            a.track
        ),
        WireCommand::SetTrackSyncLock(a) => format!(
            "{} sync lock on track {}",
            if a.sync_lock { "set" } else { "cleared" },
            a.track
        ),
        WireCommand::SetTrackGain(a) => {
            format!("set track {} fader to {:.2}x", a.track, a.gain)
        }
        WireCommand::SetTrackOutput(a) => match a.to {
            Some(bus) => format!("routed track {} into bus {bus}", a.track),
            None => format!("routed track {} straight to the master", a.track),
        },
        WireCommand::AddSend(a) => format!(
            "sent track {} to bus {} at {:.2}x, {}",
            a.track,
            a.to,
            a.level.unwrap_or(1.0),
            if a.pre_fader.unwrap_or(false) {
                "pre-fader"
            } else {
                "post-fader"
            }
        ),
        WireCommand::SetSend(a) => {
            let mut parts: Vec<String> = Vec::new();
            if let Some(level) = a.level {
                parts.push(format!("level {level:.2}x"));
            }
            if let Some(pre_fader) = a.pre_fader {
                parts.push(if pre_fader { "pre-fader" } else { "post-fader" }.to_string());
            }
            format!(
                "set send {} on track {} to {}",
                a.index,
                a.track,
                parts.join(", ")
            )
        }
        WireCommand::RemoveSend(a) => {
            format!("removed send {} from track {}", a.index, a.track)
        }
        WireCommand::RippleDelete(a) => {
            format!(
                "ripple-deleted clip {} (later clips closed the gap)",
                a.clip
            )
        }
        WireCommand::ShiftClips(a) => format!(
            "shifted clips on track {} from {} by {:+.2}s",
            a.track,
            secs(a.from),
            a.delta
        ),
        WireCommand::RippleInsert(a) => format!(
            "ripple-inserted media {} at {} on track {} (later clips moved right)",
            a.media,
            secs(a.at),
            a.track
        ),
        WireCommand::OverwriteClip(a) => format!(
            "overwrote media {} at {} on track {} (nothing downstream moved)",
            a.media,
            secs(a.at),
            a.track
        ),
        WireCommand::FitToFill(a) => format!(
            "fitted media {} into {} at {} on track {}",
            a.media,
            secs(a.duration),
            secs(a.at),
            a.track
        ),
        WireCommand::LinkClips(a) => format!("linked clips {}", id_list(&a.clips)),
        WireCommand::UnlinkClips(a) => format!(
            "unlinked complete groups touched by clips {}",
            id_list(&a.clips)
        ),
        WireCommand::AddMarker(a) => {
            let name = match &a.name {
                Some(name) if !name.is_empty() => format!(" '{name}'"),
                _ => String::new(),
            };
            let color = a
                .color
                .map(|c| format!(" ({c:?})").to_lowercase())
                .unwrap_or_default();
            format!("added marker{name} at {}{color}", secs(a.at))
        }
        WireCommand::RemoveMarker(a) => format!("removed marker {}", a.marker),
        WireCommand::SetMarker(a) => {
            let mut parts = Vec::new();
            if let Some(at) = a.at {
                parts.push(format!("moved to {}", secs(at)));
            }
            if let Some(name) = &a.name {
                parts.push(format!("named '{name}'"));
            }
            if let Some(color) = a.color {
                parts.push(format!("colored {color:?}").to_lowercase());
            }
            if let Some(comment) = &a.comment {
                parts.push(if comment.is_empty() {
                    "comment cleared".into()
                } else {
                    format!("commented '{comment}'")
                });
            }
            if parts.is_empty() {
                parts.push("unchanged".into());
            }
            format!("set marker {} {}", a.marker, parts.join(", "))
        }
        WireCommand::SetCanvas(a) => {
            let mut parts = Vec::new();
            if let Some(aspect) = a.aspect {
                parts.push(format!("aspect {}", aspect.name()));
            }
            if let Some([r, g, b]) = a.background {
                parts.push(format!("background rgb({r}, {g}, {b})"));
            }
            match a.resolution {
                Some([0, 0]) => parts.push("size derived".into()),
                Some([w, h]) => parts.push(format!("size {w}×{h}")),
                None => {}
            }
            if parts.is_empty() {
                parts.push("unchanged".into());
            }
            format!("set canvas {}", parts.join(", "))
        }
        // The shell replaces this line with what it actually started; this is
        // what a dry run shows, so it says a render begins rather than ends.
        WireCommand::RenderAndReplace(a) => format!(
            "start rendering clip {} and decode that file in its place",
            a.clip
        ),
        WireCommand::RestoreUnrendered(a) => {
            format!("put clip {} back as it was before it was rendered", a.clip)
        }
    };
    // Exhaustive on purpose, with no `_` arm: the ids the assistant reports back
    // are the ones a follow-up turn can address, and an outcome that creates
    // something the operator cannot then name is a silent hole. A new
    // `EditOutcome` variant stops this file compiling, which is where the
    // decision belongs.
    match outcome {
        Some(EditOutcome::Created(id)) => line.push_str(&format!(" (new clip {})", id.raw())),
        Some(EditOutcome::CreatedTrack(id)) => line.push_str(&format!(" (track {})", id.raw())),
        Some(EditOutcome::CreatedMarker(id)) => line.push_str(&format!(" (marker {})", id.raw())),
        Some(EditOutcome::CreatedBin(id)) => line.push_str(&format!(" (folder {})", id.raw())),
        // Nothing was created, so there is no new id to name: the description of
        // the command already says what changed.
        Some(
            EditOutcome::Updated(_)
            | EditOutcome::Removed(_)
            | EditOutcome::RemovedTrack(_)
            | EditOutcome::ShiftedTrack(_)
            | EditOutcome::UpdatedTrack(_)
            | EditOutcome::UpdatedMarker(_)
            | EditOutcome::RemovedMarker(_)
            | EditOutcome::UpdatedCanvas
            | EditOutcome::UpdatedProject
            | EditOutcome::UpdatedPool
            | EditOutcome::UpdatedSequenceRate
            | EditOutcome::UpdatedAudioLayout
            | EditOutcome::UpdatedMaster
            // Navigation, not creation: the sequence already had a name before
            // it was opened, so there is no new id to hand back.
            | EditOutcome::OpenedSequence,
        )
        | None => {}
    }
    line
}
