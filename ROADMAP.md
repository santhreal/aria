# Aria Roadmap

Delivery order for the editor defined in [docs/spec](docs/spec/README.md). Each
milestone lists the spec chapters it delivers. A milestone is done when its
chapters' rows are gone from [docs/spec/status.md](docs/spec/status.md) and the
named [performance](docs/spec/performance.md) scenarios pass. Parked work is in
[docs/deferred.md](docs/deferred.md).

## 1. Plugin host

[plugins.md](docs/spec/plugins.md)

- `plugin.toml` manifests with owned data, activation events, permissions, and
  `.ariaplugin` packages.
- Host API: project snapshot, `transact` as one undo step, the full event set,
  render, media, gpu, storage, settings, ui, and jobs.
- Runtime effects and transitions: WGSL render graphs dispatched by the
  compositor, parameters through a storage buffer, CPU fallback.
- Generators, clip types, property drivers, audio processors, decoders,
  importers, exporters, scopes, AI tools, and drop handlers as contribution
  points.
- Stable C ABI under `aria-sdk`; WASM component tier on Wasmtime; unload, hot
  reload, fault isolation, missing-plugin passthrough.
- Every shipped capability moved onto the public API.
- Plugins panel and the `aria plugin` developer commands.

## 2. Playback engine

[playback.md](docs/spec/playback.md), [performance.md](docs/spec/performance.md)

- Zero-copy present: the Viewer draws the rendered texture.
- Persistent decode pool with decode-ahead, GPU frame cache with system memory
  spill, render-ahead, audio master clock.
- Playback and paused resolution, proxies, render ranges, dropped-frame
  counters, shuttle audio with pitch held.
- `aria-bench` scenarios and fixtures.

## 3. Timeline and editing

[timeline.md](docs/spec/timeline.md), [editing.md](docs/spec/editing.md)

- Timeline drawn from a clip index with culling, holding refresh rate at
  10,000 clips.
- Track headers, clip bars, property rows, render bar.
- Tools, edge selection, keyboard trim, gaps, source patching, three-point and
  four-point edits.

## 4. Animation

[animation.md](docs/spec/animation.md)

- Keyframe model with per-side speed and influence, spatial tangents, roving,
  and hold; project schema bump with conversion on load.
- Graph editor, motion paths, parenting, drivers with `aria.expressions`,
  presets, time remap.

## 5. Workspace and panels

[layout.md](docs/spec/layout.md), [look.md](docs/spec/look.md),
[viewer.md](docs/spec/viewer.md), [panels.md](docs/spec/panels.md),
[audio.md](docs/spec/audio.md), [keyboard.md](docs/spec/keyboard.md)

- Frames with tab docking, splitting, floating windows, and workspaces.
- Program and Source tabs, on-canvas controls, rulers and guides.
- Project, Effect Controls, Effects & Presets, Info, Markers, Align, Text,
  Jobs, and Plugins panels; Audio Track Mixer inserts and automation.
- The key map in keyboard.md with the keyboard editor; light theme.
