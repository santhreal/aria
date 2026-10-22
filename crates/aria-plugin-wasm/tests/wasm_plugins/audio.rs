//! WASM audio effect registration, stereo block processing, deadline misses, and processor traps.

use std::fs;
use std::path::Path;

use aria_core::{Rational, RationalTime, TimeRange};
use aria_models::{AudioInsertTarget, MediaSource, Param, Project, TrackKind, audio_effect_spec};
use aria_plugin_wasm::{Limits, load};
use aria_render::{
    EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, ExportAudioMixer, audio_processor_registered,
};

use super::{FIXTURE_WASM, FLUSH_TIMEOUT, SERIAL, setup_host_context, write_package};

const BLOCK_FRAMES: usize = 256;

fn write_wav_tone(path: &Path, duration_ms: u64, hz: f32) {
    let sample_rate = EXPORT_AUDIO_RATE;
    let channels = EXPORT_AUDIO_CHANNELS;
    let frames = (duration_ms * u64::from(sample_rate) / 1000) as usize;
    let samples_len = frames * usize::from(channels);
    let data_bytes = (samples_len * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * u32::from(channels) * 2;
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = channels * 2;
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for frame in 0..frames {
        let t = frame as f32 / sample_rate as f32;
        let v = ((t * hz * std::f32::consts::TAU).sin() * 0.5 * f32::from(i16::MAX)) as i16;
        for _ in 0..channels {
            wav.extend_from_slice(&v.to_le_bytes());
        }
    }
    fs::write(path, wav).expect("write wav tone");
}

/// Registered WASM audio effect processes stereo blocks, reports 3 consecutive missed
/// deadlines without unmounting, and faults the plugin on a processor trap.
#[test]
fn audio_effect_processes_samples_and_handles_missed_deadlines_and_traps() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");
    assert!(audio_effect_spec("example.gain").is_some());
    assert!(audio_processor_registered("example.gain"));

    // Build an audible project with a 440 Hz tone on an audio lane.
    let wav_path = temp.path().join("tone.wav");
    write_wav_tone(&wav_path, 500, 440.0);
    let mut base_project = Project::new("audio test", Rational::FPS_24);
    let media = base_project.add_media(MediaSource::new(
        &wav_path,
        0,
        0,
        Rational::new(1, 1000),
        500,
        true,
    ));
    let lane = base_project.add_track(TrackKind::Audio, "A1");
    base_project
        .add_clip(
            lane,
            media,
            TimeRange::at_rate(0, 500, Rational::new(1, 1000)),
            RationalTime::new(0, Rational::FPS_24),
        )
        .expect("add audio clip");

    let mix_blocks = |project: &Project, blocks: usize, realtime: bool| -> Vec<f32> {
        let mut mixer = ExportAudioMixer::for_project(project)
            .expect("mixer opens")
            .expect("project is audible");
        mixer.set_realtime(realtime);
        let channels = usize::from(EXPORT_AUDIO_CHANNELS);
        let total_frames = blocks * BLOCK_FRAMES;
        let mut out = vec![0.0; total_frames * channels];
        for (idx, chunk) in out.chunks_mut(BLOCK_FRAMES * channels).enumerate() {
            mixer
                .mix_into((idx * BLOCK_FRAMES) as i64, chunk)
                .expect("mix block");
        }
        out
    };

    let dry = mix_blocks(&base_project, 4, false);
    assert!(dry.iter().any(|s| s.abs() > 0.1));

    // Add `example.gain` with `gain = 0.5` on Master and assert exact halved samples.
    let mut gain_project = base_project.clone();
    gain_project
        .add_audio_effect(AudioInsertTarget::Master, "example.gain")
        .expect("add example.gain");
    gain_project
        .set_audio_effect_param(AudioInsertTarget::Master, 0, 0, Param::Constant(0.5))
        .expect("set gain = 0.5");

    let wet = mix_blocks(&gain_project, 4, false);
    let max_diff = dry
        .iter()
        .zip(wet.iter())
        .map(|(d, w)| (d * 0.5 - w).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        max_diff <= 1e-6,
        "expected exact 0.5x gain, max_diff={max_diff}"
    );

    // Set `gain = -1.0` (slow block mode): 3 missed deadlines report a fault and bypass the insert.
    let mut slow_project = gain_project.clone();
    slow_project
        .set_audio_effect_param(AudioInsertTarget::Master, 0, 0, Param::Constant(-1.0))
        .expect("set gain = -1.0");
    let _ = mix_blocks(&slow_project, 4, true);
    let faults = ctx.faults();
    assert_eq!(faults.len(), 1);
    assert_eq!(faults[0].plugin, "com.example.wasm-tools");
    assert!(
        faults[0]
            .message
            .contains("example.gain missed three playback deadlines"),
        "unexpected fault message: {}",
        faults[0].message
    );
    assert!(
        ctx.is_mounted("com.example.wasm-tools"),
        "missed audio deadlines bypass the insert without unmounting the plugin"
    );

    // Set `gain = -2.0` (trap mode): the processor traps, silences the block, and fails the plugin.
    let mut trap_project = gain_project;
    trap_project
        .set_audio_effect_param(AudioInsertTarget::Master, 0, 0, Param::Constant(-2.0))
        .expect("set gain = -2.0");
    let trapped_out = mix_blocks(&trap_project, 2, false);
    let block_samples = BLOCK_FRAMES * usize::from(EXPORT_AUDIO_CHANNELS);
    let (first_block, second_block) = trapped_out.split_at(block_samples);
    assert!(
        first_block.iter().all(|s| *s == 0.0),
        "faulting block is silenced"
    );
    let bypass_diff = second_block
        .iter()
        .zip(&dry[block_samples..2 * block_samples])
        .map(|(got, want)| (got - want).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        bypass_diff <= 1e-6,
        "second block bypasses the faulted processor"
    );

    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(audio_effect_spec("example.gain").is_none());
    assert!(!audio_processor_registered("example.gain"));
}
