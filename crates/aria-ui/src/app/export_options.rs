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
    fn ratio(self) -> (u32, u32) {
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

    fn resolve(self, project: Rational) -> Rational {
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

    pub fn label(self) -> String {
        match self {
            Self::Codec => "Codec default".to_string(),
            Self::Frames(frames) => format!("every {frames}"),
        }
    }

    fn frames(self) -> Option<u32> {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        }
    }
}

/// Step to the next entry of `all` after `current`, wrapping.
fn next_of<T: PartialEq + Copy>(all: &[T], current: T) -> T {
    let at = all.iter().position(|item| *item == current).unwrap_or(0);
    all[(at + 1) % all.len()]
}

impl AppState {
    pub fn cycle_export_scale(&mut self) {
        self.export_options.scale = next_of(&ExportScale::ALL, self.export_options.scale);
    }

    pub fn cycle_export_rate(&mut self) {
        self.export_options.rate = next_of(&ExportRate::ALL, self.export_options.rate);
    }

    pub fn cycle_export_gop(&mut self) {
        self.export_options.gop = next_of(&ExportGop::ALL, self.export_options.gop);
    }

    pub fn toggle_export_audio(&mut self) {
        self.export_options.audio = !self.export_options.audio;
    }

    /// Step to the next codec this build's encoder can write.
    ///
    /// The list comes from the encoder, so a machine missing an H.265 plugin
    /// never offers one; with a single codec available the control says so
    /// rather than appearing to do nothing.
    pub fn cycle_export_codec(&mut self) {
        let codecs = aria_encoder::available_codecs();
        match codecs.len() {
            0 => {
                self.report_error("This build has no encoder, so it cannot export.");
            }
            1 => {
                self.report_error(format!(
                    "{} is the only codec {} can write here.",
                    codecs[0].label(),
                    aria_encoder::capability().backend
                ));
            }
            _ => self.export_options.codec = next_of(&codecs, self.export_options.codec),
        }
    }

    /// Step to the next bitrate, where the encoder honours one.
    pub fn cycle_export_bitrate(&mut self) {
        if !aria_encoder::capability().bitrate {
            self.report_error(format!(
                "{} sets its own bitrate.",
                aria_encoder::capability().backend
            ));
            return;
        }
        self.export_options.bitrate = next_of(&ExportBitrate::ALL, self.export_options.bitrate);
    }

    /// Ask for the marked range. Refused with a sentence when nothing is marked,
    /// because an export of an empty span writes an empty file.
    pub fn toggle_export_range(&mut self) {
        if !self.export_options.marked_range && self.marked_range().is_none() {
            self.report_error("No range is marked. Set In and Out first, with I and O.");
            return;
        }
        self.export_options.marked_range = !self.export_options.marked_range;
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
            audio: Some(self.export_options.audio),
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
        let engine = self.engine().lock();
        ExportSettings::resolved(engine.project(), format)
            .unwrap_or_else(|_| ExportSettings::for_project(engine.project()))
    }

    /// How the sheet and the status line describe the span being rendered.
    pub fn export_range_label(&self) -> String {
        match self
            .export_options
            .marked_range
            .then(|| self.marked_range())
            .flatten()
        {
            None => "Whole timeline".to_string(),
            Some((from, to)) => {
                let rate = self.store.sequence.frame_rate;
                format!(
                    "{} to {}",
                    crate::timecode::display_ticks(from, rate.num, rate.den),
                    crate::timecode::display_ticks(to, rate.num, rate.den)
                )
            }
        }
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
