# Audio effects

Audio effects run as `AudioProcessor` instances on stereo `f32` audio blocks in clip or track insert chain order (`crates/aria-render/src/audio_fx.rs`). The built-in catalog is embedded from `crates/aria-models/data/audio_effects.toml` and overridden at `<data dir>/catalogs/audio_effects.toml` (see [Catalogs](../catalogs.md)).

## Examples

### Dialogue chain: Noise Reduction, Parametric EQ, De-esser, Compressor, and Limiter

1. Select a dialogue clip on an audio track (`A1`).
2. Open **Window > Effects & Presets**, expand **Audio Effects**, and double-click:
   - **Noise Reduction > Noise Reduction** (`amount = 0.85`)
   - **Filter & EQ > Parametric EQ** (`low_cut = 80.0`, `band5_freq = 2560.0`, `band5_gain = 2.0`)
   - **Filter & EQ > De-esser** (`frequency = 6000.0`, `threshold = -30.0`, `range = 12.0`)
   - **Amplitude & Compression > Compressor** (`threshold = -18.0`, `ratio = 4.0`, `attack = 10.0`, `release = 100.0`, `knee = 6.0`, `makeup = 3.0`)
   - **Amplitude & Compression > Limiter** (`ceiling = -1.0`, `release = 50.0`, `input = 0.0`)
3. Open **Window > Effect Controls > Audio** (or **Window > Audio Track Mixer** for track/bus inserts) to adjust parameters, reorder processors with the up and down chevrons, or keyframe parameters with `◆`.

### Override `audio_effects.toml` to change the default Loudness Normalizer target

Place an override file at `<data dir>/catalogs/audio_effects.toml` preserving each shipped entry's parameter slot names, `latency`, and `sidechain`:

```toml
[[entry]]
id = "loudness_normalizer"
label = "Loudness Normalizer"
category = "Amplitude & Compression"
params = [
  { name = "target", label = "Target (LUFS)", default = -14.0, min = -36.0, max = -6.0 },
  { name = "max_gain", label = "Max Gain (dB)", default = 12.0, min = 0.0, max = 24.0 },
]
```

## Where to apply and edit audio effects

- **Effects & Presets (`Window > Effects & Presets > Audio Effects`)**: expand **Amplitude & Compression**, **Channels**, **Delay & Echo**, **Filter & EQ**, **Noise Reduction**, **Reverb**, or **Time & Pitch**, and double-click an entry or drag it onto an audio or video-with-audio clip.
- **Project panel (`Window > Project > Effects > Audio`)**: click an entry to append it to the selected clip or drag it onto a timeline clip.
- **Effect Controls (`Window > Effect Controls > Audio` tab)**:
  - Click the **On** or **Bypassed** chip to bypass an audio effect instance or bring it back.
  - Reorder instances with the up and down chevrons, or remove one with the bin.
  - Drag parameter sliders or click `◆` to keyframe any parameter over clip time.
- **Audio Track Mixer (`Window > Audio Track Mixer`)**: inserts audio effects on track and bus strips, including sidechain source routing for effects with `sidechain = true` (`compressor`).

## Catalog entries (`audio_effects.toml`)

| ID | Label | Category | Latency (`48 kHz` frames) | Sidechain | Processor | Summary |
| --- | --- | --- | --- | --- | --- | --- |
| `gain` | Gain | Amplitude & Compression | `0` | `no` | `channels::gain` | Scales both channels by `10^(gain / 20)`. |
| `parametric_eq` | Parametric EQ | Filter & EQ | `0` | `no` | `eq::make` | 8-band RBJ biquad equalizer (low shelf, 6 peaking bells, high shelf) plus Butterworth low-cut and high-cut filters and output gain. |
| `compressor` | Compressor | Amplitude & Compression | `0` | `yes` | `dynamics::compressor` | Feed-forward peak compressor (input or sidechain keyed) with soft knee, attack/release ballistics, and makeup gain. |
| `limiter` | Limiter | Amplitude & Compression | `240` (`5 ms`) | `no` | `dynamics::limiter` | `240`-frame (`5 ms` at `48 kHz`) sliding-minimum lookahead brickwall peak limiter with input drive. |
| `deesser` | De-esser | Filter & EQ | `0` | `no` | `dynamics::deesser` | Dynamic high-shelf sibilance reducer (`4:1` above `threshold`, capped at `range` dB) keyed by a Butterworth high-pass detector at `frequency`. |
| `noise_reduction` | Noise Reduction | Noise Reduction | `960` (`20 ms`) | `no` | `denoise::make` | Per-channel neural speech denoiser (`nnnoiseless` / RNNoise, `480`-sample frames) blended with latency-compensated dry signal by `amount`. |
| `reverb` | Reverb | Reverb | `0` | `no` | `space::reverb` | Freeverb-tuned Schroeder/Moorer stereo reverb (8 parallel damped combs + 4 series all-passes per channel) with up to `200 ms` pre-delay. |
| `delay` | Delay | Delay & Echo | `0` | `no` | `space::delay` | Per-channel fractional-tap feedback delay line up to `2000 ms` (`4 s` buffer ceiling). |
| `pitch_shifter` | Pitch Shifter | Time & Pitch | `1024` (`21.3 ms`) | `no` | `pitch::make` | Dual-head sine-squared crossfading delay-line pitch shifter (`2048`-frame window at `48 kHz`, `1024`-frame latency). |
| `swap_channels` | Swap Channels | Channels | `0` | `no` | `channels::swap` | Swaps left and right channels (`left.swap_with_slice(right)`). |
| `fill_left` | Fill Left with Right | Channels | `0` | `no` | `channels::fill_left` | Copies the right channel buffer into the left channel (`left.copy_from_slice(right)`). |
| `fill_right` | Fill Right with Left | Channels | `0` | `no` | `channels::fill_right` | Copies the left channel buffer into the right channel (`right.copy_from_slice(left)`). |
| `mono` | Mono | Channels | `0` | `no` | `channels::mono` | Replaces both channels with `(left + right) * 0.5`. |
| `loudness_normalizer` | Loudness Normalizer | Amplitude & Compression | `0` | `no` | `loudness::make` | ITU-R BS.1770 K-weighted 3-second (`30 x 100 ms`) short-term loudness rider with `-50 LUFS` silence hold. |

### Gain (`gain`)

Multiplies left and right channels by `10^(gain / 20.0)` (`crates/aria-render/src/audio_fx/channels.rs`). When `gain` is constant `0.0 dB`, skips multiplication.

UI location: **Effects & Presets > Audio Effects > Amplitude & Compression > Gain** and **Project > Effects > Audio > Gain**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `gain` | Gain (dB) | `scalar` | dB | `-60.0` | `24.0` | `0.0` | `yes` |

### Parametric EQ (`parametric_eq`)

Runs up to 10 biquad filter stages in series per channel (`crates/aria-render/src/audio_fx/eq.rs`), updating coefficients every `32` sample frames when automated:
- **Band 1 (`band1_*`)**: RBJ low shelf (`Coefs::low_shelf`).
- **Bands 2–7 (`band2_*` .. `band7_*`)**: RBJ peaking bell filters (`Coefs::peaking`).
- **Band 8 (`band8_*`)**: RBJ high shelf (`Coefs::high_shelf`).
- **Low Cut (`low_cut`)**: Butterworth (`Q = 1 / sqrt(2)`) 2nd-order high-pass filter when `low_cut > 0.0 Hz` (bypassed at `0.0 Hz`).
- **High Cut (`high_cut`)**: Butterworth (`Q = 1 / sqrt(2)`) 2nd-order low-pass filter when `high_cut < 24000.0 Hz` and below Nyquist (bypassed at `24000.0 Hz`).
- **Output (`output`)**: post-filter linear gain `10^(output / 20.0)`. Any band with `gain == 0.0 dB` is bypassed and its filter state is cleared.

UI location: **Effects & Presets > Audio Effects > Filter & EQ > Parametric EQ** and **Project > Effects > Audio > Parametric EQ**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `band1_freq` | Low Shelf (Hz) | `scalar` | Hz | `20.0` | `20000.0` | `80.0` | `yes` |
| `band1_gain` | Low Shelf Gain (dB) | `scalar` | dB | `-24.0` | `24.0` | `0.0` | `yes` |
| `band1_q` | Low Shelf Q | `scalar` | Q | `0.1` | `18.0` | `0.71` | `yes` |
| `band2_freq` | Band 2 (Hz) | `scalar` | Hz | `20.0` | `20000.0` | `160.0` | `yes` |
| `band2_gain` | Band 2 Gain (dB) | `scalar` | dB | `-24.0` | `24.0` | `0.0` | `yes` |
| `band2_q` | Band 2 Q | `scalar` | Q | `0.1` | `18.0` | `1.0` | `yes` |
| `band3_freq` | Band 3 (Hz) | `scalar` | Hz | `20.0` | `20000.0` | `320.0` | `yes` |
| `band3_gain` | Band 3 Gain (dB) | `scalar` | dB | `-24.0` | `24.0` | `0.0` | `yes` |
| `band3_q` | Band 3 Q | `scalar` | Q | `0.1` | `18.0` | `1.0` | `yes` |
| `band4_freq` | Band 4 (Hz) | `scalar` | Hz | `20.0` | `20000.0` | `640.0` | `yes` |
| `band4_gain` | Band 4 Gain (dB) | `scalar` | dB | `-24.0` | `24.0` | `0.0` | `yes` |
| `band4_q` | Band 4 Q | `scalar` | Q | `0.1` | `18.0` | `1.0` | `yes` |
| `band5_freq` | Band 5 (Hz) | `scalar` | Hz | `20.0` | `20000.0` | `1280.0` | `yes` |
| `band5_gain` | Band 5 Gain (dB) | `scalar` | dB | `-24.0` | `24.0` | `0.0` | `yes` |
| `band5_q` | Band 5 Q | `scalar` | Q | `0.1` | `18.0` | `1.0` | `yes` |
| `band6_freq` | Band 6 (Hz) | `scalar` | Hz | `20.0` | `20000.0` | `2560.0` | `yes` |
| `band6_gain` | Band 6 Gain (dB) | `scalar` | dB | `-24.0` | `24.0` | `0.0` | `yes` |
| `band6_q` | Band 6 Q | `scalar` | Q | `0.1` | `18.0` | `1.0` | `yes` |
| `band7_freq` | Band 7 (Hz) | `scalar` | Hz | `20.0` | `20000.0` | `5120.0` | `yes` |
| `band7_gain` | Band 7 Gain (dB) | `scalar` | dB | `-24.0` | `24.0` | `0.0` | `yes` |
| `band7_q` | Band 7 Q | `scalar` | Q | `0.1` | `18.0` | `1.0` | `yes` |
| `band8_freq` | High Shelf (Hz) | `scalar` | Hz | `20.0` | `20000.0` | `10000.0` | `yes` |
| `band8_gain` | High Shelf Gain (dB) | `scalar` | dB | `-24.0` | `24.0` | `0.0` | `yes` |
| `band8_q` | High Shelf Q | `scalar` | Q | `0.1` | `18.0` | `0.71` | `yes` |
| `low_cut` | Low Cut (Hz, 0 is off) | `scalar` | Hz | `0.0` | `1000.0` | `0.0` | `yes` |
| `high_cut` | High Cut (Hz, 24000 is off) | `scalar` | Hz | `1000.0` | `24000.0` | `24000.0` | `yes` |
| `output` | Output (dB) | `scalar` | dB | `-24.0` | `24.0` | `0.0` | `yes` |

### Compressor (`compressor`)

Measures peak level across `left` and `right` (or the sidechain stereo pair when `sidechain = true` is routed), converts to decibels, evaluates soft-knee static compression (`knee` dB wide around `threshold` at `ratio`), smooths gain reduction in dB with one-pole `attack` and `release` coefficients (`exp(-1 / (ms * 0.001 * rate))`), and scales both channels by `10^((envelope + makeup) / 20)` (`crates/aria-render/src/audio_fx/dynamics.rs`).

UI location: **Effects & Presets > Audio Effects > Amplitude & Compression > Compressor** and **Project > Effects > Audio > Compressor**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `threshold` | Threshold (dB) | `scalar` | dB | `-60.0` | `0.0` | `-18.0` | `yes` |
| `ratio` | Ratio | `scalar` | ratio (`N:1`) | `1.0` | `20.0` | `4.0` | `yes` |
| `attack` | Attack (ms) | `scalar` | ms | `0.1` | `200.0` | `10.0` | `yes` |
| `release` | Release (ms) | `scalar` | ms | `5.0` | `2000.0` | `100.0` | `yes` |
| `knee` | Knee (dB) | `scalar` | dB | `0.0` | `24.0` | `6.0` | `yes` |
| `makeup` | Makeup (dB) | `scalar` | dB | `0.0` | `24.0` | `0.0` | `yes` |

### Limiter (`limiter`)

Applies input drive `10^(input / 20)`, delays the signal by `LIMITER_LOOKAHEAD = 240` frames at `48 kHz` (`5 ms`, scaled by sample rate), maintains a monotonic deque of minimum required attenuation `ceiling / peak` over the lookahead window, smooths recovery with `release`, box-averages the gain across the window so attenuation reaches the required ceiling smoothly at the peak sample, and clamps output to `±ceiling` (`crates/aria-render/src/audio_fx/dynamics.rs`).

UI location: **Effects & Presets > Audio Effects > Amplitude & Compression > Limiter** and **Project > Effects > Audio > Limiter**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `ceiling` | Ceiling (dB) | `scalar` | dB | `-24.0` | `0.0` | `-1.0` | `yes` |
| `release` | Release (ms) | `scalar` | ms | `1.0` | `1000.0` | `50.0` | `yes` |
| `input` | Input Gain (dB) | `scalar` | dB | `0.0` | `24.0` | `0.0` | `yes` |

### De-esser (`deesser`)

Runs a Butterworth 2nd-order high-pass sidechain detector at `frequency` (`0.5 ms` attack, `40.0 ms` release), computes `4:1` gain reduction above `threshold` capped to `range` dB, and updates a Butterworth high-shelf filter at `frequency` every `DEESS_CONTROL = 16` frames (`crates/aria-render/src/audio_fx/dynamics.rs`).

UI location: **Effects & Presets > Audio Effects > Filter & EQ > De-esser** and **Project > Effects > Audio > De-esser**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `frequency` | Frequency (Hz) | `scalar` | Hz | `2000.0` | `12000.0` | `6000.0` | `yes` |
| `threshold` | Threshold (dB) | `scalar` | dB | `-60.0` | `0.0` | `-30.0` | `yes` |
| `range` | Range (dB) | `scalar` | dB | `0.0` | `24.0` | `12.0` | `yes` |

### Noise Reduction (`noise_reduction`)

Processes each channel through an independent `nnnoiseless::DenoiseState` RNNoise recurrent neural network in `480`-sample frames (`latency = 960` frames / `20 ms` at `48 kHz` to buffer one full input frame and align output), then mixes the denoised `wet` frame with latency-compensated `dry` audio via `wet * amount + dry * (1.0 - amount)` (`crates/aria-render/src/audio_fx/denoise.rs`).

UI location: **Effects & Presets > Audio Effects > Noise Reduction > Noise Reduction** and **Project > Effects > Audio > Noise Reduction**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `amount` | Amount | `scalar` | normalized | `0.0` | `1.0` | `1.0` | `yes` |

### Reverb (`reverb`)

Runs a Freeverb-tuned Schroeder-Moorer stereo reverberator (`crates/aria-render/src/audio_fx/space.rs`):
- Mono pre-delay line up to `200 ms` (`predelay`), scaled by `INPUT_GAIN = 0.015`.
- 8 parallel lowpass-feedback comb filters per channel (`COMBS = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617]` at `44.1 kHz`, right channel offset by `SPREAD = 23` samples) with `feedback = size * 0.28 + 0.7` and `damp = damping * 0.4`.
- 4 series Schroeder all-pass filters per channel (`ALLPASSES = [556, 441, 341, 225]`, feedback `0.5`).
- Stereo width cross-matrix `direct = 3.0 * (width / 2.0 + 0.5)` and `cross = 3.0 * ((1.0 - width) / 2.0)`, blended with dry input by `mix`.

UI location: **Effects & Presets > Audio Effects > Reverb > Reverb** and **Project > Effects > Audio > Reverb**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `size` | Room Size | `scalar` | normalized | `0.0` | `1.0` | `0.5` | `yes` |
| `damping` | Damping | `scalar` | normalized | `0.0` | `1.0` | `0.5` | `yes` |
| `width` | Width | `scalar` | normalized | `0.0` | `1.0` | `1.0` | `yes` |
| `mix` | Mix | `scalar` | normalized | `0.0` | `1.0` | `0.25` | `yes` |
| `predelay` | Pre-delay (ms) | `scalar` | ms | `0.0` | `200.0` | `0.0` | `yes` |

### Delay (`delay`)

Maintains a per-channel circular delay line (`DELAY_SECONDS = 4` ceiling) with linear fractional-sample interpolation at `delay = time * rate / 1000.0` frames (`crates/aria-render/src/audio_fx/space.rs`). Writes `flush(sample + echo * feedback)` back into the line and outputs `sample * (1.0 - mix) + echo * mix`.

UI location: **Effects & Presets > Audio Effects > Delay & Echo > Delay** and **Project > Effects > Audio > Delay**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `time` | Time (ms) | `scalar` | ms | `1.0` | `2000.0` | `250.0` | `yes` |
| `feedback` | Feedback | `scalar` | fraction | `0.0` | `0.95` | `0.35` | `yes` |
| `mix` | Mix | `scalar` | normalized | `0.0` | `1.0` | `0.35` | `yes` |

### Pitch Shifter (`pitch_shifter`)

Sweeps two fractional-sample read heads separated by half a `2048`-frame window (`latency = 1024` frames at `48 kHz`) at pitch ratio `2^((semitones + cents / 100) / 12)` (`clamped to 0.25..=4.0`), crossfaded with complementary `sin^2(pi * phase)` and `cos^2(pi * phase)` windows (`crates/aria-render/src/audio_fx/pitch.rs`).

UI location: **Effects & Presets > Audio Effects > Time & Pitch > Pitch Shifter** and **Project > Effects > Audio > Pitch Shifter**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `semitones` | Semitones | `scalar` | semitones | `-12.0` | `12.0` | `0.0` | `yes` |
| `cents` | Cents | `scalar` | cents | `-100.0` | `100.0` | `0.0` | `yes` |

### Swap Channels (`swap_channels`)

Exchanges the left and right channel slices (`left.swap_with_slice(right)`, `crates/aria-render/src/audio_fx/channels.rs`).

UI location: **Effects & Presets > Audio Effects > Channels > Swap Channels** and **Project > Effects > Audio > Swap Channels**.

Parameters: none (`params = []`).

### Fill Left with Right (`fill_left`)

Copies the right channel slice into the left channel (`left.copy_from_slice(right)`, `crates/aria-render/src/audio_fx/channels.rs`).

UI location: **Effects & Presets > Audio Effects > Channels > Fill Left with Right** and **Project > Effects > Audio > Fill Left with Right**.

Parameters: none (`params = []`).

### Fill Right with Left (`fill_right`)

Copies the left channel slice into the right channel (`right.copy_from_slice(left)`, `crates/aria-render/src/audio_fx/channels.rs`).

UI location: **Effects & Presets > Audio Effects > Channels > Fill Right with Left** and **Project > Effects > Audio > Fill Right with Left**.

Parameters: none (`params = []`).

### Mono (`mono`)

Replaces both left and right samples in each frame with `(left + right) * 0.5` (`crates/aria-render/src/audio_fx/channels.rs`).

UI location: **Effects & Presets > Audio Effects > Channels > Mono** and **Project > Effects > Audio > Mono**.

Parameters: none (`params = []`).

### Loudness Normalizer (`loudness_normalizer`)

Applies the two-stage ITU-R BS.1770 K-weighting pre-filter (`+4.0 dB` high shelf at `1681.97 Hz`, high-pass at `38.14 Hz`), integrates mean-square power over a rolling 3-second window of thirty `100 ms` buckets (`loudness = -0.691 + 10 * log10(power)`), holds gain when loudness sits below `GATE_LUFS = -50.0 LUFS`, and slews gain toward `clamp(target - loudness, -40.0, max_gain)` with a `500 ms` downward and `2000 ms` upward time constant (`crates/aria-render/src/audio_fx/loudness.rs`).

UI location: **Effects & Presets > Audio Effects > Amplitude & Compression > Loudness Normalizer** and **Project > Effects > Audio > Loudness Normalizer**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `target` | Target (LUFS) | `scalar` | LUFS | `-36.0` | `-6.0` | `-16.0` | `yes` |
| `max_gain` | Max Gain (dB) | `scalar` | dB | `0.0` | `24.0` | `12.0` | `yes` |
