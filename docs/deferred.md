# Deferred work

Every entry is parked, and states what would end the parking. An entry has
either an **Acceptance** line, the observable result that closes it, so the work
can be picked up without re-deciding what it means, or an **Out of scope** line
stating why it will not be done. Nothing here is parked without one.

When something here ships, delete the entry, update
[spec/status.md](spec/status.md), and fix any README or overview claim that
still oversells it.

## Platform / media backends

### Tiled DMABuf import on Linux

- **Wanted:** Import vendor-tiled GStreamer DMABuf (NVIDIA block-linear,
  Intel CCS, AMD DCC) into the compositor without a CPU copy.
- **Today:** LINEAR NV12 DMABuf imports through Vulkan
  `VK_KHR_external_memory_fd`. Tiled modifiers and missing fds fall back
  to CPU NV12 at decoder open, and the modifier is named once per session
  in the log so the fallback is visible.
- **Blocked on:** wgpu-hal 29 has no `DRM_FORMAT_MODIFIER_EXT` image create, so
  there is no call to write the import against.
- **Acceptance:** a tiled NV12 buffer from the GStreamer decoder reaches the
  compositor with no CPU copy on the same machine that logs the fallback today,
  and the log line stops naming a fallback for that modifier.
- **Where:** `crates/aria-compositor/src/vulkan_import.rs`,
  `crates/aria-decoder/src/gst_gpu.rs`.

### Zero-copy GPU surface import on Android

- **Wanted:** Import decoder GPU surfaces (`AHardwareBuffer`) into the
  compositor without a CPU round-trip.
- **Today:** MediaCodec runs in ByteBuffer mode, so there is no
  `AHardwareBuffer` to import. `decoder_capability()` reports no GPU
  surfaces on Android and `MediaCodecDecoder::open` rejects
  `OutputMode::Gpu`, so CPU planes are the shipped Android path
  ([architecture.md](architecture.md) “Decoder output modes”). Apple,
  Windows, and Linux have zero-copy import (Linux LINEAR NV12 only).
- **Blocked on:** a device. The path cannot be written against an emulator's
  software codec, which produces no hardware buffer either.
- **Acceptance:** `decoder_capability()` reports GPU surfaces on Android,
  `MediaCodecDecoder::open` accepts `OutputMode::Gpu`, and a decoded frame
  composites with no plane copy, measured on hardware.
- **Where:** `crates/aria-compositor` GPU import paths,
  `crates/aria-decoder/src/android.rs`.

## AI assistant

### `Import` behind a confirmation

- **Wanted:** The agent can import media behind an explicit per-prompt
  confirmation card. `Open` / `Save` / `Export` stay human-only.
- **Today:** Edit-only vocabulary. The tool schema has no import verb, so a
  prompt asking for one is refused by the closed schema rather than by a
  policy check.
- **Acceptance:** an import verb exists in the schema, a prompt that uses it
  draws a confirmation naming the exact file, nothing is read until it is
  accepted, refusing it leaves the project untouched, and `Open` / `Save` /
  `Export` are still rejected.
- **Where:** [docs/ai-agent-roadmap.md](ai-agent-roadmap.md) Phase 5.

### Local-model tool-calling quality

- **Out of scope:** a product risk, not a code milestone. A local model that
  emits a malformed call is already refused by the closed schema and the dry-run
  pass, so there is nothing to build; picking a better model is a setting.
- **Where:** `crates/aria-ai/src/validate`.

## Resolved

Kept so a reader does not re-park something that landed.

- **`describe_project` windows a long project.** Up to 300 clips it describes
  everything, unchanged. Past that it describes the clips around the playhead
  and includes a `window` stating the span, how many clips it holds, and how many
  clips and markers sit outside it. The tool takes `from_seconds` and
  `to_seconds`, so the model reads another stretch by asking for it.
- **A dropped setting** is reported: every value the loader cannot use keeps
  its default and is recorded on the settings, and the storage report and the
  Caches section of Settings both name the key, the value, and the reason.
- **Pitch lock** ships: `EditCommand::SetClipPitch` toggles it, a retimed clip
  time-stretches while keeping its pitch by default, and clearing the lock mixes
  varispeed in preview and export.
- **Auto-duck** ships: a flagged voice lane is measured and the dip is written
  as volume keyframes on the selected music.
- **Beat detection** ships: a beat run stores its grid on the clip, the ruler
  draws it, and the magnet snaps to it.
- **Catalog effects without a shader** are not a gap: every id in
  `data/effects.toml` has a compositor pass, and an id with none is skipped
  rather than dispatched, so a catalog override cannot produce an effect that
  silently does nothing.
- **`TextStyle` fields all reach the renderer.** `effect_preset` is the one
  field the renderer never reads, because the model resolves it into the
  stroke, background and shadow it names before a frame is drawn.
- **Effect presets can be made, not only applied.** Saving is a name field the
  rack's Save button opens, Enter commits it, and each saved row has a
  delete control. Both write the preset file, so the list a launch reads is the list the
  last one left.
- **The export sheet is a value.** Pressing Export assembles an `ExportRequest`
  with no window, no session lock and no encoder probe, and queueing takes that
  value, so the request is asserted without starting a render. Whether the
  machine can encode it is still answered where the job is enqueued.
- **Render and replace covers sound.** A clip on an audio lane bakes to a WAV of
  16-bit PCM through the same mixer, the same transactional publish and the same
  one-entry history as a picture bake. Which medium a lane bakes to is read off
  its lane kind row, so a lane kind that is neither picture nor sound is refused
  by name.
- **The assistant renders and replaces.** Two tool rows cover the pair. A
  restore is an edit the turn applies; a render is work the shell starts, and the
  turn reports it started rather than finished, because the file does not exist
  when the call returns. A render worth nothing is refused in the turn, by the
  rule the renderer itself reads.
