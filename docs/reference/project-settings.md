# Project Settings, Markers, Labels, and Tracks Reference

Every sequence in an Aria project stores a canvas configuration (`CanvasSettings`), a rational frame rate (`Rational`), an audio channel layout (`AudioLayout`), a timecode display format (`TimeDisplay`), ruler markers (`MarkerColor` and `MarkerKind`), clip and media pool labels (`LabelColor`), audio clip role tags (`AudioRole`), and typed timeline tracks (`TrackKind` and `TrackDisplay`). The model types are defined in `crates/aria-models/src/timeline_canvas.rs`, `crates/aria-models/src/label_color.rs`, `crates/aria-models/src/look.rs`, `crates/aria-models/src/track.rs`, and `crates/aria-models/src/track_catalog.rs`, with UI pickers in `crates/aria-ui/src/app/project_settings.rs`, `crates/aria-ui/src/surfaces/settings_pick.rs`, and `crates/aria-ui/src/theme.rs`.

## Canvas aspects and delivery sizes (`CanvasAspect` and `CanvasSettings`)

`CanvasSettings` (`crates/aria-models/src/timeline_canvas.rs`) holds three fields: `aspect: CanvasAspect` (default `Auto`), `background: [u8; 3]` (default `[0, 0, 0]`), and `resolution: Option<[u32; 2]>` (default `None`). Explicit canvas dimensions must be even integers within `CanvasSettings::MIN_DIM..=CanvasSettings::MAX_DIM` (`16..=8192`). When `resolution` is `None`, a fixed aspect resolves to a 1080-pixel shorter edge (`pixels_for_baseline(1080)`), and `Auto` derives its dimensions from the largest video media in the sequence.

The Settings dialog offers five delivery baselines (`CANVAS_BASELINES = [720, 1080, 1440, 2160, 4320]` in `crates/aria-ui/src/app/project_settings.rs`), scaling the shorter edge to the baseline and rounding both dimensions to even integers while filtering out dimensions that exceed `8192`.

| Variant | Wire Name | Ratio (`w:h`) | 1080p Baseline (`[w, h]`) | Default |
| --- | --- | --- | --- | --- |
| `Auto` | `auto` | `-` | `-` | `yes` |
| `Wide16x9` | `16:9` | `16:9` | `1920x1080` | `no` |
| `Tall9x16` | `9:16` | `9:16` | `1080x1920` | `no` |
| `Square1x1` | `1:1` | `1:1` | `1080x1080` | `no` |
| `Portrait4x5` | `4:5` | `4:5` | `1080x1350` | `no` |
| `Cinema21x9` | `21:9` | `21:9` | `2520x1080` | `no` |

## Sequence frame rate presets (`RATES`)

`crates/aria-ui/src/surfaces/settings_pick.rs` defines `RATES` using exact `aria_core::Rational` constants (`crates/aria-core/src/time.rs`). Changing the sequence rate via `EditCommand::SetSequenceRate` resamples timeline clip positions, fade envelopes, transitions, marker positions, and keyframe ticks to the new timebase.

| Constant | Rational (`num/den`) | Approximate FPS (`as_f64`) | Nominal FPS (`nominal_fps`) | Settings Dialog Label |
| --- | --- | --- | --- | --- |
| `FPS_23_976` | `24000/1001` | `23.976` | `24` | 23.976 fps (NTSC Cinema) |
| `FPS_24` | `24/1` | `24.000` | `24` | 24 fps (Cinema) |
| `FPS_25` | `25/1` | `25.000` | `25` | 25 fps (PAL) |
| `FPS_29_97` | `30000/1001` | `29.970` | `30` | 29.97 fps (NTSC Video) |
| `FPS_30` | `30/1` | `30.000` | `30` | 30 fps (Web / Video) |
| `FPS_50` | `50/1` | `50.000` | `50` | 50 fps (PAL High) |
| `FPS_59_94` | `60000/1001` | `59.940` | `60` | 59.94 fps (NTSC High) |
| `FPS_60` | `60/1` | `60.000` | `60` | 60 fps (Web High) |

## Audio channel layouts (`AudioLayout`)

`AudioLayout` (`crates/aria-models/src/timeline_canvas.rs`) sets the channel count for sequence playback and export via `EditCommand::SetAudioLayout`.

| Variant | Wire Name | Label | Channels (`channels()`) | Default |
| --- | --- | --- | --- | --- |
| `Mono` | `mono` | Mono (1.0) | `1` | `no` |
| `Stereo` | `stereo` | Stereo (2.0) | `2` | `yes` |
| `Surround51` | `5.1` | 5.1 Surround | `6` | `no` |

## Timecode display formats (`TimeDisplay`)

`TimeDisplay` (`crates/aria-models/src/timeline_canvas.rs`) sets how the timeline ruler, transport readouts, and Viewer clock format positions via `EditCommand::SetTimeDisplay`. `TimeDisplay::FRAMES_PER_FOOT` is `16` (35 mm 4-perf film).

| Variant | Wire Name | Label | Default | Summary |
| --- | --- | --- | --- | --- |
| `Timecode` | `timecode` | Timecode | `yes` | SMPTE timecode (`HH:MM:SS:FF`, or `HH:MM:SS;FF` at NTSC rates). |
| `Frames` | `frames` | Frames | `no` | Zero-based integer frame index from sequence start. |
| `FeetFrames` | `feet_frames` | Feet + Frames 35mm | `no` | 35 mm 4-perf film feet and frames at `FRAMES_PER_FOOT = 16` (`FF+FRM`). |

## Marker colors and kinds (`MarkerColor` and `MarkerKind`)

Markers on the sequence ruler or on clips carry a `MarkerColor` (`MarkerColor::ALL` in `crates/aria-models/src/timeline_canvas.rs`) and a `MarkerKind` (`MarkerKind::ALL`). When a marker is created without an explicit color, `MarkerColor::cycle(index)` selects `MarkerColor::ALL[index % 8]`. In `aria-ui`, `Theme::marker` (`crates/aria-ui/src/theme.rs`) converts each `MarkerColor` to HSLA with fixed saturation `0.68`, lightness `0.62` in Dark appearance and `0.46` in Light appearance, and per-color hue.

| Variant | Wire Name | RGBA (`rgba()`) | UI Hue (`Theme::marker`) | Label Alias (`LabelColor::from`) |
| --- | --- | --- | --- | --- |
| `Teal` | `teal` | `[0, 229, 199, 255]` | `0.48` | `Teal` |
| `Blue` | `blue` | `[74, 158, 245, 255]` | `0.58` | `Blue` |
| `Purple` | `purple` | `[167, 123, 245, 255]` | `0.75` | `Purple` |
| `Pink` | `pink` | `[245, 111, 192, 255]` | `0.90` | `Magenta` |
| `Red` | `red` | `[245, 90, 90, 255]` | `0.99` | `Rose` |
| `Orange` | `orange` | `[245, 154, 60, 255]` | `0.07` | `Mango` |
| `Yellow` | `yellow` | `[240, 208, 74, 255]` | `0.14` | `Yellow` |
| `Green` | `green` | `[111, 216, 94, 255]` | `0.34` | `Green` |

### Marker categories (`MarkerKind`)

| Variant | Wire Name | Label | Cycles To (`cycle_next()`) | Default |
| --- | --- | --- | --- | --- |
| `Comment` | `comment` | Comment | `Chapter` | `yes` |
| `Chapter` | `chapter` | Chapter | `Segmentation` | `no` |
| `Segmentation` | `segmentation` | Segmentation | `Comment` | `no` |

## Clip and media label colors (`LabelColor`)

`LabelColor` (`crates/aria-models/src/label_color.rs`) defines the 16-color Premiere Pro-compatible palette used for timeline clips and media pool entries. Selected clips render with `selected_rgb()`, computed per channel as `c + ((255 - u16::from(c)) / 3) as u8`. Legacy marker color names deserialize through aliases (`red` -> `Rose`, `orange` -> `Mango`, `pink` -> `Magenta`).

Default label constants by clip type:
- `LabelColor::VIDEO = LabelColor::Teal`
- `LabelColor::AUDIO = LabelColor::Forest`
- `LabelColor::GENERATOR = LabelColor::Mango`
- `LabelColor::NESTED = LabelColor::Tan`
- `LabelColor::ADJUSTMENT = LabelColor::Yellow`

| Variant | Wire ID | Label | Fill RGB (`rgb()`) | Selected RGB (`selected_rgb()`) | Serde Alias |
| --- | --- | --- | --- | --- | --- |
| `Violet` | `violet` | Violet | `[165, 140, 222]` | `[195, 178, 233]` | `-` |
| `Iris` | `iris` | Iris | `[113, 128, 221]` | `[160, 170, 232]` | `-` |
| `Caribbean` | `caribbean` | Caribbean | `[29, 194, 151]` | `[104, 214, 185]` | `-` |
| `Lavender` | `lavender` | Lavender | `[216, 143, 219]` | `[229, 180, 231]` | `-` |
| `Cerulean` | `cerulean` | Cerulean | `[27, 162, 228]` | `[103, 193, 237]` | `-` |
| `Forest` | `forest` | Forest | `[65, 143, 55]` | `[128, 180, 121]` | `-` |
| `Rose` | `rose` | Rose | `[241, 74, 117]` | `[245, 134, 163]` | `red` |
| `Mango` | `mango` | Mango | `[241, 158, 31]` | `[245, 190, 105]` | `orange` |
| `Purple` | `purple` | Purple | `[152, 57, 166]` | `[186, 123, 195]` | `-` |
| `Blue` | `blue` | Blue | `[46, 82, 214]` | `[115, 139, 227]` | `-` |
| `Teal` | `teal` | Teal | `[25, 145, 145]` | `[101, 181, 181]` | `-` |
| `Magenta` | `magenta` | Magenta | `[227, 59, 175]` | `[236, 124, 201]` | `pink` |
| `Tan` | `tan` | Tan | `[201, 176, 134]` | `[219, 202, 174]` | `-` |
| `Green` | `green` | Green | `[60, 168, 60]` | `[125, 197, 125]` | `-` |
| `Brown` | `brown` | Brown | `[139, 82, 32]` | `[177, 139, 106]` | `-` |
| `Yellow` | `yellow` | Yellow | `[227, 211, 42]` | `[236, 225, 113]` | `-` |

## Audio clip roles (`AudioRole`)

`AudioRole` (`crates/aria-models/src/look.rs`) tags audio-lane clips for timeline badges and role-based filtering.

| Variant | Wire ID | Label |
| --- | --- | --- |
| `Music` | `music` | Music |
| `Sfx` | `sfx` | Sound FX |
| `Voiceover` | `voiceover` | Voiceover |
| `Extracted` | `extracted` | Extracted |

## Timeline track kinds and display heights (`TrackKind` and `TrackDisplay`)

`crates/aria-models/src/track_catalog.rs` defines the capability table (`SHIPPED`, returned by `track_kind_specs()`) for every `TrackKind` variant (`crates/aria-models/src/track.rs`). Each lane sits in a `LaneZone` ordered from bottom to top by `LaneZone::rank()`:
- `Bus` (`rank = 0`)
- `Floor` (`rank = 1`)
- `Main` (`rank = 2`, selected when a `Video` track has `main = true`)
- `Stack` (`rank = 3`)
- `Ceiling` (`rank = 4`)

| Variant | Wire ID | Label | Default Name | Audio | Visual | Media | Transitions | Main | Carries Audio | Extracts Audio | Bus | Agent Creates | Zone | Default Height (px) | Bake Medium |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `Video` | `video` | Overlay | `V1` | `false` | `true` | `true` | `true` | `true` | `true` | `true` | `false` | `true` | `Stack` | `48` | `Picture` |
| `Audio` | `audio` | Audio | `A1` | `true` | `false` | `true` | `false` | `false` | `true` | `false` | `false` | `true` | `Floor` | `48` | `Sound` |
| `Text` | `text` | Text | `T1` | `false` | `true` | `false` | `true` | `false` | `false` | `false` | `false` | `true` | `Ceiling` | `48` | `Picture` |
| `Sticker` | `sticker` | Sticker | `S1` | `false` | `true` | `false` | `true` | `false` | `false` | `false` | `false` | `true` | `Stack` | `48` | `Picture` |
| `Effect` | `effect` | Effects | `E1` | `false` | `true` | `false` | `false` | `false` | `false` | `false` | `false` | `false` | `Stack` | `48` | `Picture` |
| `Filter` | `filter` | Filters | `F1` | `false` | `true` | `false` | `false` | `false` | `false` | `false` | `false` | `false` | `Stack` | `48` | `Picture` |
| `Adjustment` | `adjustment` | Adjust | `FX1` | `false` | `true` | `false` | `false` | `false` | `false` | `false` | `false` | `false` | `Stack` | `48` | `Picture` |
| `Submix` | `submix` | Submix | `Bus 1` | `true` | `false` | `false` | `false` | `false` | `false` | `false` | `true` | `true` | `Bus` | `28` | `Error` |

### Track display heights (`TrackDisplay`)

`TrackDisplay` (`crates/aria-models/src/track.rs`) stores per-track view state: `height: u32` (default `DEFAULT_HEIGHT = 48`, bus default `BUS_HEIGHT = 28`, clamped to `HEIGHT = 20..=200`), `waveform: bool` (default `true`), `filmstrip: bool` (default `true`), and `collapsed: bool` (default `false`).

| Preset Label | Height (px) |
| --- | --- |
| Mini | `20` |
| Small | `32` |
| Medium | `48` |
| Large | `72` |
