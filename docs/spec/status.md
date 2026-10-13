# Status

The current build compared with each chapter. A row is removed when the
chapter's behaviour ships with its tests; this page is updated in the same
change.

## Plugins ([plugins.md](plugins.md))

| Area | Current build |
| --- | --- |
| Host | `aria_plugin::PluginContext`: a typed service registry (one value per type) and an event bus. Manifests are `&'static` Rust structs. |
| Effects and transitions | A plugin registers an effect or transition as a catalog spec plus WGSL fragment passes (`PluginContext::register_effect`, `register_transition`; `crates/aria-compositor/src/registered_passes.rs`). Parameters are scalar, vec2, and colour, and reach the shader through a uniform capped at 256 floats. An effect runs at most 8 passes, each reading the previous pass and the effect's input at the frame size. No compute passes, layer references, plugin textures, output sizes, bounds changes, CPU path, parameter groups, or transition parameters. The `stateful` flag and frame cache keys wait for a frame cache. `EffectService::render_effect` and `TransitionService::render_transition` render on the device registered by `aria.render` (`crates/aria-ui/src/plugins/gpu.rs`). |
| Frames | `aria_plugin::RenderHost` renders the open sequence, the root or a nested sequence, or one clip, at a time and a size, as a straight-alpha image, one image per tick of a range, or into an `Rgba8Unorm` texture with no CPU copy (`crates/aria-plugin/src/render_host.rs`). Frames render one at a time on a renderer separate from the preview; no pipelined range render, and no render of a time range to a file. |
| Generators | A plugin generator is a registered effect whose spec sets `generator: true`. It is placed as a transparent solid clip with the effect first in its chain (`crates/aria-ui/src/app/media_import.rs`). A clip whose generator is no longer registered draws nothing rather than a slate, and shows no missing-plugin badge. |
| Events | Every event in the chapter's table is dispatched (`docs/plugin-development.md#events`). `CommandApplied` holds the command, its `ApplyOutcome`, the engine revision, and the origin, with no undo step and no list of changed ids. `ResolveDroppedPath` claims paths, not URLs. |
| Project access | `aria_plugin::ProjectHost`: immutable snapshots, single commands, named transactions that return each command's outcome, nested groups with savepoints, and undo, for native plugins. The selection, playhead, and In/Out are read from the `EditorView` service as of the last drawn frame, not from the snapshot. No query by time range or selection. |
| UI contributions | Panels, overlays, commands, menus, keys, and settings exist through hosts in `crates/aria-ui/src/plugins/`, which requires linking `aria-ui`. |
| External plugins | `cdylib` loaded from `<data dir>/plugins`, refused unless built by the same `rustc` and `aria-plugin` (`crates/aria-plugin/src/external.rs`, `ABI_VERSION = 6`). Libraries are never unloaded. No WASM tier, packages, permissions, activation events, or hot reload. |

## Layout ([layout.md](layout.md))

Fixed panes: sidebar, inspector, preview, timeline, and an optional scopes
strip. A workspace is a set of pane sizes and visibility
(`crates/aria-ui/src/layouts.rs`). There is no tab docking, splitting,
floating, or frame maximize.

## Look ([look.md](look.md))

Dark theme from `docs/theme-tokens.md`. No light theme.

## Viewer ([viewer.md](viewer.md))

One preview with a source mode for the loaded pool item, safe-margin guides,
and scopes. No Source tabs, split view, or rulers.

## Timeline ([timeline.md](timeline.md))

One element per clip, built every frame (`crates/aria-ui/src/panels/timeline/`).
Double-click opens a keyframe strip under one clip. No property rows per
property, no graph editor, no render bar.

## Editing ([editing.md](editing.md))

The command set in `crates/aria-commands/src/command.rs` covers moves, trims,
ripple, roll, slip, slide, rate, split, lift, extract, insert, overwrite,
nesting, multicam, sync lock, markers, and keyframes. Missing: Track Select
tools, edge selection with keyboard trim, gap selection, source patching
indicators, and Fit to Fill from four points. Shipped keys differ from
[keyboard.md](keyboard.md); `docs/keyboard-shortcuts.md` lists the shipped set.

## Animation ([animation.md](animation.md))

Keyframes carry one easing per segment: Linear, EaseIn, EaseOut, EaseInOut,
Hold, or a CSS cubic bezier (`crates/aria-models/src/param/easing.rs`). No
per-side speed and influence, spatial tangents, roving keyframes, motion paths,
parenting, drivers, or presets.

## Playback ([playback.md](playback.md))

| Area | Current build |
| --- | --- |
| Present | Each frame is rendered on the GPU, read back to the CPU, copied with a BGRA swap (`crates/aria-ui/src/frame.rs`), and uploaded again for display. |
| Threads | One OS thread per frame (`crates/aria-ui/src/handoff.rs`, `on_a_thread`). |
| Decode | Serial, no decode-ahead, no frame cache. On the Desktop machine the first frame took 527 ms, 513 ms of it decode, after zero-copy DMABuf import failed and decode fell back to CPU NV12. |
| Scopes | Recomputed on the UI thread every frame (`crates/aria-ui/src/app/transport.rs`). |
| Shuttle audio | Silent above 1x and in reverse. |

## Audio ([audio.md](audio.md))

Waveforms, clip volume, a mixer with faders, output routing, and sends,
ducking, beat and silence detection, and RNNoise denoise exist. Audio effects
are not a plugin contribution point.

## Panels ([panels.md](panels.md))

Six panes (`ActivePanel` in `crates/aria-ui/src/types_editor.rs`): Media,
Preview, Timeline, Inspector, Assistant, and Mixer; the Assistant and Mixer
replace the preview while open. A markers list is in the sidebar, the plugin
list is a Settings page, and history, the command palette, and the export
dialog exist. No Info, Align, Character, Paragraph, or Jobs panels.

## Performance ([performance.md](performance.md))

No `aria-bench` scenarios exist. Current measurements are in
[`docs/performance.md`](../performance.md): plugin mount 222 ms cold, warm
decoder seek 0.8 ms, cold open and seek 151 ms.
