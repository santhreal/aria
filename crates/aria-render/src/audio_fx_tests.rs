//! The shipped audio effects, driven directly through the processor contract.
//!
//! The class this closes: a shipped effect whose processor disagrees with its
//! catalog entry (a slot read under another name, a latency the mixer does not
//! compensate, a key the spec does not declare), whose output depends on how
//! the stream was cut into blocks or on what it processed before a reset, and
//! each effect's own arithmetic. Every effect in [`SHIPPED`] goes through the
//! class tests with no list to update; an effect added there is covered or
//! turns them red. The mixer's handling of chains is `tests/audio_inserts.rs`.

use std::f32::consts::PI;

use super::*;

const RATE: u32 = 48_000;

/// Every slot of `id` at its default, with `set` applied by name.
fn values(id: &str, set: &[(&str, f32)]) -> Vec<f32> {
    let spec = aria_models::audio_effect_spec(id).unwrap_or_else(|| panic!("{id} ships"));
    for (name, _) in set {
        assert!(spec.param(name).is_some(), "{id} has no parameter {name}");
    }
    spec.params
        .iter()
        .map(|param| {
            set.iter()
                .find(|(name, _)| *name == param.name)
                .map_or(param.default, |(_, value)| *value)
        })
        .collect()
}

/// Every slot of `id` three quarters of the way up its range.
fn high_values(id: &str) -> Vec<f32> {
    let spec = aria_models::audio_effect_spec(id).unwrap_or_else(|| panic!("{id} ships"));
    spec.params
        .iter()
        .map(|param| param.min + 0.75 * (param.max - param.min))
        .collect()
}

fn processor(id: &str) -> Box<dyn AudioProcessor> {
    (shipped(id).unwrap_or_else(|| panic!("{id} ships")).make)(RATE)
}

/// Drive `processor` over the stereo signal in blocks of `block` frames,
/// with constant parameter `values` and an optional key.
fn drive(
    processor: &mut dyn AudioProcessor,
    values: &[f32],
    stereo: (&mut [f32], &mut [f32]),
    key: Option<(&[f32], &[f32])>,
    block: usize,
) {
    let signals: Vec<ParamSignal<'_>> = values.iter().map(|v| ParamSignal::Constant(*v)).collect();
    let (left, right) = stereo;
    let mut at = 0;
    for (left, right) in left.chunks_mut(block).zip(right.chunks_mut(block)) {
        let frames = left.len();
        let key = key
            .map(|(key_left, key_right)| (&key_left[at..at + frames], &key_right[at..at + frames]));
        let mut block = AudioBlock::new(left, right, at as i64, RATE, &signals, key);
        processor.process(&mut block);
        at += frames;
    }
}

/// `id` over copies of `left` and `right`, returned.
fn run(
    id: &str,
    values: &[f32],
    left: &[f32],
    right: &[f32],
    key: Option<(&[f32], &[f32])>,
    block: usize,
) -> (Vec<f32>, Vec<f32>) {
    let (mut out_left, mut out_right) = (left.to_vec(), right.to_vec());
    drive(
        &mut *processor(id),
        values,
        (&mut out_left, &mut out_right),
        key,
        block,
    );
    (out_left, out_right)
}

fn sine(freq: f32, amplitude: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| amplitude * (2.0 * PI * freq * i as f32 / RATE as f32).sin())
        .collect()
}

/// Uniform noise in `[-amplitude, amplitude]`, the same for one seed.
fn noise(amplitude: f32, frames: usize, seed: u32) -> Vec<f32> {
    let mut state = seed.wrapping_mul(2_654_435_761).max(1);
    (0..frames)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            amplitude * (state as f32 / u32::MAX as f32 * 2.0 - 1.0)
        })
        .collect()
}

/// A sawtooth whose pitch sweeps from 100 Hz to 400 Hz: harmonic like a
/// voice, and never periodic, so a correlation has one peak.
fn chirp(amplitude: f32, frames: usize) -> Vec<f32> {
    let mut phase = 0.0f32;
    (0..frames)
        .map(|i| {
            let freq = 100.0 + 300.0 * i as f32 / frames as f32;
            phase = (phase + freq / RATE as f32).fract();
            amplitude * (2.0 * phase - 1.0)
        })
        .collect()
}

fn rms(signal: &[f32]) -> f32 {
    (signal.iter().map(|s| s * s).sum::<f32>() / signal.len() as f32).sqrt()
}

fn peak(signal: &[f32]) -> f32 {
    signal.iter().fold(0.0, |held, s| held.max(s.abs()))
}

fn db(ratio: f32) -> f32 {
    20.0 * ratio.log10()
}

/// The power of `signal` at `freq`, by Goertzel's recurrence.
fn power_at(signal: &[f32], freq: f32) -> f32 {
    let k = 2.0 * (2.0 * std::f64::consts::PI * f64::from(freq) / f64::from(RATE)).cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &x in signal {
        let s0 = f64::from(x) + k * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - k * s1 * s2) as f32
}

/// Fails at the first sample that differs, naming it.
fn same(got: &[f32], want: &[f32], case: &str) {
    assert_eq!(got.len(), want.len(), "{case}: length");
    if let Some(at) = got
        .iter()
        .zip(want)
        .position(|(a, b)| a.to_bits() != b.to_bits())
    {
        panic!(
            "{case}: frame {at} is {} where {} was expected",
            got[at], want[at]
        );
    }
}

/// The lag in `0..=most` at which `output` best matches `input`.
fn lag(input: &[f32], output: &[f32], most: usize) -> usize {
    let span = input.len() - most;
    (0..=most)
        .map(|lag| {
            let score: f64 = (0..span)
                .map(|i| f64::from(input[i]) * f64::from(output[i + lag]))
                .sum();
            (lag, score)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(lag, _)| lag)
        .expect("a lag")
}

// --------------------------------------------------------------- the class --

/// The parameter names each processor reads, in slot order: what its code
/// means by `Knob::of(block, 0, ..)`, `Knob::of(block, 1, ..)` and on.
const SLOTS: &[(&str, &[&str])] = &[
    ("gain", &["gain"]),
    ("parametric_eq", eq::PARAMS),
    (
        "compressor",
        &["threshold", "ratio", "attack", "release", "knee", "makeup"],
    ),
    ("limiter", &["ceiling", "release", "input"]),
    ("deesser", &["frequency", "threshold", "range"]),
    ("noise_reduction", &["amount"]),
    ("reverb", &["size", "damping", "width", "mix", "predelay"]),
    ("delay", &["time", "feedback", "mix"]),
    ("pitch_shifter", &["semitones", "cents"]),
    ("swap_channels", &[]),
    ("fill_left", &[]),
    ("fill_right", &[]),
    ("mono", &[]),
    ("loudness_normalizer", &["target", "max_gain"]),
];

#[test]
fn every_shipped_audio_effect_has_a_processor_reading_its_slots() {
    let catalog = aria_models::shipped_audio_effect_catalog();
    for spec in catalog {
        assert!(
            shipped(&spec.id).is_some(),
            "{} is in the catalog with no processor",
            spec.id
        );
        let (_, slots) = SLOTS
            .iter()
            .find(|(id, _)| *id == spec.id)
            .unwrap_or_else(|| panic!("{} has no slot list here", spec.id));
        let names: Vec<&str> = spec.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, *slots, "{}: slot order", spec.id);
    }
    for entry in SHIPPED {
        assert!(
            catalog.iter().any(|spec| spec.id == entry.id),
            "{} has a processor and no catalog entry",
            entry.id
        );
    }
}

/// An effect reads a key exactly when its spec says it does: a loud key over
/// a quiet input changes the output of a sidechain effect and nothing else.
#[test]
fn only_an_effect_that_declares_a_key_reads_one() {
    let frames = RATE as usize / 2;
    let input = sine(440.0, 0.05, frames);
    let key = noise(0.9, frames, 7);
    for entry in SHIPPED {
        let spec = aria_models::audio_effect_spec(entry.id).expect("shipped");
        let values = values(entry.id, &[]);
        let plain = run(entry.id, &values, &input, &input, None, 480);
        let keyed = run(entry.id, &values, &input, &input, Some((&key, &key)), 480);
        assert_eq!(
            plain != keyed,
            spec.sidechain,
            "{}: declares sidechain = {} but a key {} its output",
            entry.id,
            spec.sidechain,
            if plain == keyed {
                "does not change"
            } else {
                "changes"
            }
        );
    }
}

/// How the stream is cut into blocks never changes what comes out, and a reset
/// forgets everything before it. Run at the defaults and with every parameter
/// high in its range.
#[test]
fn every_shipped_effect_ignores_block_boundaries_and_forgets_on_reset() {
    let frames = RATE as usize * 3 / 2;
    let left: Vec<f32> = sine(330.0, 0.3, frames)
        .iter()
        .zip(noise(0.1, frames, 3))
        .map(|(a, b)| a + b)
        .collect();
    let right: Vec<f32> = chirp(0.25, frames)
        .iter()
        .zip(noise(0.05, frames, 4))
        .map(|(a, b)| a + b)
        .collect();
    for entry in SHIPPED {
        for (setting, values) in [
            ("defaults", values(entry.id, &[])),
            ("high", high_values(entry.id)),
        ] {
            let case = format!("{} at {setting}", entry.id);
            let whole = run(entry.id, &values, &left, &right, None, frames);
            // An unstable recursion passes any bound within the run. The
            // loudest stable case, the equalizer with every bell stacked at
            // +12 dB on one frequency, peaks near 800.
            for out in [&whole.0, &whole.1] {
                assert!(
                    out.iter().all(|s| s.is_finite() && s.abs() < 1e4),
                    "{case}: the output is not finite and bounded"
                );
            }
            for block in [64, 1031] {
                let cut = run(entry.id, &values, &left, &right, None, block);
                same(
                    &cut.0,
                    &whole.0,
                    &format!("{case}, blocks of {block}, left"),
                );
                same(
                    &cut.1,
                    &whole.1,
                    &format!("{case}, blocks of {block}, right"),
                );
            }

            let mut used = processor(entry.id);
            let (mut warm_left, mut warm_right) = (right.clone(), left.clone());
            drive(
                &mut *used,
                &values,
                (&mut warm_left, &mut warm_right),
                None,
                512,
            );
            used.reset();
            let (mut again_left, mut again_right) = (left.clone(), right.clone());
            drive(
                &mut *used,
                &values,
                (&mut again_left, &mut again_right),
                None,
                frames,
            );
            same(&again_left, &whole.0, &format!("{case}, after reset, left"));
            same(
                &again_right,
                &whole.1,
                &format!("{case}, after reset, right"),
            );
        }
    }
}

/// The latency an effect declares is the delay of its signal: a voice-like
/// sweep comes out best aligned at exactly that many frames.
#[test]
fn a_declared_latency_is_the_delay_of_the_signal_through_it() {
    let frames = RATE as usize / 2;
    let input = chirp(0.3, frames);
    for entry in SHIPPED {
        let spec = aria_models::audio_effect_spec(entry.id).expect("shipped");
        let (out, _) = run(entry.id, &values(entry.id, &[]), &input, &input, None, 480);
        assert_eq!(
            lag(&input, &out, 1500),
            spec.latency as usize,
            "{}: the signal is not delayed by the declared latency",
            entry.id
        );
    }
}

// ------------------------------------------------------------- each effect --

#[test]
fn gain_scales_by_decibels_constant_or_automated() {
    let input = noise(0.5, 4800, 1);
    let (out, _) = run("gain", &[-6.0], &input, &input, None, 480);
    let scale = 10f32.powf(-6.0 / 20.0);
    for (got, x) in out.iter().zip(&input) {
        assert_eq!(*got, x * scale);
    }

    let ramp: Vec<f32> = (0..input.len())
        .map(|i| -24.0 + 30.0 * i as f32 / input.len() as f32)
        .collect();
    let (mut left, mut right) = (input.clone(), input.clone());
    let signals = [ParamSignal::PerFrame(&ramp)];
    let mut block = AudioBlock::new(&mut left, &mut right, 0, RATE, &signals, None);
    processor("gain").process(&mut block);
    for ((got, x), db) in left.iter().zip(&input).zip(&ramp) {
        assert_eq!(*got, x * 10f32.powf(db / 20.0));
    }
}

#[test]
fn the_channel_tools_route_exactly() {
    let left = noise(0.5, 1000, 11);
    let right = noise(0.5, 1000, 12);
    let mid: Vec<f32> = left
        .iter()
        .zip(&right)
        .map(|(l, r)| (l + r) * 0.5)
        .collect();
    for (id, want_left, want_right) in [
        ("swap_channels", &right, &left),
        ("fill_left", &right, &right),
        ("fill_right", &left, &left),
        ("mono", &mid, &mid),
    ] {
        let (got_left, got_right) = run(id, &[], &left, &right, None, 100);
        same(&got_left, want_left, &format!("{id} left"));
        same(&got_right, want_right, &format!("{id} right"));
    }
}

/// Steady-state gain of `id` at `values` for a tone at `freq`, in dB.
fn tone_gain(id: &str, values: &[f32], freq: f32) -> f32 {
    let frames = RATE as usize;
    let input = sine(freq, 0.25, frames);
    let (out, _) = run(id, values, &input, &input, None, 512);
    db(rms(&out[frames / 2..]) / rms(&input[frames / 2..]))
}

#[test]
fn the_equalizer_is_transparent_at_its_defaults_and_shapes_each_band() {
    let left = noise(0.5, 9600, 5);
    let right = chirp(0.5, 9600);
    let (got_left, got_right) = run(
        "parametric_eq",
        &values("parametric_eq", &[]),
        &left,
        &right,
        None,
        480,
    );
    same(&got_left, &left, "defaults, left");
    same(&got_right, &right, "defaults, right");

    let bell = values(
        "parametric_eq",
        &[
            ("band5_freq", 1000.0),
            ("band5_gain", 12.0),
            ("band5_q", 1.0),
        ],
    );
    let boosted = tone_gain("parametric_eq", &bell, 1000.0);
    assert!((boosted - 12.0).abs() < 0.1, "bell center: {boosted} dB");
    let far = tone_gain("parametric_eq", &bell, 60.0);
    assert!(far.abs() < 0.5, "bell, five octaves away: {far} dB");

    let shelf = values("parametric_eq", &[("band1_gain", -12.0)]);
    let low = tone_gain("parametric_eq", &shelf, 30.0);
    assert!((low + 12.0).abs() < 0.5, "low shelf: {low} dB");

    let cuts = values("parametric_eq", &[("low_cut", 200.0), ("high_cut", 2000.0)]);
    for (freq, below) in [(50.0, -20.0), (8000.0, -20.0)] {
        let got = tone_gain("parametric_eq", &cuts, freq);
        assert!(got < below, "{freq} Hz through the cuts: {got} dB");
    }
    let middle = tone_gain("parametric_eq", &cuts, 630.0);
    assert!(middle.abs() < 0.5, "between the cuts: {middle} dB");

    let output = values("parametric_eq", &[("output", -6.0)]);
    let got = tone_gain("parametric_eq", &output, 1000.0);
    assert!((got + 6.0).abs() < 0.01, "output: {got} dB");
}

#[test]
fn the_compressor_follows_its_curve_and_its_key() {
    let frames = RATE as usize;
    let settings = values(
        "compressor",
        &[
            ("threshold", -18.0),
            ("ratio", 4.0),
            ("knee", 0.0),
            ("attack", 1.0),
            ("release", 50.0),
        ],
    );
    // A steady level of -6.02 dBFS sits 11.98 dB over the threshold and leaves
    // a quarter of that over it: -15.005 dBFS.
    let level = vec![0.5; frames];
    let (out, _) = run("compressor", &settings, &level, &level, None, 480);
    let got = db(out[frames - 1]);
    let want = -18.0 + (db(0.5) + 18.0) / 4.0;
    assert!(
        (got - want).abs() < 0.01,
        "a steady level: {got} dBFS, not {want}"
    );

    let quiet = sine(440.0, 0.05, frames);
    let (out, _) = run("compressor", &settings, &quiet, &quiet, None, 480);
    same(&out, &quiet, "under the threshold");

    let key = vec![1.0; frames];
    let (out, _) = run(
        "compressor",
        &settings,
        &quiet,
        &quiet,
        Some((&key, &key)),
        480,
    );
    // A key at 0 dBFS is 18 dB over: 13.5 dB of reduction.
    let reduced = db(rms(&out[frames / 2..]) / rms(&quiet[frames / 2..]));
    assert!((reduced + 13.5).abs() < 0.05, "keyed: {reduced} dB");

    let makeup = values("compressor", &[("threshold", 0.0), ("makeup", 6.0)]);
    let (out, _) = run("compressor", &makeup, &quiet, &quiet, None, 480);
    let lifted = db(rms(&out) / rms(&quiet));
    assert!((lifted - 6.0).abs() < 0.01, "makeup: {lifted} dB");
}

#[test]
fn the_limiter_never_passes_its_ceiling_and_leaves_quiet_sound_alone() {
    let frames = RATE as usize;
    let mut loud = sine(110.0, 3.0, frames);
    for spike in (1000..frames).step_by(4801) {
        loud[spike] = -8.0;
    }
    for ceiling in [-1.0f32, -6.0] {
        let settings = values("limiter", &[("ceiling", ceiling), ("input", 6.0)]);
        let (out, _) = run("limiter", &settings, &loud, &loud, None, 333);
        let most = 10f32.powf(ceiling / 20.0);
        assert!(
            peak(&out) <= most,
            "ceiling {ceiling} dB: a sample left at {}",
            peak(&out)
        );
        let held = rms(&out[frames / 2..]);
        assert!(held > most * 0.5, "ceiling {ceiling} dB: limited to {held}");
    }

    let quiet = noise(0.2, 4800, 9);
    let (out, _) = run(
        "limiter",
        &values("limiter", &[]),
        &quiet,
        &quiet,
        None,
        480,
    );
    let delay = LIMITER_LATENCY;
    same(&out[..delay], &vec![0.0; delay], "the lookahead");
    same(
        &out[delay..],
        &quiet[..quiet.len() - delay],
        "quiet sound, delayed",
    );
}

const LIMITER_LATENCY: usize = dynamics::LIMITER_LOOKAHEAD as usize;

#[test]
fn the_deesser_turns_down_sibilance_and_leaves_the_voice() {
    let settings = values(
        "deesser",
        &[("frequency", 4000.0), ("threshold", -30.0), ("range", 12.0)],
    );
    // A -12 dBFS tone well over the shelf: 18 dB over the threshold asks for
    // 13.5 dB at 4:1, which the range holds to 12.
    let ess = tone_gain("deesser", &settings, 12000.0);
    assert!((ess + 12.0).abs() < 0.5, "a tone over the range: {ess} dB");
    // About 7 dB over a -20 dB threshold reduces by about 5.4 dB at 4:1.
    let softer = values(
        "deesser",
        &[("frequency", 4000.0), ("threshold", -20.0), ("range", 12.0)],
    );
    let ratio = tone_gain("deesser", &softer, 12000.0);
    assert!(
        (-6.5..-4.5).contains(&ratio),
        "a tone 7 dB over: {ratio} dB"
    );

    let voice = sine(200.0, 0.3, RATE as usize / 2);
    let (out, _) = run("deesser", &settings, &voice, &voice, None, 480);
    for (got, want) in out.iter().zip(&voice) {
        assert!((got - want).abs() < 1e-6, "{got} where {want}");
    }
}

#[test]
fn the_delay_repeats_at_its_time_and_decays_by_its_feedback() {
    let mut impulse = vec![0.0; RATE as usize];
    impulse[0] = 1.0;
    let settings = values("delay", &[("time", 250.0), ("feedback", 0.5), ("mix", 1.0)]);
    let (out, _) = run("delay", &settings, &impulse, &impulse, None, 777);
    let echo = RATE as usize / 4;
    for (at, sample) in out.iter().enumerate() {
        let want = match at {
            at if at == echo => 1.0,
            at if at == 2 * echo => 0.5,
            at if at == 3 * echo => 0.25,
            _ => 0.0,
        };
        assert_eq!(*sample, want, "frame {at}");
    }

    let dry = values("delay", &[("mix", 0.0)]);
    let input = noise(0.5, 4800, 2);
    let (out, _) = run("delay", &dry, &input, &input, None, 480);
    same(&out, &input, "mix 0");
}

#[test]
fn the_reverb_is_dry_at_no_mix_decays_and_stays_stable() {
    let input = noise(0.5, 4800, 6);
    let (out, _) = run(
        "reverb",
        &values("reverb", &[("mix", 0.0)]),
        &input,
        &input,
        None,
        480,
    );
    same(&out, &input, "mix 0");

    let mut impulse = vec![0.0; RATE as usize * 3];
    impulse[0] = 1.0;
    let wet = values("reverb", &[("mix", 1.0), ("size", 0.5)]);
    let (out, _) = run("reverb", &wet, &impulse, &impulse, None, 512);
    let second = RATE as usize;
    let early = rms(&out[..second]);
    let late = rms(&out[2 * second..]);
    assert!(early > 1e-3, "no tail: {early}");
    assert!(
        late < early * 0.1,
        "the tail does not decay: {late} after {early}"
    );

    // The longest, undamped room rings for about 12 s; five seconds on, its
    // tail is near 30 dB down, and it never grows.
    let held = values("reverb", &[("mix", 1.0), ("size", 1.0), ("damping", 0.0)]);
    let mut long = vec![0.0; RATE as usize * 7];
    long[0] = 1.0;
    let (out, _) = run("reverb", &held, &long, &long, None, 1024);
    let first = rms(&out[..second]);
    let sixth = rms(&out[6 * second..]);
    assert!(
        sixth < first * 0.1,
        "the longest room does not decay: {sixth} after {first}"
    );

    let delayed = values("reverb", &[("mix", 1.0), ("predelay", 100.0)]);
    let (out, _) = run("reverb", &delayed, &impulse, &impulse, None, 512);
    let first = out.iter().position(|s| *s != 0.0).expect("a tail");
    let earliest = RATE as usize / 10;
    assert!(
        first >= earliest,
        "the tail starts at {first}, before the pre-delay"
    );
}

#[test]
fn the_pitch_shifter_moves_a_tone_by_its_interval() {
    let frames = RATE as usize;
    let tone = sine(440.0, 0.5, frames);
    for (semitones, cents, want) in [(12.0, 0.0, 880.0), (-12.0, 0.0, 220.0), (7.0, 0.0, 659.26)] {
        let settings = values(
            "pitch_shifter",
            &[("semitones", semitones), ("cents", cents)],
        );
        let (out, _) = run("pitch_shifter", &settings, &tone, &tone, None, 480);
        let steady = &out[frames / 4..];
        let moved = power_at(steady, want);
        let stayed = power_at(steady, 440.0);
        assert!(
            moved > stayed * 20.0,
            "{semitones} semitones: {moved} at {want} Hz against {stayed} at 440 Hz"
        );
    }
    let (out, _) = run(
        "pitch_shifter",
        &values("pitch_shifter", &[]),
        &tone,
        &tone,
        None,
        480,
    );
    let delay = latency("pitch_shifter");
    same(&out[delay..], &tone[..frames - delay], "no interval");
}

#[test]
fn noise_reduction_removes_steady_noise_and_is_dry_at_no_amount() {
    let frames = RATE as usize * 2;
    let hiss = noise(0.05, frames, 13);
    let reduction = |amount: f32| {
        let (out, _) = run("noise_reduction", &[amount], &hiss, &hiss, None, 480);
        db(rms(&out[frames / 2..]) / rms(&hiss[frames / 2..]))
    };
    // RNNoise takes white hiss at -31 dBFS down by about 8 dB.
    let full = reduction(1.0);
    assert!(full < -6.0, "steady noise only reduced by {full} dB");
    let half = reduction(0.5);
    assert!(
        full < half - 1.0 && half < -1.0,
        "half the amount reduces by {half} dB against {full} dB at full"
    );

    let (out, _) = run("noise_reduction", &[0.0], &hiss, &hiss, None, 480);
    let delay = latency("noise_reduction");
    same(&out[..delay], &vec![0.0; delay], "the latency");
    same(&out[delay..], &hiss[..frames - delay], "amount 0, delayed");
}

/// The latency the catalog declares for `id`, in frames.
fn latency(id: &str) -> usize {
    aria_models::audio_effect_spec(id)
        .unwrap_or_else(|| panic!("{id} ships"))
        .latency as usize
}

/// The last half second of `id` over a stereo 1 kHz tone at `amplitude`, as
/// a level in dBFS.
fn normalized_level(settings: &[f32], amplitude: f32) -> f32 {
    let frames = RATE as usize * 10;
    let tone = sine(1000.0, amplitude, frames);
    let (out, _) = run("loudness_normalizer", settings, &tone, &tone, None, 1024);
    db(peak(&out[frames - RATE as usize / 2..]))
}

/// A stereo 1 kHz tone at -X dBFS peak measures about -X LUFS, so the output
/// level reads directly as loudness.
#[test]
fn the_loudness_normalizer_reaches_its_target_within_its_gain() {
    let target = values(
        "loudness_normalizer",
        &[("target", -16.0), ("max_gain", 12.0)],
    );
    for (input_db, want) in [(-26.0f32, -16.0f32), (-6.0, -16.0), (-36.0, -24.0)] {
        let got = normalized_level(&target, 10f32.powf(input_db / 20.0));
        assert!(
            (got - want).abs() < 0.3,
            "a tone at {input_db} dBFS leaves at {got} dBFS, not {want}"
        );
    }

    let silence_ish = sine(1000.0, 10f32.powf(-70.0 / 20.0), RATE as usize * 4);
    let (out, _) = run(
        "loudness_normalizer",
        &target,
        &silence_ish,
        &silence_ish,
        None,
        1024,
    );
    same(&out, &silence_ish, "under the gate");
}
