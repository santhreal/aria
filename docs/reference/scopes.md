# Video Scopes Reference

Reference for the waveform monitor, vectorscope, and RGB histogram measured by `aria-analysis` (`crates/aria-analysis/src/scopes.rs`), accelerated on the GPU by `aria-compositor` (`crates/aria-compositor/src/scopes.rs`), and rendered into scope textures by `aria-ui` (`crates/aria-ui/src/scopes.rs` and `crates/aria-ui/src/panels/preview/scopes.rs`).

## Waveform modes and traces (`WaveformMode` and `WaveformTrace`)

Defined in `crates/aria-ui/src/scopes.rs` (`WaveformMode::ALL`, `#[serde(rename_all = "camelCase")]`) and `crates/aria-analysis/src/scopes.rs` (`WaveformTrace::ALL`).

| Variant | Serde name | Label | Short label | Chip ID | View menu ID | Trace (`WaveformTrace`) | Counted channels (`WaveformChannel`) | Default |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `Luma` | `luma` | Luma | Luma | `waveform-luma` | `view-waveform-luma` | `Luma` | `Luma` | `yes` |
| `RgbParade` | `rgbParade` | RGB Parade | RGB | `waveform-rgb` | `view-waveform-rgb` | `RgbParade` | `Red, Green, Blue` | `no` |
| `YcbcrParade` | `ycbcrParade` | YCbCr Parade | YCbCr | `waveform-ycbcr` | `view-waveform-ycbcr` | `YcbcrParade` | `Luma, Cb, Cr` | `no` |

## Waveform channels (`WaveformChannel`)

Defined in `crates/aria-analysis/src/scopes.rs` (`WaveformChannel`) and `crates/aria-ui/src/scopes.rs` (`channel_colour`). BT.709 luma (`luma709`) uses 16-bit fixed-point weights `13933 / 46871 / 4732` over `65536` (`0.2126 / 0.7152 / 0.0722`). BT.709 chroma (`chroma_codes709`) uses 15-bit fixed-point offsets with neutral gray at code value `128`.

| Variant | Trace RGB (`[u8; 3]`) | Gray (`128, 128, 128`) code | Measured quantity |
| --- | --- | --- | --- |
| `Luma` | `[120, 235, 170]` | `128` | BT.709 luma code value (`luma709`) |
| `Red` | `[235, 80, 80]` | `128` | 8-bit red code value |
| `Green` | `[80, 235, 110]` | `128` | 8-bit green code value |
| `Blue` | `[90, 130, 245]` | `128` | 8-bit blue code value |
| `Cb` | `[90, 130, 245]` | `128` | BT.709 blue-difference chroma code value (`chroma_codes709.0`) |
| `Cr` | `[235, 80, 80]` | `128` | BT.709 red-difference chroma code value (`chroma_codes709.1`) |

## Scope configuration and bounds (`ScopeConfig`)

Defined in `crates/aria-analysis/src/scopes.rs`. Fully transparent pixels (`alpha == 0`) are skipped during sampling.

| Constant or default field | Value | Meaning |
| --- | --- | --- |
| `SCOPE_BINS` | `256` | Code-value bins per histogram and waveform channel |
| `MAX_WAVEFORM_COLUMNS` | `1024` | Maximum waveform horizontal column count accepted by `ScopeConfig::new` |
| `MAX_VECTORSCOPE_SIZE` | `512` | Maximum square vectorscope grid edge accepted by `ScopeConfig::new` |
| `MAX_SCOPE_SAMPLES` | `4000000` | Maximum sampled pixels per frame accepted by `ScopeConfig::new` |
| `ScopeConfig::default().waveform_columns()` | `256` | Default waveform horizontal column count |
| `ScopeConfig::default().vectorscope_size()` | `128` | Default square vectorscope grid edge |
| `ScopeConfig::default().maximum_samples()` | `200000` | Default pixel sampling ceiling per frame |
| `ScopeConfig::default().waveform_trace()` | `Luma` | Default waveform trace mode |

## Scope bitmap dimensions and vectorscope graticule (`aria_ui::scopes`)

Defined in `crates/aria-ui/src/scopes.rs`. Drawn cell intensity scales by `(count / peak).sqrt()`.

### Bitmap and graticule constants

| Constant | Value | Meaning |
| --- | --- | --- |
| `SCOPE_PX` | `192` | Edge length in pixels of the vectorscope bitmap and height of the waveform bitmap |
| `HISTOGRAM_W` | `256` | Width in pixels of the histogram bitmap and single-trace waveform bitmap |
| `HISTOGRAM_H` | `96` | Height in pixels of the histogram bitmap |
| `SKIN_TONE_DEGREES` | `123.0` | Vectorscope skin-tone line angle in degrees counter-clockwise from the positive Cb axis |
| `TARGET_LEVELS` | `191, 255` | Vectorscope color target code levels (75% and 100%) |

### Vectorscope color targets (`TARGET_COLOURS`)

Defined in `crates/aria-ui/src/scopes.rs` (`TARGET_COLOURS`). Each primary and secondary color has a graticule target box at both `TARGET_LEVELS` (`191` for 75% saturation, `255` for 100% saturation).

| Index | Target name | Full-scale RGB (`[u8; 3]`) | 75% cell on 128x128 grid (`(x, y)`) | 100% cell on 128x128 grid (`(x, y)`) |
| --- | --- | --- | --- | --- |
| `0` | Red | `[255, 0, 0]` | `(53, 16)` | `(49, 0)` |
| `1` | Yellow | `[255, 255, 0]` | `(16, 59)` | `(0, 58)` |
| `2` | Green | `[0, 255, 0]` | `(27, 107)` | `(15, 121)` |
| `3` | Cyan | `[0, 255, 255]` | `(74, 111)` | `(78, 127)` |
| `4` | Blue | `[0, 0, 255]` | `(111, 68)` | `(127, 69)` |
| `5` | Magenta | `[255, 0, 255]` | `(100, 20)` | `(112, 6)` |
