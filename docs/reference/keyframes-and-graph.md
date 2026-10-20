# Keyframes, Graph Editor, and Property Drivers Reference

Timeline property rows, keyframe icon classification, the timeline graph editor, and property drivers are defined in `crates/aria-ui/src/app/property_rows.rs`, `crates/aria-ui/src/app/property_rows_graph.rs`, `crates/aria-ui/src/panels/timeline/graph.rs`, `crates/aria-models/src/param/easing.rs`, `crates/aria-models/src/clip/driver.rs`, and `crates/aria-render/src/driver.rs`.

## Timeline property outline groups (`PropertyGroup::ALL`)

Expanding a timeline clip opens its outline mode (`ClipRevealMode::Outline`) with `PropertyGroup::Transform` expanded by default. Each group header and property row renders at height `PROPERTY_ROW_H = 22.0` px (`Theme::ROW_HEIGHT` is `28.0` px).

| Variant | Header Label | Matched `ParamSpec` / `ClipParam` | Default Expanded |
| --- | --- | --- | --- |
| `Transform` | Transform | `spec.group == "transform"` | `true` |
| `Effects` | Effects (`n`) | `ClipParam::Effect` or `spec.group` starting with `"effect-"` | `false` |
| `Masks` | Masks (`n`) | `ClipParam::Mask`, `spec.group` starting with `"mask-"`, plus path mask `TimelineMaskPathRow` entries | `false` |
| `Audio` | Audio | `spec.group == "audio"` | `false` |

## Property reveal actions (`RevealAction`)

`AppState::reveal_properties(action, shift)` reveals or toggles explicit property rows on the selected clips (or clips under the playhead on unlocked targeted tracks when no clip is selected). Holding `Shift` adds or removes the action's properties from the open set.

| Variant | Shortcut | Revealed Properties |
| --- | --- | --- |
| `Position` | `Alt+P` | `ClipParam::Position` |
| `Scale` | `Alt+S` | `ClipParam::Scale` |
| `Rotation` | `Alt+R` | `ClipParam::Rotation` |
| `Opacity` | `Alt+T` | `ClipParam::Opacity` |
| `Anchor` | `Alt+A` | `ClipParam::AnchorPoint` |
| `Effects` | `Alt+E` | All parameters in `PropertyGroup::Effects` |
| `Masks` | `Alt+M` | All parameters in `PropertyGroup::Masks` plus all path mask `Path` rows |
| `Audio` | `Alt+L` | `ClipParam::Volume` and `ClipParam::Pan` |
| `AnimatedOrModified` | `Alt+U` | First press reveals keyframed or driver-controlled properties and animated mask paths; second consecutive press reveals all modified properties |
| `Modified` | Menu command | All properties modified from `rest_state(clip)` plus all path mask `Path` rows |
| `CloseAll` | ``Alt+` `` | Closes all expanded timeline property rows |

## Keyframe icon shapes and temporal ease (`KeyframeIconShape` and `TemporalEase`)

`KeyframeIconShape::classify(index, total, easing)` (`crates/aria-ui/src/app/property_rows.rs`) selects the glyph drawn for each keyframe on a timeline property row or in the graph editor.

`TemporalEase` (`crates/aria-models/src/param/easing.rs`) stores per-side velocity `speed: f32` (value units per tick) and handle length `influence: f32` clamped to `MIN_TEMPORAL_INFLUENCE..=MAX_TEMPORAL_INFLUENCE` (`0.001..=1.0`). `TemporalEase::EASY_EASE` has `speed = 0.0` and `influence = EASY_EASE_INFLUENCE` (`1.0 / 3.0`, approximately `0.33333334`).

| Variant | Glyph | Classification Rule (`KeyframeIconShape::classify`) |
| --- | --- | --- |
| `Linear` | Diamond | `Easing::Linear` (non-roving, non-hold) |
| `Bezier` | Hourglass | `Easing::EaseInOut`, `Easing::Bezier`, or `Easing::Temporal` with both or neither side shaped |
| `EaseIn` | Incoming ease arrow | `Easing::EaseOut` (segment deceleration into keyframe) or `Easing::Temporal` with only `in_ease` shaped |
| `EaseOut` | Outgoing ease arrow | `Easing::EaseIn` (segment acceleration out of keyframe) or `Easing::Temporal` with only `out_ease` shaped |
| `Hold` | Square | `Easing::Hold` or `easing.is_hold() == true` |
| `Roving` | Circle | Interior keyframe (`0 < index && index + 1 < total`) with `easing.is_roving() == true` and not hold |

### Outgoing segment interpolation (`Easing`)

Defined in `crates/aria-models/src/param/easing.rs` (`#[serde(rename_all = "snake_case")]`).

| Variant | Serde tag | `is_hold()` | `is_bezier_like()` | Default |
| --- | --- | --- | --- | --- |
| `Linear` | `linear` | `false` | `false` | `yes` |
| `EaseIn` | `ease_in` | `false` | `true` | `no` |
| `EaseOut` | `ease_out` | `false` | `true` | `no` |
| `EaseInOut` | `ease_in_out` | `false` | `true` | `no` |
| `Hold` | `hold` | `true` | `false` | `no` |
| `Bezier` | `bezier` | `false` | `true` | `no` |
| `Temporal` | `temporal` | `hold` | `!hold && (in_ease.is_some() or out_ease.is_some())` | `no` |

## Timeline graph editor (`GraphMode`, `GraphHandleSide`, `GraphDragTarget`)

The timeline graph editor (`Shift+F3`, `crates/aria-ui/src/app/property_rows_graph.rs` and `crates/aria-ui/src/panels/timeline/graph.rs`) renders a band of height `GRAPH_EDITOR_H = 168.0` px with vertical plot inset `GRAPH_PAD_Y = 18.0` px, value-axis gutter width `AXIS_GUTTER = 40.0` px, and three horizontal grid lines at `graph_axis_ys(168.0) = [18.0, 84.0, 150.0]`.

| `GraphMode` Variant | Chip Selector | Label | Default |
| --- | --- | --- | --- |
| `Value` | `graph-mode-value` | Value | `yes` |
| `Speed` | `graph-mode-speed` | Speed | `no` |

Handle sides (`GraphHandleSide`) are `Incoming` and `Outgoing`. Interactive pointer drags (`GraphDragTarget`) target `Keyframe` (time and value), `Influence` (incoming or outgoing `TemporalEase` speed and influence), or `SpatialTangent` (2D motion-path cubic bezier tangent on `ClipParam::Position`).

### On-canvas motion path (`PathParam::ALL` and `TangentSide::ALL`)

In the Viewer (`crates/aria-ui/src/app/canvas_overlay_motion_path.rs`), when `OverlayMode::Transform` is active and the selected clip has 2 or more keyframes on a 2D spatial property (`PathParam::ALL`), the canvas draws the cubic Bezier motion path and tangent handles (`TangentSide::ALL`):

| `PathParam` Variant | `ClipParam` (`param()`) | Element ID prefix (`id()`) | Bends (`bends()`) |
| --- | --- | --- | --- |
| `AnchorPoint` | `AnchorPoint` | `anchor-point-path` | `false` |
| `Position` | `Position` | `position-path` | `true` |

| `TangentSide` Variant | `SpatialTangents` Field |
| --- | --- |
| `In` | `in_tangent` |
| `Out` | `out_tangent` |

## Property drivers (`DriverCode`, `PropertyDriver`, `DriverLanguageRegistration`)

A clip stores at most one `PropertyDriver` (`crates/aria-models/src/clip/driver.rs`) per `ClipParam`. `DriverCode` pairs a `language: String` (1 to `MAX_DRIVER_LANGUAGE = 128` ASCII bytes from `a-z`, `A-Z`, `0-9`, `.`, `-`, `_`) with `source: String` (up to `MAX_DRIVER_SOURCE = 65536` bytes).

Runtime compilation and evaluation are defined in `crates/aria-render/src/driver.rs`:
- `register_driver_language(id, language)` registers an `Arc<dyn DriverLanguage>` and returns a `DriverLanguageRegistration` registration handle; dropping the handle removes that registration and increments the language revision counter so cached programs (`PROGRAM_CACHE = 512`) recompile.
- `DEFAULT_DRIVER_BUDGET` is `20` ms (`Duration::from_millis(20)`); `set_driver_budget` clamps the per-evaluation deadline to a minimum of `1` ms. A driver that exceeds `driver_budget()`, returns a mismatched `ParamValue` variant, or panics leaves the property at its keyframed value and records a `DriverFault` (`stopped = true` on timeout).
- `InputClip` addresses either `This` (the driver's own clip) or `Named(String)` (a clip matched by title in the sequence). `InputProperty` addresses either `Id(String)` (a catalog property ID) or `Effect { effect, param }` (case-insensitive effect ID/label and parameter name/label).

| `DriverScope` Method | Return Type | Description |
| --- | --- | --- |
| `time(&self)` | `f64` | Sequence time in seconds being evaluated |
| `property(&self)` | `ClipParam` | Property replaced by the driver |
| `clip_time(&self)` | `f64` | Seconds elapsed since the first frame of the driver's clip |
| `frame_rate(&self)` | `f64` | Sequence frame rate in frames per second |
| `duration(&self)` | `f64` | Sequence duration in seconds |
| `canvas(&self)` | `[u32; 2]` | Canvas width and height in pixels |
| `value(&self)` | `ParamValue` | Keyframed value of `property()` at `time()` |
| `value_at(&self, seconds: f64)` | `ParamValue` | Keyframed value of `property()` at `seconds` |
| `key_count(&self)` | `usize` | Number of keyframes on `property()` |
| `key_time(&self, index: usize)` | `Option<f64>` | Sequence time in seconds of keyframe `index` |
| `input(&mut self, index: usize)` | `Result<ParamValue, String>` | Declared `DriverInput` `index` evaluated at `time()` |
| `input_at(&mut self, index: usize, seconds: f64)` | `Result<ParamValue, String>` | Declared `DriverInput` `index` evaluated at `seconds` |
| `clip_info(&mut self, clip: &InputClip)` | `Result<DriverClipInfo, String>` | Clip `name`, `in_point`, and `out_point` in sequence seconds |
| `seed(&self)` | `u64` | Deterministic per-clip, per-property seed for noise functions |
| `expired(&self)` | `bool` | `true` when the evaluation has exceeded `driver_budget()` |
