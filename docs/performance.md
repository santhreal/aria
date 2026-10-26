# Performance

Measured numbers and the commands that produce them. Every figure here comes
from a bench or test in the repository. Release targets are in
[spec/performance.md](spec/performance.md).

## Benches

| Command | What it measures |
| --- | --- |
| `cargo bench -p aria-ui --bench ui_hot_paths` | Projection, magnet index build, magnet query |
| `cargo bench -p aria-models --bench param_sample` | Parameter and transform sampling |
| `cargo bench -p aria-models --bench project_persist` | Representative project save/load time and persistence allocation counts |
| `cargo bench -p aria-render --bench text_anim` | Text animation resolve |
| `cargo bench -p aria-render --bench resolve` | Production scene resolution |
| `cargo bench -p aria-render --bench export_audio_mix` | Production export audio mixing |
| `cargo bench -p aria-decoder --bench seek_latency` | Scrub seek, cold open and warm re-seek |
| `cargo bench -p aria-compositor --bench blend_pipeline` | Specialized blend pipeline throughput |
| `cargo bench -p aria-engine --bench command_apply` | One edit's cost on a 480-clip project |

## The editor's per-frame CPU work

`ui_hot_paths` measures the three costs an edit pays before the GPU is
involved, on the workload in `aria_ui::workload::standard()`: 8 tracks, 480
clips, 3840 beats, 15 markers, at 24 fps.

| Bench | Time | What it is |
| --- | --- | --- |
| `projection` | 407 µs | The project turned into the view model, once per adopted edit |
| `snap_index` | 70 µs | The magnet set built from that projection |
| `snap_query` | 46 ns | One magnet lookup, paid per pointer move during a drag |

Recorded on a Ryzen 9 9950X, Linux, release profile. Treat them as a shape, not
a target: a projection in the hundreds of microseconds is a frame that does not
notice it, and a projection in the tens of milliseconds is a drag that stutters.

A drag is the query, not the build: 46 ns per lookup is why the magnet set is
built once per projection and binary-searched, instead of being rebuilt from
every clip edge and beat on each mouse move.

## Playback against the sequence's rate

A frame that takes longer than its share of a second is a frame playback cannot
deliver on time. `aria_render::PlaybackMeter` counts those: give it the target
rate and each rendered frame's duration, and it reports the render rate, how
many renders were over budget, and the worst one. During playback the editor
also hands it each frame shown and each frame dropped with its cause, and the
report adds the achieved rate (frames shown over the run's time) and the drops.
The Info panel's Playback section reads the same report, so the number on
screen and the number the scenario checks come from one counter.

`cargo test -p aria-render playback_rate_report` renders 120 frames of a solid
generator at 640x360, 24 fps, through the preview entry point, excluding four
warm-up frames because the first render of a session pays for pipeline and
decoder setup. On a GPU it must hold 24 fps with at most 5% of frames late. On a
CPU rasterizer the floor drops to a tenth of the target: `lavapipe` is an order
of magnitude off a GPU, and a test that failed there would only teach everyone
to ignore it. The report prints either way.

Measured on a Ryzen 9 9950X with an RTX 5090, debug profile: **5517.2 fps
against a 24 fps target, 0 of 116 frames late, worst frame 1.3 ms**. A solid
generator is the whole sequence, so that number is the pipeline's ceiling for a
640x360 preview and not the speed of any codec; decode cost is measured
separately below.

## Scrub seek

`cargo bench -p aria-decoder --bench seek_latency` measures the two seeks a
scrub makes, over `tests/fixtures/fixture-640x360-24fps-3s.mp4`:

- `cold_open_and_seek` opens the decoder at a mid-clip frame and decodes it,
  as the renderer opens a clip's decoder: what clicking into the middle of an
  unopened clip costs, demuxer setup included.
- `warm_scrub` holds one open decoder and requests a backward jump, a one-frame
  step, and a long forward jump: the three gestures the preview issues once a
  clip is open.

| Bench | Time | What it is |
| --- | --- | --- |
| `cold_open_and_seek` | 6.24 ms | Opening the file at a mid-clip frame and decoding it |
| `warm_scrub` | 8.06 ms | Four requests through an already open decoder |

Recorded on Server (Intel Core i7-11700K), Linux, release profile. A warm
request averages 2.0 ms, a third of a cold open. The preview holds one decoder
per clip.

A host with no native decoder prints why and measures nothing.

## Time to the first interactive frame

`cargo test -p aria-desktop first_interactive_frame` measures the two phases the
process runs before its window opens: mounting the plugins, which reads every
catalog and registers every panel, command and key, and building the session,
which is the engine and the first projection a frame reads. Cold is an empty
profile directory; warm is the same directory after a session has written its
settings and a project into it.

The boot cover ends on readiness, so these are the numbers that determine whether
the boot cover is ever shown. `aria_ui::shell::SPLASH_MAX` (2 s) is the ceiling the
test asserts against: a start past it is a start the cover's timeout ended.

| Phase | Cold | Warm |
| --- | --- | --- |
| Plugin mount | 222 ms | 150 ms |
| Session and first projection | 35 µs | 46 µs |

Recorded on a Ryzen 9 9950X with an RTX 5090, Linux, debug profile. The session
is microseconds because the engine is mounted, not built here; the mount is the
start.

The test prints the dearest mounts, which is what makes the number actionable:
`aria.render` (bringing up the GPU device), `aria.engine` (the pipelines built on
it, and the first parse of the installed fonts), and nothing else above a few
milliseconds. On `santhserver` (RTX 3080 Ti) a cold mount spends 112–126 ms in
`aria.render`, 114–119 ms in `aria.engine`, and about 2 ms in `aria.storage`.
Every plugin's own mount cost is also in Settings, next to the plugin.

Before this breakdown the process created three GPU devices (one in the
compositor plugin, one in the render plugin, one in the engine), each with its
own driver instance, adapter, and pipeline set; one was rendered with and the
other two had no reader. One device is created now and shared with
`GpuContext::share`, so a cold start fell from **583 ms to 222 ms** and the
process holds a third of the video memory it used to.

Headless engines follow the same device rule. A process keeps one headless
device on Vulkan, Metal and D3D12; the GL fallback keeps a context per thread.
Text renderers also parse the installed font catalog once, while shaping,
raster and custom-font state remain isolated per renderer. On `santhserver`
(RTX 3080 Ti), the 1,535-test UI library suite completes under its default
parallelism in 127.53 s.

## Effect passes per frame

`cargo test -p aria-compositor pass_batching` measures what an effect chain
costs to dispatch. An effect pass is three things: the pipeline it draws with,
the texture it reads, and the constants it reads them with. Layers agreeing on
all three issue the same draw, so one bind group and one uniform buffer serve
all of them. The binding is shared and the draw is not, which is why a grade on
eight clips still grades each clip's own pixels.

Every pass used to build both and drop them at submit. Twelve layers with two
effects each is 24 passes per frame, so 24 uniform buffers and 24 bind groups
per frame, for descriptors that were usually identical to the frame before.

| 12 layers × 2 effects, 512×288 | Per frame (median) |
| --- | --- |
| A bind group per pass | 0.78 ms |
| Shared by descriptor | 0.57 ms |

Recorded on a Ryzen 9 9950X with an RTX 5090, Linux, debug profile, as the
median of 15 rounds that alternate the two modes one frame at a time: a frame
this size is readback-bound, and a block of frames per mode measures whatever
else the machine was doing during that block. 24 allocations per frame become 0
once the descriptors repeat. The counts are the test's contract; the clock is
reported. The same numbers are shown in the Info panel's Last frame section and
by Last Frame Cost on the status line, so a frame that is dear because of a
stacked chain shows the cause.

The table is bounded (128 entries) and keyed on the pool's own views, so it is
built and dropped with the pool that holds them: resizing the canvas cannot
leave an entry naming a texture that is gone.

## What one edit costs

`cargo bench -p aria-engine --bench command_apply` applies commands to a project
of 8 tracks and 480 clips. Applying an edit mutates the project in place and
records the inverse action, so the cost is the edit's, not the project's: the
same transform on a project of one clip is the comparison.

| Command, 8 tracks × 60 clips | Per apply |
| --- | --- |
| Set a clip's transform | 216 ns |
| Set a clip's transform, project of 1 clip | 183 ns |
| Set a transform, then undo and redo it | 493 ns |
| Split a clip, then undo the split | 686 ns |
| Move a clip to another track and back | 764 ns |

Recorded on a Ryzen 9 9950X, Linux, release profile. The 33 ns between the two
transform rows is the lookup of one clip among 480; a project the edit had to
copy would separate them by the clip count, not by a lookup.

A project is copied per export and per assistant turn, where a snapshot is what
is wanted: an export renders the timeline as it was when the job started, and an
assistant turn rolls back to the state it began from. Neither is per command.

## The frame budget test

`cargo test -p aria-ui --test frame_budget` asserts the same work stays inside
a ceiling and grows with the clip count rather than faster. The ceilings are
set for a tenfold regression, not for these numbers: a machine several times
slower than the one above still passes. The test is a binary of its own and
runs one measurement at a time, so no other test of the suite runs beside it.

`cargo test -p aria-ui --lib magnet_index` asserts the magnet index holds the
magnets the projection holds and no others, and that every `SnapKind` has
something that produces it, so a magnet nobody builds fails the suite instead
of shipping.
