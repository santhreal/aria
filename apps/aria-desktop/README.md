# aria-desktop

The Aria desktop editor is a native Rust + [GPUI](https://www.gpui.rs)
frontend. It mounts the built-in plugins and opens the `aria-ui` shell.
Unlike the mobile apps, it links `aria-engine` **directly** (no C-ABI/JNI
bridge).

This crate owns process bring-up. Timeline state and edit validation live
in the shared engine and model crates. Presentation lives in `aria-ui`.

## Responsibilities

- Launch the desktop application.
- Mount the built-in plugin set (`aria.storage` through `aria.ai`).
- Open the GPUI editor shell.

## Main areas

- `src/main.rs`: process startup, plugin mount, GPUI shell.
- `src/bootstrap.rs`: tracing.

## Running

```bash
cargo run -p aria-desktop
cargo run -p aria-desktop -- clip.mp4 grade.cube
```

A path argument means what dropping that file on the window means. A `.aria`
file opens as a project, media is imported into a new project and laid end to
end, a `.cube` file becomes the selected clip's LUT, and a `SKILL.md` file
installs as an assistant skill.

`ARIA_DATA_DIR` moves the settings, cache, and recovery directory off `~/.aria`.
Set it to run a second instance without the two fighting over one settings file.
`ARIA_MOTION_SCALE` multiplies every animation duration.

Media decode/encode is platform-native: AVFoundation on Apple, Media
Foundation on Windows, GStreamer on Linux. Linux needs the distro
GStreamer plugins (`gstreamer1.0-plugins-base`, `-good`, and either
`-ugly`/`-libav` or a VAAPI/NVENC plugin). Stills import without them.

## Development notes

Keep UI-only state in `aria-ui`. If behavior affects project correctness,
undo/redo, export, or preview output, it should usually be an engine command
or model change rather than hidden in this crate.

Avoid blocking the GPUI event loop. File dialogs and long-running engine
work stay asynchronous or worker-backed.

## Testing

This crate is a binary. UI and first-run tests live in `aria-ui`:

```bash
cargo test -p aria-ui --lib
```

Many UI changes also need targeted engine tests because the engine is the
source of truth for edit behavior.
