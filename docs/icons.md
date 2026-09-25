# Icon registry

The single place that tracks every icon the UI wants. Titlebar, transport,
preview, and track headers use the shipped Lucide SVGs and the brand mark.
Every entry below is wired; register a new placeholder here before shipping it.

## Workflow

When you build UI and reach for an icon that doesn't exist yet, **don't
block on it**:

1. Ship the control now with a short text/char placeholder.
2. **Register it here** under the right section, newest first.
3. Later, someone fetches the SVG, drops it in `assets/icon/`, and swaps
   the placeholder — then flips the entry to `[x]`.

### Registry format

```
- [ ] `lucide-name` — placeholder `"X"` — `crates/aria-ui/src/panels/file.rs` — what it does.
```

- `[ ]` = needed (placeholder live in the UI) · `[x]` = fetched + wired in.
- Always include the **placeholder string** and the **file**.

## Where icons live

All UI icons live under the **single** tracked root `assets/icon/`
(transport and the brand mark in `icon/`, library glyphs in
`icon/library/`, and Lucide/Tabler fetches under `shell/`, `timeline/`,
`text/`, `inspector/`, `ui/`). Media scratch files for local dev and tests
live in gitignored `local-assets/assets/`.

`assets/icon/aria-icon.png` is the canonical mark. `icons::brand_mark`
draws it in the window, Linux packages install it, `scripts/package-macos.sh`
turns it into `AppIcon.icns`, and the Xcode asset catalog under
`apps/aria-ios-macos` contains the resized copies.

### Generated workflow set

`assets/generated/` contains the hand-authored 32 px workflow icon set added
in repository commit `9e55722`. The files use square line caps, a 2.5 px
stroke, and `currentColor`; no external generator or upstream icon package
produced them.

## Source

Primary: **[Lucide](https://lucide.dev)** (MIT, single-stroke, 2 px default
stroke). Fallback: **[Tabler](https://tabler.io/icons)** (MIT).

## Already shipped

`play`, `pause`, `fullscreen` · library tabs `media`, `audio`, `text`,
`stickers`, `effects`, `transitions`, `stock`, `ai`, `sfx`, `filters`,
`adjustment` · brand mark `aria-icon.png` · chat `send` /
`circle-stop`.

---

## Registry

### Titlebar — `crates/aria-ui/src/chrome/tool_strip.rs`

- [x] `upload` — `assets/icon/shell/upload.svg` — export.
- [x] `settings` — `assets/icon/shell/settings.svg` — settings.
- [x] `scissors` — `assets/icon/timeline/scissors.svg` — razor tool.
- [x] `zoom-in` / `zoom-out` — `assets/icon/timeline/zoom-in.svg` — zoom tool and transport.
- [x] `magnet` — `assets/icon/timeline/magnet.svg` — snap toggle.
- [x] `aria-icon.png` — `assets/icon/aria-icon.png` — the titlebar mark, drawn by `icons::brand_mark`.

### Chat — `crates/aria-ui/src/panels/assistant.rs`

- [x] `send` — `assets/icon/chat/send.svg` — submit a message.
- [x] `circle-stop` — `assets/icon/chat/stop.svg` — stop generation.

### Preview — `crates/aria-ui/src/panels/preview.rs`

- [x] `play` / `pause` — `assets/icon/play.svg` — transport.
- [x] `fullscreen` — `assets/icon/fullscreen.svg` — fit timeline.
- [x] `zoom-in` / `zoom-out` — `assets/icon/timeline/zoom-in.svg` and `assets/icon/timeline/zoom-out.svg` — preview zoom step, replacing the `"+"` and `"−"` text glyphs.

### Home — `crates/aria-ui/src/panels/home/rail.rs` and `panes.rs`

- [x] `plus` — `assets/icon/plus.svg` — New Project tile.
- [x] `folder-open` — `assets/icon/folder-open.svg` — Open tile.
- [x] `clapperboard` / `image` / `sparkles` — `assets/icon/clapperboard.svg`, `assets/icon/library/image.svg`, and `assets/icon/shell/sparkles.svg` — Recents, Samples, and Learn rail tabs.

### Timeline — `crates/aria-ui/src/surfaces/lane_header.rs` and `panels/timeline/ruler.rs`

- [x] `plus` — `assets/icon/plus.svg` — add video track.
- [x] `eye` / `eye-off` — `assets/icon/timeline/eye.svg` — track enabled.
- [x] `volume-2` / `volume-x` — `assets/icon/timeline/volume-2.svg` — track mute.
- [x] `lock` / `lock-open` — `assets/icon/timeline/lock.svg` — track lock.
- [x] `headphones` — `assets/icon/timeline/headphones.svg` — track solo.
- [x] `chevron-down` — `assets/icon/ui/chevron-down.svg`, registered as `CHEVRON_DOWN`. One glyph for every disclosure: `components::chevron` turns it a quarter turn for a shut row — collapse and expand a lane.
- [x] `crosshair` — `assets/icon/timeline/crosshair.svg` — lane target.
- [x] `link` / `unlink` — `assets/icon/timeline/link.svg` and `assets/icon/timeline/unlink.svg` — lane sync lock.

### Tool strip — `crates/aria-ui/src/chrome/tool_strip.rs`

- [x] `move-horizontal` — `assets/icon/timeline/move-horizontal.svg` — slip tool.
- [x] `columns-3` — `assets/icon/timeline/columns-3.svg` — slide tool.
- [x] `grip-vertical` — `assets/icon/timeline/grip-vertical.svg` — rolling edit tool.
- [x] `gauge` — `assets/icon/timeline/gauge.svg` — rate stretch tool.
- [x] `fold-horizontal` — `assets/icon/timeline/fold-horizontal.svg` — ripple trim tool.

### Media pool — `crates/aria-ui/src/panels/sidebar/bins.rs`

- [x] `folder` — a bin row draws `components::chevron` (`CHEVRON_DOWN`), which leans toward the click as the row lights.
- [x] `chevron-left` — `assets/icon/chevron-left.svg` — `crates/aria-ui/src/panels/sidebar_multicam.rs` — move a multicam angle one frame earlier on the group's clock.
- [x] `chevron-right` — `assets/icon/chevron-right.svg` — `crates/aria-ui/src/panels/sidebar_multicam.rs` — move a multicam angle one frame later.
- [x] `audio-lines` — `assets/icon/timeline/audio-lines.svg` — `crates/aria-ui/src/panels/sidebar_multicam.rs` — place a group's cameras from their sound.

### Inspector — `crates/aria-ui/src/panels/inspector/tabs/effects.rs`

- [x] `trash-2` — `assets/icon/inspector/trash-2.svg` — remove effect.
- [x] `rotate-ccw` — `assets/icon/inspector/rotate-ccw.svg` — reset the transform (`tabs/transform.rs`) and one colour wheel (`blocks/wheels.rs`).

### Fine as text (no icon needed)

Timecode separators, the zoom readout, and word buttons in dialogs
(Choose… / Cancel / Export / Done / New Project / Import).
