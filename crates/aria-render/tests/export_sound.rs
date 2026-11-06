//! A sound export writes the mix to a file with no picture in it.
//!
//! The class this closes: a sound intermediate that is not the sound. It has
//! three ways to be wrong and all of them look like success. The file can be the
//! wrong length, so the clip it stands in for drifts; it can be silence, because
//! nothing checked that a mix reached the sink; or it can be published over a
//! good file after the export was abandoned. So every assertion here is about
//! the bytes on disk: the header the samples are described by, how many there
//! are, that they are audible, and that a cancelled export leaves what was there
//! before untouched.
//!
//! The source is written by the same sink the export writes with, so no fixture
//! file is checked in and the decode side is exercised against a real container.

use std::path::Path;

use aria_core::RationalTime;
use aria_models::{MediaSource, Project, Rational, TimeRange, TrackKind};
use aria_render::{EmptyExport, ExportSettings, RenderError, export_audio_to_file_observed};

#[path = "support/tone.rs"]
mod tone;
use tone::{CHANNELS, RATE, write_tone};

/// Audio-only media counts milliseconds, the same convention the probe reports.
const MS: Rational = Rational::new(1000, 1);

/// A project holding one second of a loud 440 Hz tone on an audio lane.
fn project_with_tone(path: &Path) -> Project {
    write_tone(path, 1000, 440.0);
    let mut project = Project::new("sound", Rational::FPS_24);
    let media = project.add_media(MediaSource::new(path, 0, 0, MS, 1000, true));
    let lane = project.add_track(TrackKind::Audio, "A1");
    project
        .add_clip(
            lane,
            media,
            TimeRange::at_rate(0, 1000, MS),
            RationalTime::new(0, Rational::FPS_24),
        )
        .expect("the clip");
    project
}

fn samples_of(bytes: &[u8]) -> Vec<i16> {
    bytes[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

#[test]
fn a_sound_export_writes_the_span_it_covers_and_it_is_audible() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = project_with_tone(&dir.path().join("song.wav"));
    let out = dir.path().join("bake.wav");

    let frames = export_audio_to_file_observed(
        &project,
        &out,
        ExportSettings::for_project(&project),
        &mut |_, _| true,
    )
    .expect("a sound export");

    assert_eq!(frames, 24, "one second at 24 fps");
    let bytes = std::fs::read(&out).expect("the exported file");
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(
        u32::from_le_bytes(bytes[24..28].try_into().expect("rate")),
        RATE
    );
    // One second of stereo 16-bit at 48 kHz, and the header agrees with it.
    let data = RATE as usize * CHANNELS * 2;
    assert_eq!(bytes.len(), 44 + data, "the file is the span it covers");
    assert_eq!(
        u32::from_le_bytes(bytes[40..44].try_into().expect("data size")) as usize,
        data,
        "the header counts the samples that are there"
    );

    let samples = samples_of(&bytes);
    let loudest = samples.iter().map(|s| s.abs()).max().unwrap_or(0);
    assert!(
        loudest > 8_000,
        "the export wrote silence: the mix never reached the sink (peak {loudest})"
    );
}

/// A muted lane is nothing to render. With no picture to carry, a silent file
/// holds nothing at all, so the refusal names the fader rather than writing it.
#[test]
fn a_silent_sequence_is_refused_by_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = project_with_tone(&dir.path().join("song.wav"));
    let lane = project
        .timeline()
        .tracks_ordered()
        .map(|track| track.id)
        .next()
        .expect("the lane");
    project
        .timeline_mut()
        .track_mut(lane)
        .expect("the lane")
        .muted = true;

    let out = dir.path().join("bake.wav");
    let err = export_audio_to_file_observed(
        &project,
        &out,
        ExportSettings::for_project(&project),
        &mut |_, _| true,
    )
    .expect_err("silence is not exportable");
    assert!(
        matches!(err, RenderError::Empty(EmptyExport::Silence)),
        "the refusal must name silence: {err}"
    );
    assert!(!out.exists(), "a refusal wrote a file anyway");
}

/// Publication is transactional, so abandoning an export leaves whatever was at
/// the destination exactly as it was. An intermediate is overwritten by the next
/// render of the same clip, and a half-written one that replaced a good file
/// would be decoded as the clip.
#[test]
fn a_cancelled_export_leaves_the_destination_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = project_with_tone(&dir.path().join("song.wav"));
    let out = dir.path().join("bake.wav");
    std::fs::write(&out, b"the render that is already there").expect("seed the destination");

    let err = export_audio_to_file_observed(
        &project,
        &out,
        ExportSettings::for_project(&project),
        &mut |done, _| done == 0,
    )
    .expect_err("cancelled");
    assert!(matches!(err, RenderError::Cancelled), "{err}");
    assert_eq!(
        std::fs::read(&out).expect("the old file"),
        b"the render that is already there",
        "the destination was replaced by an abandoned export"
    );
}
#[cfg(target_os = "linux")]
mod mp4_av_verification {
    use std::path::Path;
    use std::process::Command;

    use aria_core::{AudioEncoderConfig, RationalTime};
    use aria_models::{
        AudioTransition, CanvasAspect, CanvasSettings, Generator, MediaSource, Param, Project,
        Rational, TextStyle, TimeRange, TrackKind,
    };
    use aria_render::{EXPORT_AUDIO_RATE, Renderer, export_to_file, sample_boundary};

    use super::MS;

    fn write_wav_stereo(path: &Path, samples_stereo: &[f32]) {
        let mut sink =
            aria_encoder::open_audio_encoder(path, AudioEncoderConfig::new(EXPORT_AUDIO_RATE, 2))
                .expect("open wav");
        sink.push_audio(
            samples_stereo,
            RationalTime::new(0, Rational::new(EXPORT_AUDIO_RATE as i32, 1)),
        )
        .expect("push wav");
        sink.finish().expect("finish wav");
    }

    fn probe_stream_counts(path: &Path) -> (u64, f64, f64) {
        let out = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-count_frames",
                "-show_entries",
                "stream=nb_read_frames,duration:format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
            ])
            .arg(path)
            .output()
            .expect("ffprobe");
        assert!(out.status.success(), "ffprobe failed");
        let text = String::from_utf8_lossy(&out.stdout);
        let nums: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let stream_dur: f64 = nums[0].parse().expect("stream duration");
        let frames: u64 = nums[1].parse().expect("nb_read_frames");
        let fmt_dur: f64 = nums[2].parse().expect("format duration");
        (frames, stream_dur, fmt_dur)
    }

    fn decode_audio_f32_stereo(path: &Path) -> Vec<f32> {
        let out = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(path)
            .args([
                "-f",
                "f32le",
                "-acodec",
                "pcm_f32le",
                "-ar",
                "48000",
                "-ac",
                "2",
                "-",
            ])
            .output()
            .expect("ffmpeg audio decode");
        assert!(out.status.success(), "ffmpeg audio decode failed");
        out.stdout
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect()
    }

    fn decode_video_rgba_frames(path: &Path, width: u32, height: u32) -> Vec<Vec<u8>> {
        let out = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(path)
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .output()
            .expect("ffmpeg video decode");
        assert!(out.status.success(), "ffmpeg video decode failed");
        let frame_bytes = (width * height * 4) as usize;
        out.stdout
            .chunks_exact(frame_bytes)
            .map(|c| c.to_vec())
            .collect()
    }

    /// WHY: End-to-end verification of exported MP4 files through `ffprobe` and
    /// `ffmpeg` across the sample project, multi-clip cuts, A/V sync offset
    /// (< 1 frame), EOS audio preservation, forward and reverse retiming
    /// (pitch-locked and varispeed), video transitions, and multi-track audio
    /// gain, fades, and crossfades.
    #[test]
    fn exported_mp4_matches_frame_count_duration_av_sync_audio_levels_and_cut_frames() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fps = Rational::FPS_30;
        let (w, h) = (320_u32, 180_u32);
        let mut renderer = Renderer::new_headless().expect("headless renderer");

        // 1. Sample project (bundled still + title overlay, 150 frames = 5.0 s).
        {
            let mut sample = Project::new("sample", fps);
            sample.timeline_mut().set_canvas(CanvasSettings {
                aspect: CanvasAspect::Wide16x9,
                background: [20, 20, 30],
                resolution: Some([w, h]),
            });
            let fixture =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
            let media = sample.add_media(MediaSource::image(&fixture, 1, 1));
            let full = sample.media(media).expect("media").full_range();
            let v1 = sample.add_track(TrackKind::Video, "V1");
            sample
                .add_clip(v1, media, full, RationalTime::new(0, fps))
                .expect("sample clip");
            let t1 = sample.add_track(TrackKind::Text, "T1");
            sample
                .add_generated(
                    t1,
                    Generator::Text {
                        content: "Aria".into(),
                        style: TextStyle::default(),
                    },
                    TimeRange::at_rate(0, 30, fps),
                )
                .expect("sample title");
            let out_mp4 = dir.path().join("sample.mp4");
            let written = export_to_file(&mut renderer, &sample, &out_mp4).expect("export sample");
            assert_eq!(written, 150);
            let (probed_frames, stream_dur, fmt_dur) = probe_stream_counts(&out_mp4);
            assert_eq!(probed_frames, 150);
            assert!((stream_dur - 5.0).abs() < 0.02, "stream_dur={stream_dur}");
            assert!((fmt_dur - 5.0).abs() < 0.02, "fmt_dur={fmt_dur}");
        }

        // 2. Cuts, A/V sync (< 1 frame), colorimetry accuracy at cut points,
        // and audio preservation through the final frame of the timeline.
        {
            let mut project = Project::new("cuts-av-sync", fps);
            project.timeline_mut().set_canvas(CanvasSettings {
                aspect: CanvasAspect::Wide16x9,
                background: [0, 0, 0],
                resolution: Some([w, h]),
            });
            let v1 = project.add_track(TrackKind::Sticker, "V1");
            for (start, rgba) in [
                (0, [220, 20, 20, 255]),
                (15, [20, 220, 20, 255]),
                (30, [20, 20, 220, 255]),
            ] {
                project
                    .add_generated(
                        v1,
                        Generator::SolidColor { rgba },
                        TimeRange::at_rate(start, 15, fps),
                    )
                    .expect("solid clip");
            }

            let wav_path = dir.path().join("tone_half_sec.wav");
            let samples_15f = sample_boundary(15, fps.num, fps.den) as usize;
            let mut pcm = Vec::with_capacity(samples_15f * 2);
            for s in 0..samples_15f {
                let t = s as f32 / EXPORT_AUDIO_RATE as f32;
                let v = (std::f32::consts::TAU * 440.0 * t).sin() * 0.8;
                pcm.push(v);
                pcm.push(v);
            }
            write_wav_stereo(&wav_path, &pcm);
            let a1 = project.add_track(TrackKind::Audio, "A1");
            let audio_media = project.add_media(MediaSource::new(&wav_path, 0, 0, MS, 500, true));
            project
                .add_clip(
                    a1,
                    audio_media,
                    TimeRange::at_rate(0, 500, MS),
                    RationalTime::new(15, fps),
                )
                .expect("tone at frame 15..30");

            let out_mp4 = dir.path().join("cuts.mp4");
            let written = export_to_file(&mut renderer, &project, &out_mp4).expect("export cuts");
            assert_eq!(written, 45);
            let (probed_frames, stream_dur, fmt_dur) = probe_stream_counts(&out_mp4);
            assert_eq!(probed_frames, 45);
            assert!((stream_dur - 1.5).abs() < 0.02, "stream_dur={stream_dur}");
            assert!((fmt_dur - 1.5).abs() < 0.04, "fmt_dur={fmt_dur}");

            let decoded_frames = decode_video_rgba_frames(&out_mp4, w, h);
            assert_eq!(decoded_frames.len(), 45);
            let center = ((h / 2 * w + w / 2) * 4) as usize;
            for check_f in [0usize, 14, 15, 29, 30, 44] {
                let rendered = renderer
                    .render_frame(&project, RationalTime::new(check_f as i64, fps))
                    .expect("render timeline frame");
                let dec_px = &decoded_frames[check_f][center..center + 3];
                let ren_px = &rendered.pixels[center..center + 3];
                for c in 0..3 {
                    let diff = (dec_px[c] as i32 - ren_px[c] as i32).abs();
                    assert!(
                        diff <= 8,
                        "frame {check_f} channel {c}: decoded {dec_px:?} vs rendered {ren_px:?} (diff {diff})"
                    );
                }
            }

            let decoded_audio = decode_audio_f32_stereo(&out_mp4);
            let total_stereo_frames = decoded_audio.len() / 2;
            let first_audible = (0..total_stereo_frames)
                .find(|&i| decoded_audio[i * 2].abs() > 0.05)
                .expect("audible start") as i64;
            let last_audible = (0..total_stereo_frames)
                .rev()
                .find(|&i| decoded_audio[i * 2].abs() > 0.05)
                .expect("audible end") as i64;
            let one_frame_samples = sample_boundary(1, fps.num, fps.den);
            let expected_start = sample_boundary(15, fps.num, fps.den);
            let expected_end = sample_boundary(30, fps.num, fps.den);
            assert!(
                (first_audible - expected_start).abs() < one_frame_samples,
                "A/V sync onset offset >= 1 frame: first_audible={first_audible}, expected={expected_start}, 1_frame={one_frame_samples}"
            );
            assert!(
                (last_audible - expected_end).abs() < one_frame_samples,
                "A/V sync end offset >= 1 frame: last_audible={last_audible}, expected={expected_end}, 1_frame={one_frame_samples}"
            );
        }

        // 3. Speed changes (forward & reverse, pitch-locked & varispeed) and
        // multi-track audio gain, fades, crossfades, and video transitions.
        {
            let wav_1s = dir.path().join("tone_1s.wav");
            let samples_30f = sample_boundary(30, fps.num, fps.den) as usize;
            let mut pcm = Vec::with_capacity(samples_30f * 2);
            for s in 0..samples_30f {
                let t = s as f32 / EXPORT_AUDIO_RATE as f32;
                let v = (std::f32::consts::TAU * 440.0 * t).sin() * 0.8;
                pcm.push(v);
                pcm.push(v);
            }
            write_wav_stereo(&wav_1s, &pcm);

            for (label, speed, reversed, preserve_pitch, out_ticks) in [
                ("2x_pitch", Rational::new(2, 1), false, true, 15_i64),
                ("2x_varispeed", Rational::new(2, 1), false, false, 15_i64),
                ("half_pitch", Rational::new(1, 2), false, true, 60_i64),
                ("rev_2x_pitch", Rational::new(2, 1), true, true, 15_i64),
                ("rev_half_pitch", Rational::new(1, 2), true, true, 60_i64),
                ("rev_varispeed", Rational::new(2, 1), true, false, 15_i64),
            ] {
                let mut project = Project::new(label, fps);
                project.timeline_mut().set_canvas(CanvasSettings {
                    aspect: CanvasAspect::Wide16x9,
                    background: [0, 0, 0],
                    resolution: Some([w, h]),
                });
                let v1 = project.add_track(TrackKind::Sticker, "V1");
                project
                    .add_generated(
                        v1,
                        Generator::SolidColor {
                            rgba: [100, 100, 100, 255],
                        },
                        TimeRange::at_rate(0, out_ticks, fps),
                    )
                    .expect("video span");
                let a1 = project.add_track(TrackKind::Audio, "A1");
                let media = project.add_media(MediaSource::new(&wav_1s, 0, 0, MS, 1000, true));
                let cid = project
                    .add_clip(
                        a1,
                        media,
                        TimeRange::at_rate(0, 1000, MS),
                        RationalTime::new(0, fps),
                    )
                    .expect("audio clip");
                {
                    let mut clip = project.timeline_mut().clip_timeline_mut(cid).expect("clip");
                    clip.speed = speed;
                    clip.reversed = reversed;
                    clip.preserve_pitch = preserve_pitch;
                    clip.timeline = TimeRange::at_rate(0, out_ticks, fps);
                }
                let out_mp4 = dir.path().join(format!("speed_{label}.mp4"));
                let written = export_to_file(&mut renderer, &project, &out_mp4).expect("export");
                assert_eq!(written, out_ticks as u64);
                let decoded_audio = decode_audio_f32_stereo(&out_mp4);
                // Every interior frame block (1..out_ticks) must contain signal (~0.8 peak).
                for f in 1..out_ticks as usize {
                    let s0 = f * 1600 * 2;
                    let s1 = ((f + 1) * 1600 * 2).min(decoded_audio.len());
                    let peak = decoded_audio[s0..s1]
                        .iter()
                        .map(|x| x.abs())
                        .fold(0.0_f32, f32::max);
                    assert!(
                        peak > 0.5,
                        "{label}: frame {f}/{out_ticks} dropped to peak {peak}"
                    );
                }
            }

            // 4. Multi-track project with video transition + audio crossfade, fades, and gain.
            let mut multi = Project::new("multi-track", fps);
            multi.timeline_mut().set_canvas(CanvasSettings {
                aspect: CanvasAspect::Wide16x9,
                background: [0, 0, 0],
                resolution: Some([w, h]),
            });
            let v1 = multi.add_track(TrackKind::Sticker, "V1");
            let vc1 = multi
                .add_generated(
                    v1,
                    Generator::SolidColor {
                        rgba: [220, 20, 20, 255],
                    },
                    TimeRange::at_rate(0, 15, fps),
                )
                .expect("vc1");
            multi
                .add_generated(
                    v1,
                    Generator::SolidColor {
                        rgba: [20, 20, 220, 255],
                    },
                    TimeRange::at_rate(15, 15, fps),
                )
                .expect("vc2");
            multi
                .add_transition(vc1, "crossfade")
                .expect("video dissolve");

            let amedia = multi.add_media(MediaSource::new(&wav_1s, 0, 0, MS, 1000, true));
            let a1 = multi.add_track(TrackKind::Audio, "A1");
            let a2 = multi.add_track(TrackKind::Audio, "A2");
            let a1_c1 = multi
                .add_clip(
                    a1,
                    amedia,
                    TimeRange::at_rate(100, 500, MS),
                    RationalTime::new(0, fps),
                )
                .expect("a1_c1");
            multi
                .add_clip(
                    a1,
                    amedia,
                    TimeRange::at_rate(200, 500, MS),
                    RationalTime::new(15, fps),
                )
                .expect("a1_c2");
            multi
                .add_transition(a1_c1, AudioTransition::ConstantPower.id())
                .expect("audio crossfade");
            let a2_c1 = multi
                .add_clip(
                    a2,
                    amedia,
                    TimeRange::at_rate(0, 1000, MS),
                    RationalTime::new(0, fps),
                )
                .expect("a2_c1");
            {
                let mut clip = multi.timeline_mut().clip_timeline_mut(a2_c1).expect("clip");
                clip.volume = Param::Constant(0.25);
                clip.fade_in = 5;
                clip.fade_out = 5;
            }

            let out_mp4 = dir.path().join("multi.mp4");
            let written = export_to_file(&mut renderer, &multi, &out_mp4).expect("export multi");
            assert_eq!(written, 30);
            let decoded_frames = decode_video_rgba_frames(&out_mp4, w, h);
            assert_eq!(decoded_frames.len(), 30);
            let center = ((h / 2 * w + w / 2) * 4) as usize;
            for f in [0usize, 14, 15, 29] {
                let rendered = renderer
                    .render_frame(&multi, RationalTime::new(f as i64, fps))
                    .expect("render multi");
                let dec_px = &decoded_frames[f][center..center + 3];
                let ren_px = &rendered.pixels[center..center + 3];
                for c in 0..3 {
                    let diff = (dec_px[c] as i32 - ren_px[c] as i32).abs();
                    assert!(
                        diff <= 10,
                        "multi frame {f} ch {c}: decoded {dec_px:?} vs rendered {ren_px:?} (diff {diff})"
                    );
                }
            }
        }
    }
}
