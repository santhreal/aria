//! What the export sheet asks for, and how it becomes [`ExportSettings`].
//!
//! The sheet used to state the format and the canvas size and offer nothing to
//! change, so a half-resolution proof copy, a video-only file, a shorter
//! keyframe interval, or a render of the marked range all meant editing the
//! project instead. These are the export's own options: they are not part of the
//! project and are not saved with it.
//!
//! One owner, because the sheet, the job that renders, and the status line that
//! reports what was rendered must agree on the same choices.

use aria_core::{ExportFormat, Rational, VideoCodec};
use aria_render::ExportSettings;

use super::AppState;

/// Output size, as a fraction of the project canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportScale {
    Full,
    Threequarter,
    Half,
    Quarter,
}

impl ExportScale {
    pub const ALL: [Self; 4] = [Self::Full, Self::Threequarter, Self::Half, Self::Quarter];

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "100%",
            Self::Threequarter => "75%",
            Self::Half => "50%",
            Self::Quarter => "25%",
        }
    }

    /// The numerator and denominator of the scale, exact so a 1080p half is
    /// 960x540 rather than a float's idea of it.
    pub(crate) fn ratio(self) -> (u32, u32) {
        match self {
            Self::Full => (1, 1),
            Self::Threequarter => (3, 4),
            Self::Half => (1, 2),
            Self::Quarter => (1, 4),
        }
    }

    fn apply(self, size: (u32, u32)) -> (u32, u32) {
        let (num, den) = self.ratio();
        let scale = |v: u32| ((v * num) / den).max(2);
        (scale(size.0), scale(size.1))
    }
}

/// Output frame rate: the project's, or a fixed delivery rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportRate {
    Project,
    Fps24,
    Fps25,
    Fps30,
    Fps50,
    Fps60,
}

impl ExportRate {
    pub const ALL: [Self; 6] = [
        Self::Project,
        Self::Fps24,
        Self::Fps25,
        Self::Fps30,
        Self::Fps50,
        Self::Fps60,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::Fps24 => "24",
            Self::Fps25 => "25",
            Self::Fps30 => "30",
            Self::Fps50 => "50",
            Self::Fps60 => "60",
        }
    }

    pub(crate) fn resolve(self, project: Rational) -> Rational {
        match self {
            Self::Project => project,
            Self::Fps24 => Rational::new(24, 1),
            Self::Fps25 => Rational::new(25, 1),
            Self::Fps30 => Rational::new(30, 1),
            Self::Fps50 => Rational::new(50, 1),
            Self::Fps60 => Rational::new(60, 1),
        }
    }
}

/// Keyframe spacing, in frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportGop {
    Codec,
    Frames(u32),
}

impl ExportGop {
    pub const ALL: [Self; 4] = [
        Self::Codec,
        Self::Frames(15),
        Self::Frames(30),
        Self::Frames(60),
    ];

    /// The interval as the Keyframes row's chip reads it, beside the row's own
    /// label: `Every 30`.
    pub fn label(self) -> String {
        match self {
            Self::Codec => "Codec default".to_string(),
            Self::Frames(frames) => format!("Every {frames}"),
        }
    }

    pub(crate) fn frames(self) -> Option<u32> {
        match self {
            Self::Codec => None,
            Self::Frames(frames) => Some(frames),
        }
    }
}

/// Target video bitrate, in kilobits per second.
///
/// A short ladder rather than a typed number: the useful choices for a delivery
/// file are an order of magnitude apart, and a field would invite a value the
/// encoder silently clamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportBitrate {
    /// Leave the encoder's own quality-driven rate control alone.
    Encoder,
    Kbps(u32),
}

impl ExportBitrate {
    pub const ALL: [Self; 5] = [
        Self::Encoder,
        Self::Kbps(4_000),
        Self::Kbps(8_000),
        Self::Kbps(16_000),
        Self::Kbps(40_000),
    ];

    pub fn label(self) -> String {
        match self {
            Self::Encoder => "Encoder default".to_string(),
            Self::Kbps(kbps) if kbps % 1_000 == 0 => format!("{} Mb/s", kbps / 1_000),
            Self::Kbps(kbps) => format!("{kbps} kb/s"),
        }
    }

    pub(crate) fn kbps(self) -> Option<u32> {
        match self {
            Self::Encoder => None,
            Self::Kbps(kbps) => Some(kbps),
        }
    }
}

/// Every choice the export sheet offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOptions {
    pub scale: ExportScale,
    pub rate: ExportRate,
    pub gop: ExportGop,
    /// Whether the file carries audio. Off writes video only.
    pub audio: bool,
    /// Whether to render only the marked range. Ignored when no range is marked.
    pub marked_range: bool,
    /// Video codec to write. Only a codec this build's encoder offers can be
    /// selected.
    pub codec: VideoCodec,
    /// Target video bitrate. Only selectable where the encoder honours one.
    pub bitrate: ExportBitrate,
    /// Registered plugin encoder id, or `None` for the platform encoder.
    pub plugin_encoder: Option<String>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            scale: ExportScale::Full,
            rate: ExportRate::Project,
            gop: ExportGop::Codec,
            audio: true,
            marked_range: false,
            codec: VideoCodec::default(),
            bitrate: ExportBitrate::Encoder,
            plugin_encoder: None,
        }
    }
}

/// The Range row's whole-timeline choice, and how the sheet and the status line
/// name an export of the whole timeline.
pub const WHOLE_TIMELINE: &str = "Whole timeline";

/// The Audio row's choices: whether the file carries the sound, and how each
/// reads.
pub const AUDIO_CHOICES: [(bool, &str); 2] = [(true, "Included"), (false, "Video only")];

impl AppState {
    pub fn set_export_scale(&mut self, scale: ExportScale) {
        self.export_options.scale = scale;
    }

    pub fn set_export_rate(&mut self, rate: ExportRate) {
        self.export_options.rate = rate;
    }

    pub fn set_export_gop(&mut self, gop: ExportGop) {
        self.export_options.gop = gop;
    }

    /// Carry the sound, or write video only.
    pub fn set_export_audio(&mut self, audio: bool) {
        self.export_options.audio = audio;
    }

    /// Select a registered plugin encoder by id, or `None` for the platform
    /// encoder.
    pub fn select_export_plugin_encoder(&mut self, encoder_id: Option<String>) {
        self.export_options.plugin_encoder = encoder_id;
    }

    /// The Format row's choices and their labels, in order: the platform
    /// encoder (`None`), each mounted plugin encoder in registration order, and
    /// the chosen plugin encoder when it is no longer mounted, so the choice in
    /// force is always one of them.
    pub fn export_format_choices(&self) -> Vec<(Option<String>, String)> {
        let mut ids: Vec<Option<String>> = std::iter::once(None)
            .chain(self.ctx.encoders().iter().map(|spec| Some(spec.id.clone())))
            .collect();
        if !ids.contains(&self.export_options.plugin_encoder) {
            ids.push(self.export_options.plugin_encoder.clone());
        }
        ids.into_iter()
            .map(|id| {
                let label = self.format_choice_label(id.as_deref());
                (id, label)
            })
            .collect()
    }

    /// The active plugin encoder spec when `plugin_encoder` names a mounted
    /// encoder. Reads `self.ctx` directly without locking `self.engine`.
    pub fn active_plugin_encoder(&self) -> Option<std::sync::Arc<aria_plugin::EncoderSpec>> {
        self.export_options
            .plugin_encoder
            .as_deref()
            .and_then(|id| self.ctx.encoder(id))
    }

    /// File extension (without leading dot) for the active export format.
    pub fn export_extension(&self) -> String {
        self.active_plugin_encoder()
            .map_or_else(|| "mp4".to_string(), |spec| spec.extension.clone())
    }

    /// Label for the Format row in the Export dialog.
    pub fn export_format_label(&self) -> String {
        self.format_choice_label(self.export_options.plugin_encoder.as_deref())
    }

    /// How the Format row names `choice`: a plugin encoder by its name and
    /// extension, or by its id once unmounted, and the platform encoder by what
    /// it writes with the chosen codec.
    fn format_choice_label(&self, choice: Option<&str>) -> String {
        let Some(id) = choice else {
            return format_label(self.export_options.codec);
        };
        match self.ctx.encoder(id) {
            Some(spec) => format!("{} (.{})", spec.name, spec.extension),
            None => format!("{id} (unmounted)"),
        }
    }

    /// Label for the Encoder row in the Export dialog.
    pub fn export_encoder_label(&self) -> String {
        if let Some(spec) = self.active_plugin_encoder() {
            return spec.id.clone();
        }
        if let Some(id) = self.export_options.plugin_encoder.as_deref() {
            return id.to_string();
        }
        aria_encoder::capability().backend.to_string()
    }

    /// Whether the Export dialog should show the Codec and Bitrate rows.
    /// Hidden when a plugin encoder is selected.
    pub fn export_shows_codec_and_bitrate(&self) -> bool {
        self.export_options.plugin_encoder.is_none()
    }

    /// Whether the Export dialog should show the Audio toggle row.
    /// Hidden when a selected plugin encoder does not support audio.
    pub fn export_shows_audio_toggle(&self) -> bool {
        self.active_plugin_encoder().is_none_or(|spec| spec.audio)
    }

    /// Write `codec`. The sheet offers what [`aria_encoder::probed_codecs`]
    /// found; a codec this machine cannot write is refused by name when the
    /// export resolves, in [`ExportSettings::resolved`].
    pub fn set_export_codec(&mut self, codec: VideoCodec) {
        self.export_options.codec = codec;
    }

    /// Ask for `bitrate`. The sheet offers the ladder only where the encoder
    /// honours one; elsewhere [`ExportSettings::resolved`] refuses it by name.
    pub fn set_export_bitrate(&mut self, bitrate: ExportBitrate) {
        self.export_options.bitrate = bitrate;
    }

    /// Render only the marked range, or the whole timeline. The marked range is
    /// refused with a sentence when nothing is marked, because an export of an
    /// empty span writes an empty file.
    pub fn set_export_marked_range(&mut self, marked: bool) {
        if marked && self.marked_range().is_none() {
            self.report_error("No range is marked. Set In and Out first, with I and O.");
            return;
        }
        self.export_options.marked_range = marked;
    }

    /// Find which codecs this machine can write, unless that is known or
    /// already asked for.
    ///
    /// The probe opens each codec's encoder, up to a third of a second for a
    /// hardware one, so it runs off the thread that draws; the Codec row states
    /// the chosen codec until it answers. A waiter thread wakes the preview
    /// worker's mailbox once it does, and [`Self::land_codec_probe`] redraws
    /// the window.
    pub(crate) fn probe_export_codecs(&mut self) {
        if self.codec_probe_pending || aria_encoder::probed_codecs().is_some() {
            return;
        }
        aria_encoder::probe_codecs();
        self.codec_probe_pending = true;
        let wake = self.preview_worker.clone_wake();
        let waiter = std::thread::Builder::new()
            .name("aria-codec-probe-wait".to_owned())
            .spawn(move || {
                // Returns once the probe answers.
                aria_encoder::available_codecs();
                wake.wake(());
            });
        if let Err(err) = waiter {
            tracing::warn!(%err, "the codec probe waiter did not start; the Codec row offers its choices on the next redraw");
        }
    }

    /// Whether the codec probe the sheet waits on has answered since the last
    /// check, so the window redraws once and the Codec row offers its choices.
    pub(crate) fn land_codec_probe(&mut self) -> bool {
        let landed = self.codec_probe_pending && aria_encoder::probed_codecs().is_some();
        if landed {
            self.codec_probe_pending = false;
        }
        landed
    }

    /// The marked span in ticks, `[in, out)`, or nothing when it is unset or
    /// empty.
    pub fn marked_range(&self) -> Option<(i64, i64)> {
        let from = self.mark_in?;
        let to = self.mark_out?;
        if to > from { Some((from, to)) } else { None }
    }

    /// What the sheet is asking the export for.
    ///
    /// The sheet's ladders resolve to overrides here and nowhere else, so the
    /// job that renders, the blocking export, and the status line all describe
    /// the same request. Resolving it against the project — and refusing a
    /// codec or bitrate this machine cannot honour — belongs to
    /// [`ExportSettings::resolved`].
    pub fn export_format(&self) -> ExportFormat {
        let project_rate = self.store.sequence.frame_rate;
        // The canvas size is projected beside the store, so the sheet describes
        // the same output the settings dialog does without asking the session.
        let native = self.canvas_size_tuple();
        ExportFormat {
            size: Some(self.export_options.scale.apply(native)),
            frame_rate: Some(self.export_options.rate.resolve(project_rate)),
            keyframe_interval: self.export_options.gop.frames(),
            audio: Some(self.export_shows_audio_toggle() && self.export_options.audio),
            range: self
                .export_options
                .marked_range
                .then(|| self.marked_range())
                .flatten()
                .map(|(from, to)| {
                    (
                        aria_core::RationalTime::new(from, project_rate),
                        aria_core::RationalTime::new(to, project_rate),
                    )
                }),
            video_codec: Some(self.export_options.codec),
            bitrate_kbps: self.export_options.bitrate.kbps(),
        }
    }

    /// What the sheet's rows describe: the request resolved against the project.
    ///
    /// Infallible on purpose — this feeds the size row and the status line, and
    /// a picker that shows nothing while it explains a refusal is worse than one
    /// showing the project's own output. The refusal itself belongs to whoever
    /// starts the export, which reports it by name.
    pub fn export_settings(&self) -> ExportSettings {
        let format = self.export_format();
        let plugin_audio = self.active_plugin_encoder().map(|spec| spec.audio);
        let engine = self.engine().lock();
        let resolved = match plugin_audio {
            Some(supports_audio) => {
                ExportSettings::resolved_for_encoder(engine.project(), format, supports_audio)
            }
            None => ExportSettings::resolved(engine.project(), format),
        };
        resolved.unwrap_or_else(|_| ExportSettings::for_project(engine.project()))
    }

    /// How the sheet and the status line describe the span being rendered.
    pub fn export_range_label(&self) -> String {
        if self.export_options.marked_range && self.marked_range().is_some() {
            self.marked_range_label()
        } else {
            WHOLE_TIMELINE.to_string()
        }
    }

    /// The Range row's marked choice: both ends of the marked span, or
    /// `Marked range` while nothing is marked.
    pub fn marked_range_label(&self) -> String {
        let Some((from, to)) = self.marked_range() else {
            return "Marked range".to_string();
        };
        let rate = self.store.sequence.frame_rate;
        format!(
            "{} to {}",
            crate::timecode::display_ticks(from, rate.num, rate.den),
            crate::timecode::display_ticks(to, rate.num, rate.den)
        )
    }

    /// The output size the sheet shows, after the scale.
    ///
    /// Evened here rather than resolved: the row has to name the size the file
    /// will actually have, and [`aria_render::even_export_size`] is the one
    /// owner of that rule. Resolving would need the project, and the session
    /// lock with it, to answer a question the projection already carries — and
    /// this row is drawn every frame the sheet is open.
    pub fn export_size(&self) -> (u32, u32) {
        aria_render::even_export_size(
            self.export_format()
                .size
                .unwrap_or_else(|| self.canvas_size_tuple()),
        )
    }
}

/// What this export will write, read from the encoder rather than compiled in:
/// on a build with no backend the sheet must say so instead of promising an MP4.
///
/// Takes the chosen codec, so the Format row and the Codec row cannot disagree
/// about the file being written.
pub fn format_label(codec: VideoCodec) -> String {
    aria_encoder::capability().label_for(codec)
}
