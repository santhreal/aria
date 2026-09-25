# aria (Python)

MoviePy-style Python bindings for the [Aria](https://github.com/santhreal/aria)
video engine. Build a project with explicit tracks, place clips, animate properties,
and export, all through a track-first object model over the pure-Rust timeline,
GPU compositor, and platform-native exporter.

See [api-design.md](./api-design.md) for the full v2 API reference.

## Install

```bash
pip install --pre aria-py   # imports as `aria`
```

Wheels ship for macOS (Apple Silicon) and Windows (x64), Python 3.9+. The
distribution is named `aria-py` (the bare `aria` name on PyPI belongs to
an unrelated project); the module you import is `aria`. `--pre` is needed
while releases carry a pre-release version (e.g. `0.5.3a0`).

## Install (from source)

Requires the Rust toolchain and [maturin](https://www.maturin.rs/).

```bash
cd crates/aria-py
python3 -m venv .venv && source .venv/bin/activate
pip install maturin numpy
maturin develop --release      # builds + installs `aria` into the venv
```

## Quick start

```python
import aria
from aria import Project, Text, Solid

p = Project("demo", fps=30, canvas="16:9", background=(20, 20, 30))

bg = p.add_track("sticker", name="Background")
overlay = p.add_track("text", name="Titles")

bg.add(Solid((38, 42, 64, 255)), start=0.0, duration=2.0)
title = overlay.add(
    Text("Aria", size=220, color="#f0f0ff"),
    start=0.0,
    duration=2.0,
)
title.animate(opacity=[(0.0, 0.0), (0.5, 1.0)], easing="ease_out")

print(p)                      # Project(size=(1920, 1080), fps=30.000, duration=2.000s)
frame = p.get_frame(0.5)      # numpy uint8 array, shape (height, width, 4), RGBA
p.export("out.mp4")           # H.264/mp4 (+ AAC when audio clips exist)
p.save("demo.aria")
```

## Object model

```
Project
├── media pool          p.import_media(path) -> Media
├── markers             p.add_marker(t, name) -> Marker
├── tracks              p.add_track(kind) -> Track
│   └── clips           track.add(content, start=...) -> Clip
│       ├── transform / crop / speed / volume
│       ├── speed ramp  clip.set_speed_curve([(0.0, 1.0), (1.0, 2.0)])
│       ├── relink      clip.set_media(other)
│       ├── links       clip.link_with([other])
│       ├── effects     clip.add_effect(id) -> Effect
│       └── transition  clip.transition(id, duration=...)
├── template            p.save_template(path, name) -> Template
└── render              p.get_frame(t), p.export(path)
```

**Tracks are explicit.** Create a track, then add clips to it. Content descriptors
(`Text`, `Solid`, shapes, `media.subclip(...)`) describe *what*; `track.add` decides
*where* and *when*.

**Animation:** set constants with properties (`clip.opacity = 0.5`) or keyframes with
`clip.animate(opacity=1.0, at=0.4)` / batch curves
`clip.animate(opacity=[(0.0, 0.0), (0.4, 1.0)])`.

## Media import

`import_media` accepts probed **video, audio, and still images** (PNG, JPEG, and
other formats the decoder recognizes). Stills register with `kind == "image"`,
a default duration of 5 seconds, and no audio. Place them on `video` tracks;
the placement duration can exceed the pool default (the same frame holds).

```python
clip = p.import_media("footage.mp4")
still = p.import_media("photo.png")
main = p.add_track("video")
main.add(clip.subclip(3.0, 8.0), start=0.0)
main.add(clip[3:8], start=0.0)   # slicing sugar
main.add(still, start=0.0, duration=10.0)  # hold the still for 10s
```

## Track kinds

`video`, `audio`, `text`, `sticker`, `effect`, `filter`, `adjustment`. The kind
must match the clip content (`TrackKindError` on mismatch): media goes on
`video`/`audio`, `Text` on `text`, `Solid`, shapes, and `Sticker` on `sticker`.
`Sticker("heart")` places a bundled sticker (static or animated); browse ids
with `aria.stickers()`.

## Markers

```python
m = p.add_marker(1.5, "beat", color="red", comment="chorus starts")
m.time = 2.0
[marker.name for marker in p.markers]
```

Without a colour the palette cycles, so markers added in a loop stay
distinguishable. `aria.marker_colors()` lists the palette.

## Speed ramps

```python
clip.set_speed_curve([(0.0, 1.0), (0.5, 2.0), (1.0, 1.0)])
clip.set_speed_curve("ramp_up")      # a built-in; aria.speed_presets() lists them
clip.set_speed_curve(None)           # back to one rate
clip.speed_curve                     # [(0.0, 1.0), (0.5, 2.0), (1.0, 1.0)]
clip.speed_preset                    # "ramp_up", or None for a hand-written ramp
```

Positions run 0.0…1.0 across the clip, so a ramp survives a trim. The clip's
timeline length follows the ramp.

## Relink and links

```python
clip.set_media(other)                # same placement, different file
clip.set_media(other, start=3.0)     # and a different in-point
clip.link_with([audio_clip])         # move, trim and delete together
clip.linked                          # the other clips in the group
clip.unlink()
```

## Templates

A template is a finished edit whose media is a placeholder. Mark the clips a
caller may replace, save, and fill:

```python
clip.replaceable = aria.Slot(order=0, accepts="image", label="your photo")
p.save_template("card.ariat", "Photo card", author="you")

t = aria.Template.load("card.ariat")
filled = t.fill([p.import_media("mine.png")])   # one pick per slot
filled.export("out.mp4")
```

`save_template(..., bundle=True)` writes a `.ariab` that carries the sample
media, and `aria.Template.install(bundle, dest_dir)` unpacks one on a machine
that has neither. `t.slots` reports each slot's order, restriction, label and
locked length; `t.texts` reports the editable titles.

## Errors

| Exception | When |
|-----------|------|
| `OverlapError` | Clips overlap on the same track |
| `TrackKindError` | Wrong track kind for content |
| `MediaError` | Import/probe failure |
| `RenderError` | GPU frame or export failure |

## Development

Two test layers, and the Rust one runs without a build step:

```bash
cargo test --all-targets
```

That covers the conversion helpers and the binding surface itself: a script
builds the module the way `import aria` does, places clips, splits them, and
reads back what the getters report, and every shape the engine draws is checked
against the descriptor a script names it with. It needs no flags because the
crate's default feature links an interpreter. The wheel is the build that does
not: `extension-module` leaves CPython's symbols to the host, which is what lets
the module import into any supported Python, and it is what maturin asks for
(`[tool.maturin]` in `pyproject.toml`). On macOS point dyld at the framework:

```bash
DYLD_FRAMEWORK_PATH="$(python3 -c 'import sys; print(sys.base_prefix.rsplit("/Python3.framework", 1)[0])')" \
    cargo test --all-targets
```

The Python suite is the outer layer: it needs a wheel, so it runs after a build
and covers the module as an installed package.

```bash
maturin develop                     # debug build into the active venv
pip install pytest
python -m pytest tests/ -q          # full API suite (bootstraps its own mp4 via export)
python tests/smoke.py               # quick GPU frame smoke test
python examples/tour.py             # optional; needs sample media paths
```

> **Export:** uses the platform-native encoder: Apple (VideoToolbox) and Windows
> (Media Foundation), both H.264 + AAC → mp4. Other platforms raise until their
> backends land.
