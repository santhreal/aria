# Command line

The `aria` binary drives the same crates the editor drives. A frame rendered
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
| 0 | success |
| 1 | the command ran and failed |
| 2 | the command line was wrong |

A usage error prints the reason, then the usage block, on stderr. A failure
prints only the reason. Progress goes to stderr, results to stdout, so
`aria inspect project.aria > facts.txt` captures the facts alone.

## new

```
aria new <out.aria> [media...] [--fps 23.976|24|25|29.97|30|50|60]
```

Import each file and lay it after the previous one: files with picture on `V1`,
files without on `A1`. The assembly runs through the engine, so the project
obeys every invariant the editor enforces and opens with no repair.

`--fps` names an exact rate. `29.97` is `30000/1001`, not `30/1`; a rate that is
not in the list is refused rather than rounded.

```
aria new rough.aria takes/*.mp4 --fps 23.976
```

## frame

```
aria frame <project.aria> [out.png] [--at SECONDS] [--fit WxH]
```

Render one timeline instant to an 8-bit RGBA PNG. `--at` defaults to `0` and is
rounded to the nearest frame at the project rate. `--fit` scales the canvas down
to fit a box, never up; without it the frame is the full canvas.

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

Encode a span. Defaults are the project's own canvas size, rate, and audio, over
the whole timeline. `--at`/`--to` are a half-open range: the output starts at
zero regardless of where the span begins.

A codec this machine cannot write is refused by name, with the list of codecs it
can. A `--bitrate` an encoder ignores is refused too: a file at the encoder's own
rate is not the file that was asked for. `aria export` prints what it wrote,
including the codec and container, then the encoder it opened and whether it
runs on hardware or software, then one `skipped` line for each encoder tried
before it that did not open, with the reason:

```
wrote 120 frames of 1920x1080 MP4 · H.264 / AAC to out.mp4
encoder x264enc (software)
skipped nvh264enc is not installed
skipped nvcudah264enc is not installed
skipped vaapih264enc could not open its device
```

The flags are the same request the app's export sheet makes, resolved by the same
code, so a scripted export and a sheet with the same choices write the same file
and refuse for the same reasons.

An export with no frames in it is refused rather than written: a project with
nothing on its timeline, or an `--at`/`--to` span that covers no frame of it.
The message names which of the two it is.

```
aria export rough.aria out.mp4 --at 0 --to 30 --codec h265 --bitrate 8000
```

## inspect

```
aria inspect <project.aria> [--clips]
```

Print the project's schema version, canvas, rate, duration, marker count, media
pool, and lanes. A pool entry whose file is gone is marked `OFFLINE`. `--clips`
lists every clip on every lane with its timeline span and what it plays.

Lane flags appear in brackets: `disabled`, `muted`, `solo`, `locked`, `main`,
`duck source`, `not targeted`, `no sync lock`.

## analyze

```
aria analyze <media> [--beats] [--silence] [--loudness] [--shots] [--limit N]
```

Run detectors and print what they found. Nothing is written: the moments index
belongs to a project.

With no detector flag, all three audio detectors run — they share one decode, so
asking for every one costs what asking for one costs. `--shots` samples video
frames instead, at 2 fps up to 30 minutes. `--limit` caps the rows per detector
(40 by default) and the remainder is counted, never dropped silently.

```
aria analyze interview.wav --silence
aria analyze b-roll.mp4 --shots --limit 200
```

## demo

```
aria demo [out.png]
```

Composite a text layer on a headless GPU and write the canvas. The one command
that needs no project: it proves the GPU, the text rasterizer, and the readback
path on a machine that has never opened the editor.

## plugin

```
aria plugin new <id> [--tier wasm|native] [--dir PARENT] [--sdk PATH]
aria plugin dev <dir> [--editor PATH] [--data-dir PATH]
aria plugin check <dir|file.ariaplugin>
aria plugin pack <dir> [--out FILE] [--no-build]
```

Create, run, validate, and package external plugins. Missing or unknown
sub-commands and unknown flags exit `2`. Build, validation, and runtime
failures exit `1`.

### plugin new

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

When `<dir>/Cargo.toml` is present, build the plugin in debug mode and stage
`plugin.wasm` or `lib/<os>-<arch>.<ext>` via a temporary file and atomic rename.
Start the editor binary (`--editor PATH`, or `aria-desktop` beside the `aria`
executable) with `ARIA_PLUGIN_DEV` set to the canonical package directory and,
when `--data-dir` is given, `ARIA_DATA_DIR` set to that profile directory. Poll
`Cargo.toml`, `src/`, and `wit/` every 500 ms and rebuild on change. A failed
rebuild prints diagnostics on stderr and leaves the running editor and previous
staged artifact in place. Data-only directories without `Cargo.toml` start the
editor without a build loop. Exits with the editor process status code when the
editor exits.

```
aria plugin dev com.example.color-tools --data-dir /var/tmp/aria-dev-profile
```

### plugin check

Validate a package directory or `.ariaplugin` archive without starting the
editor. Archives unpack to a temporary directory that is removed when the check
completes. Checks run in order and print one status line per check on stdout:

1. `manifest`: parse `plugin.toml` and resolve the package tier.
2. `shaders`: compile declared effect, generator, and transition WGSL passes
   through the compositor validator. On failure, stderr includes the effect id,
   WGSL file path, 1-based line, and column.
3. `permissions`: print `project`, `media`, `gpu`, `net`, `fs`, `process`, and
   `clipboard` grants in full.
4. `abi`: for WASM packages, compile and type-check `plugin.wasm` against
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

Build the crate in release mode and stage the output when `<dir>/Cargo.toml`
exists (skipped with `--no-build`), run the `plugin check` validations on
`<dir>`, and write the `.ariaplugin` archive excluding crate build entries
(`Cargo.toml`, `Cargo.lock`, `src`, `wit`, `target`). `--out` sets the archive
path and defaults to `<dir>/<id>-<version>.ariaplugin`. Prints the check lines
and the written archive path on stdout.

```
aria plugin pack com.example.color-tools --out dist/color-tools.ariaplugin
```
