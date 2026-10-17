//! `av-sync` scenario: video presentation offset relative to the audio clock.

use aria_decoder::{OutputMode, open_video_decoder};
use aria_models::{MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};
use aria_render::ExportAudioMixer;
use aria_ui::preview_worker::Cadence;

use crate::fixtures::{FixtureId, ensure_fixture};
use crate::scenarios::BenchContext;
use crate::{Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation};

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    let h264 = ensure_fixture(&ctx.fixtures_dir, FixtureId::H264_1080p60)?;
    let rate = Rational::FPS_60;
    let fps = rate.as_f64();
    let sample_rate = 48_000u32;

    let mut project = Project::new("av-sync", rate);
    let media = project.add_media(MediaSource::new(&h264.path, 1920, 1080, rate, 3_600, true));
    let v_track = project.add_track(TrackKind::Video, "V1");
    let a_track = project.add_track(TrackKind::Audio, "A1");
    project
        .add_clip(
            v_track,
            media,
            TimeRange::at_rate(0, 3_600, rate),
            RationalTime::new(0, rate),
        )
        .map_err(|err| format!("failed to add video clip: {err}"))?;
    project
        .add_clip(
            a_track,
            media,
            TimeRange::at_rate(0, 3_600, rate),
            RationalTime::new(0, rate),
        )
        .map_err(|err| format!("failed to add audio clip: {err}"))?;

    let mut decoder = open_video_decoder(&h264.path, OutputMode::Cpu)
        .map_err(|err| format!("failed to open {}: {err}", h264.path.display()))?;
    let mut mixer = ExportAudioMixer::for_project(&project)
        .map_err(|err| format!("failed to create ExportAudioMixer: {err}"))?
        .ok_or_else(|| "ExportAudioMixer::for_project returned None".to_string())?;
    let mut audio_buf = vec![0.0f32; 256 * 2];
    let cadence = Cadence {
        anchor: 0,
        speed_num: 1,
        speed_den: 1,
        end: 3_600,
        rate,
    };
    let total_frames = 180i64;
    let mut abs_offsets_frames = Vec::with_capacity(total_frames as usize);
    let mut signed_offsets_ms = Vec::with_capacity(total_frames as usize);
    let mut mixed_samples = 0i64;
    let mut shown_slot = -1i64;

    for frame_idx in 0..total_frames {
        let vf = decoder
            .next_frame()
            .map_err(|err| format!("video decode error at frame {frame_idx}: {err}"))?
            .ok_or_else(|| format!("unexpected EOF at frame {frame_idx}"))?;

        let pts_secs = vf.pts.seconds();

        let target_samples = ((frame_idx as i128) * i128::from(sample_rate) / 60) as i64;
        while mixed_samples + 256 <= target_samples {
            mixer
                .mix_into(mixed_samples, &mut audio_buf)
                .map_err(|err| format!("audio mix_into failed at {mixed_samples}: {err}"))?;
            std::hint::black_box(audio_buf[0]);
            mixed_samples += 256;
        }
        let sub_block_offset = target_samples - mixed_samples;
        let audio_heard_samples = mixed_samples + sub_block_offset;
        let audio_secs = (audio_heard_samples as f64) / f64::from(sample_rate);
        let audio_tick = ((audio_heard_samples * 60 + i64::from(sample_rate) / 2)
            / i64::from(sample_rate))
        .clamp(0, total_frames - 1);

        let now_slot = cadence.slot_at(audio_tick);
        shown_slot = shown_slot.max(now_slot);
        let shown_tick = cadence.tick(shown_slot);

        let presented_secs = if shown_tick == frame_idx {
            pts_secs
        } else {
            (shown_tick as f64) / fps
        };
        let offset_secs = presented_secs - audio_secs;
        let offset_frames = (offset_secs * fps).abs();
        abs_offsets_frames.push(offset_frames);
        signed_offsets_ms.push(offset_secs * 1000.0);
    }

    let offset_series =
        SampleSeries::from_values("av_offset_abs_frames", "frames", abs_offsets_frames);
    let ms_series = SampleSeries::from_values("av_offset_signed_ms", "ms", signed_offsets_ms);

    let targets = vec![TargetEvaluation::measured(
        "max_abs_offset_frames",
        "Video presentation within half a frame of the audio clock for every frame",
        Comparison::AtMost,
        0.5,
        "frames",
        offset_series.max,
    )];

    let notes = vec![format!(
        "Measured {} frames of h264-1080p60 against 48 kHz ExportAudioMixer + Cadence slot presentation: max |offset|={:.4} frames (p95={:.4} frames)",
        total_frames, offset_series.max, offset_series.p95
    )];

    Ok(ScenarioReport::new(
        ScenarioId::AvSync,
        ctx.env.clone(),
        targets,
        Vec::new(),
        vec![offset_series, ms_series],
        notes,
    ))
}
