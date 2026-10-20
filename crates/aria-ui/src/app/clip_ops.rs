//! Clip operations: enable/disable, clip groups, time remap, Speed/Duration,
//! and Audio Gain.

use std::path::Path;
use std::sync::Arc;

use aria_analysis::{db_from_linear, linear_from_db};
use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    ClipId, ClipParam, Easing, Keyframe, MAX_CLIP_VOLUME, MAX_SPEED, MIN_SPEED, Param, ParamValue,
    Rational,
};
use gpui::SharedString;

use super::clip_audio::VOLUME_FLOOR_DB;
use super::clip_speed::RAMP_POINTS;
use super::{AppState, DialogKind, describe_engine_error};
use crate::types::ActivePanel;
use crate::waveforms::{Waveform, bars};

/// Minimum allowed speed percentage in the Speed/Duration dialog (`MIN_SPEED * 100`).
pub const MIN_SPEED_PERCENT: f32 = MIN_SPEED * 100.0;

/// Maximum allowed speed percentage in the Speed/Duration dialog (`MAX_SPEED * 100`).
pub const MAX_SPEED_PERCENT: f32 = MAX_SPEED * 100.0;

/// Maximum allowed gain in dB (`20 * log10(MAX_CLIP_VOLUME)` rounded to 12 dB).
pub const MAX_GAIN_DB: f32 = 12.0;

/// State of the Speed/Duration dialog (`Ctrl+R`).
#[derive(Debug, Clone, PartialEq)]
pub struct SpeedDurationForm {
    pub speed_percent: f32,
    pub duration_ticks: i64,
    pub base_ticks: i64,
    pub reverse: bool,
    pub preserve_pitch: bool,
    pub duration_label: SharedString,
    pub error: Option<SharedString>,
}

impl Default for SpeedDurationForm {
    fn default() -> Self {
        Self {
            speed_percent: 100.0,
            duration_ticks: 24,
            base_ticks: 24,
            reverse: false,
            preserve_pitch: false,
            duration_label: SharedString::new_static("00:00:01:00"),
            error: None,
        }
    }
}

/// Which operation the Audio Gain dialog (`G`) applies on commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioGainMode {
    #[default]
    SetGain,
    AdjustGain,
    NormalizeMaxPeak,
    NormalizeAllPeaks,
}

impl AudioGainMode {
    pub const ALL: [Self; 4] = [
        Self::SetGain,
        Self::AdjustGain,
        Self::NormalizeMaxPeak,
        Self::NormalizeAllPeaks,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::SetGain => "Set Gain to",
            Self::AdjustGain => "Adjust Gain by",
            Self::NormalizeMaxPeak => "Normalize Max Peak to",
            Self::NormalizeAllPeaks => "Normalize All Peaks to",
        }
    }
}

/// State of the Audio Gain dialog (`G`).
#[derive(Debug, Clone, PartialEq)]
pub struct AudioGainForm {
    pub mode: AudioGainMode,
    pub set_db: f32,
    pub adjust_db: f32,
    pub target_db: f32,
    pub max_peak_dbfs: Option<f32>,
    pub clip_count: usize,
    pub error: Option<SharedString>,
}

impl Default for AudioGainForm {
    fn default() -> Self {
        Self {
            mode: AudioGainMode::SetGain,
            set_db: 0.0,
            adjust_db: 0.0,
            target_db: -1.0,
            max_peak_dbfs: None,
            clip_count: 0,
            error: None,
        }
    }
}

fn gcd(mut a: i32, mut b: i32) -> i32 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

fn speed_percent_to_rational(percent: f32) -> Rational {
    let scaled = (percent * 10.0).round() as i32;
    let num = scaled.clamp(50, 100_000);
    let den = 1000;
    let g = gcd(num, den);
    Rational::new(num / g, den / g)
}

fn linear_for_db(db: f32) -> f32 {
    if db <= VOLUME_FLOOR_DB {
        0.0
    } else {
        linear_from_db(db).min(MAX_CLIP_VOLUME)
    }
}

impl AppState {
    /// Toggle `enabled` on the selected clips as one undo step (`Shift+E`).
    ///
    /// If any selected clip is disabled, enables the whole selection; otherwise
    /// disables the whole selection.
    pub fn toggle_enabled_selected(&mut self) -> Result<(), EngineError> {
        if self.selection.is_empty() {
            self.status_message = Some("Select a clip to enable or disable".into());
            return Ok(());
        }
        let any_disabled = self
            .selection
            .iter()
            .any(|id| self.ui_clip_ref(*id).is_some_and(|c| !c.enabled));
        let enabled = any_disabled;
        let clips = self.selection.to_vec();
        self.apply_command(Command::Edit(EditCommand::SetClipsEnabled {
            clips,
            enabled,
        }))?;
        self.status_message = Some(if enabled {
            "Enabled".into()
        } else {
            "Disabled".into()
        });
        Ok(())
    }

    /// Whether the current selection forms one clip group.
    pub fn selection_is_grouped(&self) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        let mut group: Option<u64> = None;
        for id in &self.selection {
            let Some(g) = self.ui_clip_ref(*id).and_then(|c| c.grouped_to) else {
                return false;
            };
            match group {
                None => group = Some(g),
                Some(seen) if seen == g => {}
                Some(_) => return false,
            }
        }
        true
    }

    /// Group the selected clips (2+ clips) as one undo step (`Ctrl+G`).
    pub fn group_selected(&mut self) -> Result<(), EngineError> {
        if self.selection.len() < 2 {
            self.status_message = Some("Select two or more clips to group".into());
            return Ok(());
        }
        let clips = self.selection.to_vec();
        self.apply_command(Command::Edit(EditCommand::GroupClips { clips }))?;
        self.status_message = Some("Grouped".into());
        Ok(())
    }

    /// Dissolve every clip group touched by the selection as one undo step (`Ctrl+Shift+G`).
    pub fn ungroup_selected(&mut self) -> Result<(), EngineError> {
        if self.selection.is_empty() {
            self.status_message = Some("Select a clip to ungroup".into());
            return Ok(());
        }
        let clips = self.selection.to_vec();
        self.apply_command(Command::Edit(EditCommand::UngroupClips { clips }))?;
        self.status_message = Some("Ungrouped".into());
        Ok(())
    }

    /// Toggle Time Remap (`Ctrl+Alt+T`) on the selected media clip.
    ///
    /// Maps sequence time to source time through the clip's keyframed speed
    /// curve (`Clip::speed_curve` / `EditCommand::SetSpeedCurve`) and focuses
    /// the Inspector's Speed ramp editor when enabled.
    pub fn toggle_time_remap(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            self.status_message = Some("Select a clip to toggle time remap".into());
            return Ok(());
        };
        if !self.selected_can_ramp() {
            self.status_message = Some("Time remap requires a media clip".into());
            return Ok(());
        }
        if self.selected_has_ramp() {
            self.clear_selected_speed_curve()?;
            self.status_message = Some("Disabled time remap".into());
            return Ok(());
        }
        let curve = Param::Keyframed {
            keyframes: RAMP_POINTS
                .iter()
                .map(|(_, _, point)| Keyframe::new(*point, 1.0, Easing::Linear))
                .collect(),
        };
        self.apply_command(Command::Edit(EditCommand::SetSpeedCurve {
            clip,
            curve: Some(curve),
        }))?;
        self.active_panel = ActivePanel::Inspector;
        self.inspector_tab = "speed".to_string();
        self.status_message = Some("Enabled time remap".into());
        Ok(())
    }

    /// Open the Speed/Duration dialog (`Ctrl+R`) seeded from the selected clip.
    pub fn open_speed_duration(&mut self) {
        let Some(ui) = self.selected_ui_clip() else {
            self.status_message = Some("Select a clip to change speed or duration".into());
            return;
        };
        let rate = self.store.sequence.frame_rate;
        let speed_percent =
            ((ui.speed_num.max(1) as f64 / ui.speed_den.max(1) as f64) * 100.0) as f32;
        let duration_ticks = (ui.timeline_end_ticks - ui.timeline_start_ticks).max(1);
        let base_ticks = ((duration_ticks as f64) * (ui.speed_num.max(1) as f64)
            / (ui.speed_den.max(1) as f64))
            .round()
            .max(1.0) as i64;
        let duration_label =
            crate::timecode::format_ticks(duration_ticks, rate.num, rate.den).into();
        self.speed_duration = SpeedDurationForm {
            speed_percent,
            duration_ticks,
            base_ticks,
            reverse: ui.reversed,
            preserve_pitch: ui.preserve_pitch,
            duration_label,
            error: None,
        };
        self.open_dialog(DialogKind::SpeedDuration);
    }

    /// Set the Speed/Duration dialog's constant speed percentage and update the
    /// resulting duration. Refuses out-of-range or non-finite values with a
    /// stated reason.
    pub fn set_speed_duration_percent(&mut self, percent: f32) -> Result<(), String> {
        if !percent.is_finite() || !(MIN_SPEED_PERCENT..=MAX_SPEED_PERCENT).contains(&percent) {
            let reason = format!(
                "Speed must be between {MIN_SPEED_PERCENT:.0}% and {MAX_SPEED_PERCENT:.0}%"
            );
            self.speed_duration.error = Some(reason.clone().into());
            self.status_message = Some(reason.clone());
            return Err(reason);
        }
        let duration_ticks = ((self.speed_duration.base_ticks as f64) * 100.0 / (percent as f64))
            .round()
            .max(1.0) as i64;
        let rate = self.store.sequence.frame_rate;
        self.speed_duration.speed_percent = percent;
        self.speed_duration.duration_ticks = duration_ticks;
        self.speed_duration.duration_label =
            crate::timecode::format_ticks(duration_ticks, rate.num, rate.den).into();
        self.speed_duration.error = None;
        Ok(())
    }

    /// Set the Speed/Duration dialog's target duration in timeline ticks and
    /// derive the corresponding speed percentage. Refuses invalid durations or
    /// speeds outside the supported range with a stated reason.
    pub fn set_speed_duration_ticks(&mut self, ticks: i64) -> Result<(), String> {
        if ticks < 1 {
            let reason = "Duration must be at least 1 frame".to_string();
            self.speed_duration.error = Some(reason.clone().into());
            self.status_message = Some(reason.clone());
            return Err(reason);
        }
        let percent = ((self.speed_duration.base_ticks as f64) * 100.0 / (ticks as f64)) as f32;
        if !percent.is_finite() || !(MIN_SPEED_PERCENT..=MAX_SPEED_PERCENT).contains(&percent) {
            let reason = format!(
                "Duration requires a speed outside {MIN_SPEED_PERCENT:.0}% to {MAX_SPEED_PERCENT:.0}%"
            );
            self.speed_duration.error = Some(reason.clone().into());
            self.status_message = Some(reason.clone());
            return Err(reason);
        }
        let rate = self.store.sequence.frame_rate;
        self.speed_duration.duration_ticks = ticks;
        self.speed_duration.speed_percent = percent;
        self.speed_duration.duration_label =
            crate::timecode::format_ticks(ticks, rate.num, rate.den).into();
        self.speed_duration.error = None;
        Ok(())
    }

    /// Set whether the Speed/Duration dialog reverses playback.
    pub fn set_speed_duration_reverse(&mut self, reverse: bool) {
        self.speed_duration.reverse = reverse;
        self.speed_duration.error = None;
    }

    /// Toggle the Speed/Duration dialog's reverse flag.
    pub fn toggle_speed_duration_reverse(&mut self) {
        self.speed_duration.reverse = !self.speed_duration.reverse;
        self.speed_duration.error = None;
    }

    /// Set whether the Speed/Duration dialog preserves audio pitch.
    pub fn set_speed_duration_preserve_pitch(&mut self, preserve_pitch: bool) {
        self.speed_duration.preserve_pitch = preserve_pitch;
        self.speed_duration.error = None;
    }

    /// Toggle the Speed/Duration dialog's pitch preservation flag.
    pub fn toggle_speed_duration_preserve_pitch(&mut self) {
        self.speed_duration.preserve_pitch = !self.speed_duration.preserve_pitch;
        self.speed_duration.error = None;
    }

    /// Commit the Speed/Duration dialog across all selected clips as one undo step.
    pub fn apply_speed_duration(&mut self) {
        if let Some(err) = &self.speed_duration.error {
            self.status_message = Some(err.to_string());
            return;
        }
        let percent = self.speed_duration.speed_percent;
        if !percent.is_finite() || !(MIN_SPEED_PERCENT..=MAX_SPEED_PERCENT).contains(&percent) {
            let reason = format!(
                "Speed must be between {MIN_SPEED_PERCENT:.0}% and {MAX_SPEED_PERCENT:.0}%"
            );
            self.speed_duration.error = Some(reason.clone().into());
            self.status_message = Some(reason);
            return;
        }
        if self.speed_duration.duration_ticks < 1 {
            let reason = "Duration must be at least 1 frame".to_string();
            self.speed_duration.error = Some(reason.clone().into());
            self.status_message = Some(reason);
            return;
        }
        if self.selection.is_empty() {
            self.open_dialog(DialogKind::None);
            return;
        }
        let speed = speed_percent_to_rational(percent);
        let reversed = self.speed_duration.reverse;
        let preserve_pitch = self.speed_duration.preserve_pitch;
        let selected = self.selection.to_vec();
        let mut seen_links: Vec<u64> = Vec::new();
        let mut speed_targets: Vec<(ClipId, bool)> = Vec::new();
        for &clip in &selected {
            let has_ramp = self.ui_clip_ref(clip).is_some_and(|ui| ui.time_remap);
            let link = self.ui_clip_ref(clip).and_then(|ui| ui.linked_to);
            if let Some(l) = link {
                if seen_links.contains(&l) {
                    continue;
                }
                seen_links.push(l);
            }
            speed_targets.push((clip, has_ramp));
        }
        let res = self.one_step(|this| {
            for (clip, has_ramp) in speed_targets {
                if has_ramp {
                    this.apply_command(Command::Edit(EditCommand::SetSpeedCurve {
                        clip,
                        curve: None,
                    }))?;
                }
                this.apply_command(Command::Edit(EditCommand::SetClipSpeed {
                    clip,
                    speed,
                    reversed,
                }))?;
            }
            for clip in selected {
                this.apply_command(Command::Edit(EditCommand::SetClipPitch {
                    clip,
                    preserve_pitch,
                }))?;
            }
            Ok(())
        });
        match res {
            Ok(()) => {
                let summary = format!(
                    "Speed {:.0}% ({})",
                    self.speed_duration.speed_percent, self.speed_duration.duration_label
                );
                self.open_dialog(DialogKind::None);
                self.status_message = Some(summary);
            }
            Err(err) => {
                let msg = describe_engine_error(&err);
                self.speed_duration.error = Some(msg.into());
                self.report_error(&err);
            }
        }
    }

    /// Every audio-bearing clip in the current selection (including linked or
    /// grouped companions on unlocked tracks).
    pub fn selected_audio_clips(&self) -> Vec<ClipId> {
        let mut out: Vec<ClipId> = Vec::new();
        let mut links: Vec<u64> = Vec::new();
        let mut groups: Vec<u64> = Vec::new();
        for &id in &self.selection {
            if let Some(ui) = self.ui_clip_ref(id) {
                if ui.can_hold_inserts && !out.contains(&id) {
                    out.push(id);
                }
                if let Some(l) = ui.linked_to
                    && !links.contains(&l)
                {
                    links.push(l);
                }
                if let Some(g) = ui.grouped_to
                    && !groups.contains(&g)
                {
                    groups.push(g);
                }
            }
        }
        if !links.is_empty() || !groups.is_empty() {
            for lane in &self.store.sequence.tracks {
                if lane.locked {
                    continue;
                }
                for clip in &lane.clips {
                    if clip.can_hold_inserts
                        && (clip.linked_to.is_some_and(|l| links.contains(&l))
                            || clip.grouped_to.is_some_and(|g| groups.contains(&g)))
                    {
                        let id = ClipId::from_raw(clip.id);
                        if !out.contains(&id) {
                            out.push(id);
                        }
                    }
                }
            }
        }
        out
    }

    /// Measure the real decoded peak amplitude (in dBFS) of `clip_id` across its
    /// source window `[source_start, source_end)`.
    pub fn clip_audio_peak_dbfs(&mut self, clip_id: ClipId) -> Option<f32> {
        let ui = self.ui_clip(clip_id)?;
        if !ui.can_hold_inserts {
            return None;
        }
        let media_id = ui.media_id?;
        let path = self
            .store
            .media
            .iter()
            .find(|item| item.id == media_id && item.has_audio)
            .map(|item| item.path.clone())?;
        if self.waveforms.get(media_id).is_none()
            && !self.waveforms.refused(media_id)
            && let Ok(decoded) =
                aria_decoder::audio_peaks_per_second(Path::new(&*path), 40.0, 400_000)
            && !decoded.peaks.is_empty()
        {
            self.waveforms.insert(
                media_id,
                Waveform {
                    peaks: Arc::new(decoded.peaks),
                    per_second: decoded.per_second,
                },
            );
        }
        let waveform = self.waveforms.get(media_id)?;
        let peak = if ui.source_end > ui.source_start {
            bars(waveform, ui.source_start, ui.source_end, 1)
                .first()
                .copied()
                .unwrap_or(0.0)
        } else {
            waveform.peaks.iter().copied().fold(0.0f32, f32::max)
        };
        db_from_linear(peak)
    }

    /// Open the Audio Gain dialog (`G`) for the selected audio-bearing clips.
    pub fn open_audio_gain(&mut self) {
        let clips = self.selected_audio_clips();
        if clips.is_empty() {
            self.status_message = Some(if self.selection.is_empty() {
                "Select an audio clip to adjust gain".into()
            } else {
                "Selected clip carries no audio".into()
            });
            return;
        }
        let first_vol = self.ui_clip_ref(clips[0]).map_or(1.0, |c| c.volume);
        let set_db = db_from_linear(first_vol)
            .unwrap_or(VOLUME_FLOOR_DB)
            .clamp(VOLUME_FLOOR_DB, MAX_GAIN_DB);
        let mut max_peak_dbfs: Option<f32> = None;
        for &clip in &clips {
            if let Some(peak) = self.clip_audio_peak_dbfs(clip) {
                max_peak_dbfs = Some(max_peak_dbfs.map_or(peak, |cur| cur.max(peak)));
            }
        }
        self.audio_gain = AudioGainForm {
            mode: AudioGainMode::SetGain,
            set_db,
            adjust_db: 0.0,
            target_db: -1.0,
            max_peak_dbfs,
            clip_count: clips.len(),
            error: None,
        };
        self.open_dialog(DialogKind::AudioGain);
    }

    /// Select the active mode in the Audio Gain dialog.
    pub fn set_audio_gain_mode(&mut self, mode: AudioGainMode) {
        self.audio_gain.mode = mode;
        self.audio_gain.error = None;
    }

    /// Set the target gain in dB for `AudioGainMode::SetGain` (`-60..=+12 dB`).
    pub fn set_audio_gain_set_db(&mut self, db: f32) -> Result<(), String> {
        if !db.is_finite() || !(VOLUME_FLOOR_DB..=MAX_GAIN_DB).contains(&db) {
            let reason =
                format!("Gain must be between {VOLUME_FLOOR_DB:.0} dB and +{MAX_GAIN_DB:.0} dB");
            self.audio_gain.error = Some(reason.clone().into());
            self.status_message = Some(reason.clone());
            return Err(reason);
        }
        self.audio_gain.set_db = db;
        self.audio_gain.error = None;
        Ok(())
    }

    /// Set the relative dB delta for `AudioGainMode::AdjustGain` (`-60..=+60 dB`).
    pub fn set_audio_gain_adjust_db(&mut self, db: f32) -> Result<(), String> {
        if !db.is_finite() || !(-60.0..=60.0).contains(&db) {
            let reason = "Adjustment must be between -60 dB and +60 dB".to_string();
            self.audio_gain.error = Some(reason.clone().into());
            self.status_message = Some(reason.clone());
            return Err(reason);
        }
        self.audio_gain.adjust_db = db;
        self.audio_gain.error = None;
        Ok(())
    }

    /// Set the normalization target in dBFS for `NormalizeMaxPeak` / `NormalizeAllPeaks`
    /// (`-60..=0 dBFS`).
    pub fn set_audio_gain_target_db(&mut self, db: f32) -> Result<(), String> {
        if !db.is_finite() || !(VOLUME_FLOOR_DB..=0.0).contains(&db) {
            let reason =
                format!("Peak target must be between {VOLUME_FLOOR_DB:.0} dBFS and 0 dBFS");
            self.audio_gain.error = Some(reason.clone().into());
            self.status_message = Some(reason.clone());
            return Err(reason);
        }
        self.audio_gain.target_db = db;
        self.audio_gain.error = None;
        Ok(())
    }

    /// Commit the Audio Gain dialog across all selected audio-bearing clips as
    /// one undo step.
    pub fn apply_audio_gain(&mut self) {
        if let Some(err) = &self.audio_gain.error {
            self.status_message = Some(err.to_string());
            return;
        }
        let clips = self.selected_audio_clips();
        if clips.is_empty() {
            self.open_dialog(DialogKind::None);
            return;
        }
        let mut plan: Vec<(ClipId, f32)> = Vec::with_capacity(clips.len());
        match self.audio_gain.mode {
            AudioGainMode::SetGain => {
                let db = self.audio_gain.set_db;
                if !db.is_finite() || !(VOLUME_FLOOR_DB..=MAX_GAIN_DB).contains(&db) {
                    let reason = format!(
                        "Gain must be between {VOLUME_FLOOR_DB:.0} dB and +{MAX_GAIN_DB:.0} dB"
                    );
                    self.audio_gain.error = Some(reason.clone().into());
                    self.status_message = Some(reason);
                    return;
                }
                let linear = linear_for_db(db);
                for clip in clips {
                    plan.push((clip, linear));
                }
            }
            AudioGainMode::AdjustGain => {
                let delta = self.audio_gain.adjust_db;
                if !delta.is_finite() || !(-60.0..=60.0).contains(&delta) {
                    let reason = "Adjustment must be between -60 dB and +60 dB".to_string();
                    self.audio_gain.error = Some(reason.clone().into());
                    self.status_message = Some(reason);
                    return;
                }
                for clip in clips {
                    let cur = self.ui_clip_ref(clip).map_or(1.0, |c| c.volume);
                    let cur_db = db_from_linear(cur).unwrap_or(VOLUME_FLOOR_DB);
                    let next_db = cur_db + delta;
                    plan.push((clip, linear_for_db(next_db)));
                }
            }
            AudioGainMode::NormalizeMaxPeak => {
                let target = self.audio_gain.target_db;
                if !target.is_finite() || !(VOLUME_FLOOR_DB..=0.0).contains(&target) {
                    let reason =
                        format!("Peak target must be between {VOLUME_FLOOR_DB:.0} dBFS and 0 dBFS");
                    self.audio_gain.error = Some(reason.clone().into());
                    self.status_message = Some(reason);
                    return;
                }
                let mut max_peak: Option<f32> = None;
                for &clip in &clips {
                    if let Some(p) = self.clip_audio_peak_dbfs(clip) {
                        max_peak = Some(max_peak.map_or(p, |cur| cur.max(p)));
                    }
                }
                let Some(max_peak) = max_peak else {
                    let reason = "Selected clips have no measurable audio peak".to_string();
                    self.audio_gain.error = Some(reason.clone().into());
                    self.status_message = Some(reason);
                    return;
                };
                let linear = linear_for_db(target - max_peak);
                for clip in clips {
                    plan.push((clip, linear));
                }
            }
            AudioGainMode::NormalizeAllPeaks => {
                let target = self.audio_gain.target_db;
                if !target.is_finite() || !(VOLUME_FLOOR_DB..=0.0).contains(&target) {
                    let reason =
                        format!("Peak target must be between {VOLUME_FLOOR_DB:.0} dBFS and 0 dBFS");
                    self.audio_gain.error = Some(reason.clone().into());
                    self.status_message = Some(reason);
                    return;
                }
                for clip in clips {
                    let Some(peak) = self.clip_audio_peak_dbfs(clip) else {
                        let reason = "Selected clip has no measurable audio peak".to_string();
                        self.audio_gain.error = Some(reason.clone().into());
                        self.status_message = Some(reason);
                        return;
                    };
                    let linear = linear_for_db(target - peak);
                    plan.push((clip, linear));
                }
            }
        }
        let res = self.one_step(|this| {
            for (clip, linear) in plan {
                this.apply_command(Command::Edit(EditCommand::SetParamConstant {
                    clip,
                    param: ClipParam::Volume,
                    value: ParamValue::Scalar(linear),
                }))?;
            }
            Ok(())
        });
        match res {
            Ok(()) => {
                self.open_dialog(DialogKind::None);
                self.status_message = Some("Applied audio gain".into());
            }
            Err(err) => {
                let msg = describe_engine_error(&err);
                self.audio_gain.error = Some(msg.into());
                self.report_error(&err);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::fixtures;

    fn applied_steps(state: &AppState) -> usize {
        state.history_entries().iter().filter(|e| e.applied).count()
    }

    #[test]
    fn toggle_enabled_flips_opacity_and_undoes_in_one_step() {
        let (mut state, clip_id) = fixtures::with_video();
        let ui = state.ui_clip_ref(clip_id).expect("clip projected");
        assert!(ui.enabled);

        state.toggle_enabled_selected().expect("disables clip");
        let ui = state.ui_clip_ref(clip_id).expect("clip projected");
        assert!(!ui.enabled);
        assert!(state.undo(), "undo restores enabled");
        assert!(state.ui_clip_ref(clip_id).expect("clip projected").enabled);

        assert!(state.redo(), "redo disables again");
        assert!(!state.ui_clip_ref(clip_id).expect("clip projected").enabled);
    }

    #[test]
    fn group_and_ungroup_expand_selection_and_undo_in_one_step() {
        let mut state = fixtures::linked_pair();
        state.unlink_selected().expect("unlink pair first");
        let clips = state.selection.clone();
        assert_eq!(clips.len(), 2);
        assert!(!state.selection_is_grouped());

        state.group_selected().expect("groups pair");
        assert!(state.selection_is_grouped());

        // Selecting one member selects the whole group on unlocked tracks.
        state.select_clip_linked(clips[0], true);
        assert_eq!(state.selection.len(), 2);

        // Alt-selection (with_link = false) selects only the clicked member.
        state.select_clip_linked(clips[0], false);
        assert_eq!(state.selection, vec![clips[0]]);

        // Ungrouping from one member dissolves the whole group in one undo step.
        state.ungroup_selected().expect("ungroups from member");
        state.select_clip_linked(clips[0], true);
        assert_eq!(state.selection, vec![clips[0]]);
        assert!(state.undo(), "undo restores group");
        state.select_clip_linked(clips[0], true);
        assert_eq!(state.selection.len(), 2);
    }

    #[test]
    fn speed_duration_dialog_validates_and_applies_in_one_undo_step() {
        let (mut state, clip_id) = fixtures::with_video();
        let before = state.ui_clip_ref(clip_id).expect("clip").clone();
        let base_ticks = before.timeline_end_ticks - before.timeline_start_ticks;
        assert!(before.preserve_pitch, "a new clip keeps its pitch");

        state.open_speed_duration();
        assert_eq!(state.dialog, DialogKind::SpeedDuration);
        assert_eq!(state.speed_duration.speed_percent, 100.0);
        assert_eq!(state.speed_duration.duration_ticks, base_ticks);

        // Out-of-range speed is refused with a stated reason.
        assert!(state.set_speed_duration_percent(0.0).is_err());
        assert!(state.speed_duration.error.is_some());

        // 200% halves the duration.
        state
            .set_speed_duration_percent(200.0)
            .expect("200% is in range");
        assert_eq!(state.speed_duration.duration_ticks, base_ticks / 2);
        state.toggle_speed_duration_reverse();
        state.toggle_speed_duration_preserve_pitch();

        let depth_before = applied_steps(&state);
        state.apply_speed_duration();
        assert_eq!(state.dialog, DialogKind::None);
        assert_eq!(applied_steps(&state), depth_before + 1);

        let after = state.ui_clip_ref(clip_id).expect("clip");
        assert_eq!((after.speed_num, after.speed_den), (2, 1));
        assert_eq!(
            after.timeline_end_ticks - after.timeline_start_ticks,
            base_ticks / 2
        );
        assert!(after.reversed);
        assert!(!after.preserve_pitch, "the toggle unlocks the pitch");

        assert!(state.undo(), "undo speed/duration");
        let reverted = state.ui_clip_ref(clip_id).expect("clip");
        assert_eq!((reverted.speed_num, reverted.speed_den), (1, 1));
        assert_eq!(
            reverted.timeline_end_ticks - reverted.timeline_start_ticks,
            base_ticks
        );
        assert!(!reverted.reversed);
        assert!(reverted.preserve_pitch, "undo locks the pitch again");
    }

    #[test]
    fn toggle_time_remap_sets_and_clears_keyframed_speed_curve() {
        let (mut state, clip_id) = fixtures::with_video();
        assert!(!state.ui_clip_ref(clip_id).expect("clip").time_remap);

        state.toggle_time_remap().expect("enables time remap");
        assert!(state.ui_clip_ref(clip_id).expect("clip").time_remap);
        assert_eq!(state.active_panel, ActivePanel::Inspector);
        assert_eq!(state.inspector_tab, "speed");

        state.toggle_time_remap().expect("disables time remap");
        assert!(!state.ui_clip_ref(clip_id).expect("clip").time_remap);

        assert!(state.undo(), "undo restores time remap");
        assert!(state.ui_clip_ref(clip_id).expect("clip").time_remap);
    }

    #[test]
    fn audio_gain_modes_set_adjust_and_normalize_peaks_in_one_undo_step() {
        let (mut state, clip_id) = fixtures::with_audio();

        state.open_audio_gain();
        assert_eq!(state.dialog, DialogKind::AudioGain);
        assert_eq!(state.audio_gain.clip_count, 1);
        let peak_dbfs = state
            .audio_gain
            .max_peak_dbfs
            .expect("decoded wav has a measured peak");

        // Mode 1: Set Gain to -6 dB.
        state.set_audio_gain_mode(AudioGainMode::SetGain);
        state.set_audio_gain_set_db(-6.0).expect("valid set dB");
        state.apply_audio_gain();
        assert_eq!(state.dialog, DialogKind::None);
        let vol_after_set = state.ui_clip_ref(clip_id).expect("clip").volume;
        assert!((vol_after_set - linear_from_db(-6.0)).abs() < 1e-3);

        // Mode 2: Adjust Gain by +3 dB.
        state.open_audio_gain();
        state.set_audio_gain_mode(AudioGainMode::AdjustGain);
        state
            .set_audio_gain_adjust_db(3.0)
            .expect("valid adjust dB");
        state.apply_audio_gain();
        let vol_after_adj = state.ui_clip_ref(clip_id).expect("clip").volume;
        assert!((vol_after_adj - linear_from_db(-3.0)).abs() < 1e-2);

        // Mode 3: Normalize Max Peak to -1 dBFS.
        state.open_audio_gain();
        state.set_audio_gain_mode(AudioGainMode::NormalizeMaxPeak);
        state
            .set_audio_gain_target_db(-1.0)
            .expect("valid target dBFS");
        state.apply_audio_gain();
        let vol_norm_max = state.ui_clip_ref(clip_id).expect("clip").volume;
        let expected_norm = linear_from_db(-1.0 - peak_dbfs).min(MAX_CLIP_VOLUME);
        assert!((vol_norm_max - expected_norm).abs() < 1e-3);

        // Mode 4: Normalize All Peaks to -3 dBFS.
        state.open_audio_gain();
        state.set_audio_gain_mode(AudioGainMode::NormalizeAllPeaks);
        state
            .set_audio_gain_target_db(-3.0)
            .expect("valid target dBFS");
        state.apply_audio_gain();
        let vol_norm_all = state.ui_clip_ref(clip_id).expect("clip").volume;
        let expected_all = linear_from_db(-3.0 - peak_dbfs).min(MAX_CLIP_VOLUME);
        assert!((vol_norm_all - expected_all).abs() < 1e-3);
    }
}
