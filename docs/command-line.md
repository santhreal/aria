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
including the codec and container.

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
