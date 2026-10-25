# Audio analyzers

An analyzer inspects mixed sound over a time range and returns findings: beat
positions, markers, labeled ranges, and a loudness measurement. The editor
runs it as a background job and applies the findings as one undo step named
after the analyzer.

```rust
use std::ops::ControlFlow;

use aria_plugin::{
    AnalyzerResult, AnalyzerRun, AnalyzerScope, AnalyzerSpec, PluginContext, PluginError,
};

struct Peaks {
    channels: usize,
    loudest: (i64, f32),
}

impl AnalyzerRun for Peaks {
    fn block(&mut self, first_frame: i64, samples: &[f32]) -> ControlFlow<()> {
        for (offset, frame) in samples.chunks_exact(self.channels).enumerate() {
            let level = frame.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
            if level > self.loudest.1 {
                self.loudest = (first_frame + offset as i64, level);
            }
        }
        ControlFlow::Continue(())
    }

    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
        Ok(AnalyzerResult {
            beats: vec![self.loudest.0],
            ..AnalyzerResult::default()
        })
    }
}

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    let spec = AnalyzerSpec::new("example.peak", "Find Loudest Peak", AnalyzerScope::Clip);
    ctx.register_analyzer(spec, |_rate: u32, channels: u16| -> Box<dyn AnalyzerRun> {
        Box::new(Peaks { channels: usize::from(channels), loudest: (0, 0.0) })
    })?;
    Ok(())
}
```

## Registration

`PluginContext::register_analyzer(spec, analyzer)` adds one analyzer. The
registration is held on the terms of `register_effect`: it lasts while the
plugin is mounted and ends when the returned `Registration` drops, the plugin
unmounts, or the plugin fails.

`AnalyzerSpec` fields:

| Field | Meaning |
| --- | --- |
| `id` | Namespaced identifier, such as `com.example.tempo`. ASCII letters, digits, `.`, `_` and `-`; at least one `.`. |
| `label` | Menu row, palette row, job title, and undo step name. Must not be blank. |
| `category` | Optional grouping. |
| `scope` | `Clip`, `Sequence`, or `InOut`. |

`AnalyzerSpec::new(id, label, scope)` builds a spec with no category, and
`with_category(category)` sets it.

Registration fails with `PluginError::AnalyzerRefused` for an empty,
unnamespaced, or malformed id, an id already registered on the context, or a
blank label.

## Scope and menus

| Scope | Menu | Audio analyzed |
| --- | --- | --- |
| `Clip` | Clip > Analyze | The selected clip's sound. The row is disabled with `Select a clip with sound` when no selected clip has sound. |
| `Sequence` | Sequence > Analyze | The whole sequence mix. |
| `InOut` | Sequence > Analyze | The sequence mix from In to Out, or the whole sequence when no range is marked. |

Each registered analyzer also appears in the command palette. Unmounting the
plugin removes its rows.

## Running

`Analyzer::begin(rate, channels)` starts one run. The host mixes the requested
source through `AudioHost::mix` at `EXPORT_AUDIO_RATE` (48 kHz) and calls
`AnalyzerRun::block(first_frame, samples)` once per block with interleaved
samples. `first_frame` counts sample frames from the start of the sequence.
Returning `ControlFlow::Break(())` ends the mix early and proceeds to
`finish`.

The run checks for cancellation before and after every block. The job is
titled "Analyzing: <label>" and reports progress as mixed frames over total
frames; on completion its result is the summary of the findings.

`PluginContext::run_analyzer` and `run_analyzer_with_progress` run a
registered analyzer outside the editor, for tools and tests.

A panic in `begin`, `block`, or `finish` is caught. The plugin is failed for
the session, its registrations are dropped, and the project is unchanged. An
`Err` from `finish` fails the job with that message, records a plugin fault,
and leaves the plugin mounted.

## Results

`AnalyzerResult` positions are sample frames at the mix rate. Applying a result
converts markers and ranges to ticks at the sequence rate and beats to the
clip's source ticks, so a beat grid follows the clip through trims and
retimes.

| Field | Applied as |
| --- | --- |
| `beats` | The clip's beat grid (`SetClipBeats`), clip scope only. |
| `markers` | Markers attached to the clip for clip scope; sequence markers otherwise. `duration_frames` greater than 0 sets the marker duration, clamped to the clip or sequence end. |
| `ranges` | Yellow segmentation markers with duration, named `label`, or "Silence" when `label` is empty. |
| `loudness` | Integrated loudness (LUFS), true peak (dBFS), and loudness range (LU), shown in the job result and on the status line. No undo step. |

`PluginContext::apply_analyzer` applies a result as one undo step named after
the analyzer label. `apply_analyzer_result` does the same with an explicit
label.

## Shipped analyzers

| Id | Label | Scope |
| --- | --- | --- |
| `aria.beats` | Detect Beats | Clip |
| `aria.silence` | Detect Silence | Sequence |
| `aria.loudness` | Measure Loudness | In to Out |

They run through the same contribution point as plugin analyzers.

## Native and WebAssembly

A native plugin built with `aria-sdk` calls `Host::register_analyzer(spec,
analyzer)`. `Analyzer::open(host, sample_rate, channels)` returns an
`AnalyzerRun`; the host calls `push(samples)` per block and `finish()` once.
`Host::register_analyzer_spec(&spec)` registers the spec with no `Analyzer`
object, and the host opens each run through
`Plugin::analyzer_open(&self, host, id, sample_rate, channels)`.

`finish` returns `aria_sdk::AnalyzerResult` with the fields under
[Results](#results). Its `markers` are `AnalyzerMarker` values (`frame`,
`duration_frames`, `name`, and `color`, a `MarkerColor`), its `ranges` are
`AnalyzerRange` values (`start`, the first frame; `end`, the frame after the
span; `label`), and its `loudness` is an `AnalyzerLoudness`
(`integrated_lufs`, `true_peak_db`, and `range_lu`).

A WebAssembly component imports `register-analyzer(spec)` and exports
`analyzer-open(id, sample-rate, channels)`, `analyzer-push(instance,
samples)`, `analyzer-finish(instance)` returning the result as JSON, and
`analyzer-drop(instance)`. Fuel and memory limits apply to every call.
