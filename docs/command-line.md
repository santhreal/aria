# Command line

The `aria` binary (`crates/aria-cli`, `Verb::ALL` in `crates/aria-cli/src/args.rs`) drives the same crates the editor drives. A frame rendered
here is the frame the editor shows, a file exported here is the file the export
sheet writes, and a detector run here finds what the analysis panel finds.

Build it:

```
cargo build --release -p aria-cli
```

The binary is `target/release/aria`.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success, or usage printed for `--help`, `-h`, or no arguments |
| `1` | The command ran and failed |
| `2` | The command line was wrong |

Running `aria` with no arguments, `aria --help`, or `aria -h` in any position
prints the usage block on stdout and exits `0`. A usage error prints the reason
and the usage block on stderr and exits `2`. A runtime failure prints the reason
on stderr and exits `1` (or the child exit code when `aria plugin dev` runs the
editor). Progress goes to stderr and results go to stdout, so
`aria inspect project.aria > facts.txt` captures the output alone.

## new

```
aria new <out.aria> [media...] [--fps 23.976|24|25|29.97|30|50|60]
```

| Flag | Default | Description |
| --- | --- | --- |
| `--fps <RATE>` | `24` | Sequence frame rate: `23.976`, `24`, `25`, `29.97`, `30`, `50`, or `60` |

Import each media file and place it after the previous one: the picture on
`V1` and the sound on `A1`, linked, as a drop in the editor places them. Each
file starts where every lane it lands on is free. Assembly runs through
`Engine::apply`, so the saved project satisfies every timeline invariant and
opens in the editor without repair.

`--fps` selects an exact rational rate (`23.976` is `24000/1001` and `29.97` is
`30000/1001`). Any rate outside the list is rejected with exit code `2`.

```
aria new rough.aria takes/*.mp4 --fps 23.976
```

## frame

```
aria frame <project.aria> [out.png] [--at SECONDS] [--fit WxH]
```

| Flag | Default | Description |
| --- | --- | --- |
| `--at <SECONDS>` | `0` | Timeline position in seconds, rounded to the nearest frame at the project rate |
| `--fit <WxH>` | canvas size | Bounding box (`WIDTHxHEIGHT`) that scales the canvas down to fit and never enlarges it |

Render one timeline instant to an 8-bit straight-alpha RGBA PNG. When `[out.png]`
is omitted, the image is written to `aria-frame.png`.

```
aria frame rough.aria thumb.png --at 12.5 --fit 640x360
```

## export

```
aria export <project.aria> <out.mp4> [--at SECONDS] [--to SECONDS]
                                     [--size WxH] [--fps N]
                                     [--codec h264|h265] [--bitrate KBPS]
                                     [--no-audio] [--quiet]
```

| Flag | Default | Description |
| --- | --- | --- |
| `--at <SECONDS>` | `0` | Start of the exported half-open timeline span in seconds |
| `--to <SECONDS>` | project duration | End of the exported half-open timeline span in seconds |
| `--size <WxH>` | project canvas | Output dimensions (`WIDTHxHEIGHT`) |
| `--fps <N>` | project rate | Whole-number output frame rate above zero |
| `--codec <CODEC>` | `h264` | Video codec: `h264` or `h265` |
| `--bitrate <KBPS>` | encoder default | Target video bitrate in kilobits per second |
| `--no-audio` | off | Omit the audio track and write video only |
| `--quiet` | off | Suppress per-frame progress updates on stderr |

Encode a timeline span. Defaults use the project canvas size, frame rate, and
audio over the whole timeline. `--at` and `--to` bound a half-open range; the
encoded file starts at timestamp zero regardless of where the span begins.

A codec this machine cannot write is rejected by name, with the list of
supported codecs. A `--bitrate` that the selected encoder cannot honor is
rejected as well. `aria export` prints the frame count, dimensions, container,
and codec on stdout, followed by the encoder that opened (with `hardware` or
`software`) and one `skipped` line for each earlier encoder candidate that did
not open:

```
wrote 120 frames of 1920x1080 MP4 · H.264 / AAC to out.mp4
encoder x264enc (software)
skipped nvh264enc is not installed
skipped nvcudah264enc is not installed
skipped vaapih264enc could not open its device
```

The flags build an `ExportFormat` resolved by `ExportSettings::resolved`, matching
the editor's export sheet. An export with zero frames (an empty timeline or an
`--at`/`--to` span covering no frames) fails with exit code `1` and states which
condition occurred.

```
aria export rough.aria out.mp4 --at 0 --to 30 --codec h265 --bitrate 8000
```

## inspect

```
aria inspect <project.aria> [--clips]
```

| Flag | Default | Description |
| --- | --- | --- |
| `--clips` | off | List every clip on every lane with its timeline span and source description |

Print the project's schema version, canvas size, frame rate, duration, marker
count, media pool, and lanes from bottom to top. A media pool entry whose file
is missing on disk is marked `OFFLINE`.

Lane state flags appear in brackets: `disabled`, `muted`, `solo`, `locked`,
`main`, `duck source`, `not targeted`, `no sync lock`.

## analyze

```
aria analyze <media> [--beats] [--silence] [--loudness] [--shots] [--limit N]
```

| Flag | Default | Description |
| --- | --- | --- |
| `--beats` | on when no audio detector flag is given | Run the beat detector on mono `16000` Hz audio |
| `--silence` | on when no audio detector flag is given | Run the silence detector on mono `16000` Hz audio |
| `--loudness` | on when no audio detector flag is given | Compute the loudness contour on mono `16000` Hz audio |
| `--shots` | off | Sample video frames at `2` fps up to `30` minutes and detect shot boundaries |
| `--limit <N>` | `40` | Maximum rows printed per detector before eliding the remainder |

Run detectors over a media file and print the results on stdout without
modifying any project file. With no audio detector flag (`--beats`, `--silence`,
`--loudness`), all three audio detectors run over one shared mono `16000` Hz
decode. `--shots` samples video frames at `2` fps up to `30` minutes (`1800`
seconds). `--limit` caps the printed rows per detector (`40` by default) and
prints the count of remaining rows.

```
aria analyze interview.wav --silence
aria analyze b-roll.mp4 --shots --limit 200
```

## demo

```
aria demo [out.png]
```

Composite a `1920x1080` text scene on a headless GPU and write the canvas to an
8-bit RGBA PNG (`aria-demo.png` when `[out.png]` is omitted). `aria demo` takes
no flags and requires no project file; it exercises headless GPU initialization,
text rasterization, compositing, and PNG readback.

## plugin

```
aria plugin new <id> [--tier wasm|native] [--dir PARENT] [--sdk PATH]
aria plugin dev <dir> [--editor PATH] [--data-dir PATH]
aria plugin check <dir|file.ariaplugin>
aria plugin pack <dir> [--out FILE] [--no-build]
```

Create, run, validate, and package external plugins (`Subcommand::ALL` in `crates/aria-cli/src/plugin/mod.rs`). Missing or unknown
sub-commands and unknown flags exit `2`. Build, validation, and runtime
failures exit `1`.

### plugin new

| Flag | Default | Description |
| --- | --- | --- |
| `--tier <TIER>` | `wasm` | Execution tier (`NewTier::ALL` in `crates/aria-cli/src/plugin/new.rs`): `wasm` or `native` |
| `--dir <PARENT>` | working directory | Parent directory in which `<id>/` is created |
| `--sdk <PATH>` | auto-resolved | Path to `crates/aria-sdk` for `--tier native` |

Create `<dir>/<id>/` (`<id>/` in the working directory when `--dir` is omitted)
with `plugin.toml`, a sample declared effect shader at `shaders/<slug>.wgsl`, a
sample palette command in `src/lib.rs`, `Cargo.toml`, and `.gitignore`. `--tier`
defaults to `wasm` and also writes `wit/plugin.wit` from the host world
definition. `--tier native` generates a crate against `aria-sdk`, resolved from
`--sdk PATH` when given, then `crates/aria-sdk` from the build tree when
present, then the repository git URL. Exits `1` without writing files if the
target directory already exists or `<id>` fails manifest validation. Prints the
created directory path and the `dev` and `pack` commands on stdout.

```
aria plugin new com.example.color-tools
aria plugin new com.example.notes --tier native --dir plugins
```

### plugin dev

| Flag | Default | Description |
| --- | --- | --- |
| `--editor <PATH>` | sibling `aria-desktop` | Path to the `aria-desktop` executable |
| `--data-dir <PATH>` | default profile | Profile directory passed to the editor in `ARIA_DATA_DIR` |

When `<dir>/Cargo.toml` is present, build the plugin in debug mode and stage
`plugin.wasm` or `lib/<os>-<arch>.<ext>` via a temporary file and atomic rename.
Start the editor binary (`--editor PATH`, or `aria-desktop` beside the `aria`
executable) with `ARIA_PLUGIN_DEV` set to the canonical package directory and,
when `--data-dir` is given, `ARIA_DATA_DIR` set to that profile directory. Poll
`Cargo.toml`, `src/`, and `wit/` every `500` ms and rebuild on change. A failed
rebuild prints diagnostics on stderr and leaves the running editor and previous
staged artifact in place. Data-only directories without `Cargo.toml` start the
editor without a build loop. Exits with the editor process status code when the
editor exits.

```
aria plugin dev com.example.color-tools --data-dir /var/tmp/aria-dev-profile
```

### plugin check

Validate a package directory or `.ariaplugin` archive without starting the
editor. `plugin check` takes no flags. Archives unpack to a temporary directory
that is removed when the check completes. Checks run in order and print one
status line per check on stdout:

1. `manifest`: parse `plugin.toml` and resolve the package tier.
2. `shaders`: compile declared effect, generator, and transition WGSL passes
   through the compositor validator. On failure, stderr includes the effect id,
   WGSL file path, 1-based line, and column.
3. `textures`: printed when the manifest declares `[[textures]]`, reporting the
   validated texture count.
4. `presets`: validate `presets/*.ariapreset` files and print the preset count
   and names (or `none`).
5. `contributes`: print declared command, menu row, key, setting, preset, and
   theme counts.
6. `permissions`: print `project`, `media`, `gpu`, `net`, `fs`, `process`, and
   `clipboard` grants in full.
7. `abi`: for WASM packages, compile and type-check `plugin.wasm` against
   `aria:plugin/plugin@1.0.0`. For native packages, inspect every file in `lib/`
   to verify its binary format (ELF, Mach-O, PE), CPU architecture, and exported
   `aria_plugin_entry` symbol against its `<os>-<arch>.<ext>` filename, load the
   current platform library, and print the unsandboxed native code warning. For
   data packages, print `data (no code)`.

An unbuilt source directory (`Cargo.toml` present without `plugin.wasm` or
`lib/`) exits `1` and prints `aria plugin pack <dir>` and
`aria plugin dev <dir>` on stderr.

```
aria plugin check com.example.color-tools
aria plugin check com.example.color-tools-0.1.0.ariaplugin
```

### plugin pack

| Flag | Default | Description |
| --- | --- | --- |
| `--out <FILE>` | `<dir>/<id>-<version>.ariaplugin` | Output `.ariaplugin` archive path |
| `--no-build` | off | Skip `cargo build --release` and pack existing staged artifacts |

Build the crate in release mode and stage the output when `<dir>/Cargo.toml`
exists (skipped with `--no-build`), run the `plugin check` validations on
`<dir>`, and write the `.ariaplugin` archive excluding crate build entries
(`Cargo.toml`, `Cargo.lock`, `src`, `wit`, `target`). `--out` sets the archive
path and defaults to `<dir>/<id>-<version>.ariaplugin`. Prints the check lines
and the written archive path on stdout.

```
aria plugin pack com.example.color-tools --out dist/color-tools.ariaplugin
```

## Desktop binary (`aria-desktop`)

```
aria-desktop [path...]
```

Start the graphical editor. Each positional `path` opens a `.aria` project or
imports a media or timeline file on launch, matching a file drop on the window.
`aria-desktop` takes no flags.

| Variable | Effect |
| --- | --- |
| `ARIA_DATA_DIR` | Profile directory for settings, caches, the recovery file, installed plugins, and catalog overrides |
| `ARIA_PLUGIN_DEV` | Plugin package directory loaded for development over any installed plugin with the same id and reloaded when its files change |
| `ARIA_MOTION_SCALE` | Startup duration multiplier for UI transitions (`MotionPolicy::parse_duration_scale`) |
| `RUST_LOG` | `tracing_subscriber::EnvFilter` directive string (`warn` default with `info` on Aria crates) |

`aria-desktop` exits with status 1 and prints the reason and the fix to
standard error when it cannot start: the profile directory is unusable, the
session cannot open a GPU device, no display server accepts the connection, or
the window does not open. On Linux the display server is the Wayland compositor
at `WAYLAND_DISPLAY`, else the X server at `DISPLAY`; with neither set, or with
`ZED_HEADLESS` set, the editor does not start.
