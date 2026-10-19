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

An icon sets `currentColor` on `stroke` or `fill`, never on the root `color`
attribute: usvg cannot read `color="currentColor"` and logs a warning each
time gpui rasterises the glyph at a new size.
`every_registered_glyph_parses_without_a_warning` in
`crates/aria-ui/src/icons.rs` parses every registered SVG through gpui's
renderer and fails on any usvg warning.

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

Primary: **[Lucide](https://lucide.dev)** (ISC, single-stroke, 2 px default
stroke). Fallback: **[Tabler](https://tabler.io/icons)** (MIT).

## Already shipped

`play`, `pause`, `fullscreen` · library tabs `media`, `audio`, `text`,
`stickers`, `effects`, `transitions`, `stock`, `ai`, `sfx`, `filters`,
`adjustment` · brand mark `aria-icon.png` · chat `send` /
`circle-stop`.

---

## Registry

### Tool bar — `crates/aria-ui/src/chrome/tool_strip.rs`

- [x] `upload` — `assets/icon/shell/upload.svg` — export.
- [x] `mouse-pointer-2` — `assets/icon/timeline/mouse-pointer-2.svg` — selection tool.
- [x] `chevrons-right` / `chevrons-left` — `assets/icon/timeline/chevrons-right.svg` and `chevrons-left.svg` — track select forward and backward tools.
- [x] `scissors` — `assets/icon/timeline/scissors.svg` — razor tool.
- [x] `zoom-in` — `assets/icon/timeline/zoom-in.svg` — zoom tool.
- [x] `hand` — `assets/icon/timeline/hand.svg` — hand tool.
- [x] `move-horizontal` — `assets/icon/timeline/move-horizontal.svg` — slip tool.
- [x] `columns-3` — `assets/icon/timeline/columns-3.svg` — slide tool.
- [x] `grip-vertical` — `assets/icon/timeline/grip-vertical.svg` — rolling edit tool.
- [x] `gauge` — `assets/icon/timeline/gauge.svg` — rate stretch tool.
- [x] `fold-horizontal` — `assets/icon/timeline/fold-horizontal.svg` — ripple trim tool.
- [x] `aria-icon.png` — `assets/icon/aria-icon.png` — the titlebar mark, drawn by `icons::brand_mark`.

### Chat — `crates/aria-ui/src/panels/assistant.rs`

- [x] `send` — `assets/icon/chat/send.svg` — submit a message.
- [x] `circle-stop` — `assets/icon/chat/stop.svg` — stop generation.

### Viewer — `crates/aria-ui/src/panels/preview.rs`, `preview_source.rs`, `preview_tabs.rs`, `preview_controls.rs`, `preview_resolution.rs`, `preview/handles.rs`, and `preview/scopes.rs`

- [x] `play` / `pause` — `assets/icon/play.svg` — transport.
- [x] `skip-back` / `skip-forward` — `assets/icon/skip-back.svg` and `assets/icon/skip-forward.svg` — go to start and go to end.
- [x] `chevron-left` / `chevron-right` — `assets/icon/chevron-left.svg` and `assets/icon/chevron-right.svg` — step one frame.
- [x] `zoom-in` / `zoom-out` — `assets/icon/timeline/zoom-in.svg` and `assets/icon/timeline/zoom-out.svg` — preview zoom step, replacing the `"+"` and `"−"` text glyphs.
- [x] `wrench` — `assets/icon/timeline/wrench.svg` — Viewer display settings, at the right end of the tab row: holds View > Viewer Display Settings under itself.
- [x] `repeat` — `assets/icon/timeline/repeat.svg`, registered as `LOOP` — loop playback.
- [x] `replace` — `assets/icon/preview/replace.svg`, registered as `PROXY` — play proxies or the original media.
- [x] `scaling` / `crop` / `square-dashed` — `assets/icon/preview/scaling.svg`, `crop.svg`, and `square-dashed.svg`, registered as `TRANSFORM`, `CROP`, and `MASK` — the Transform, Crop, and Mask handle sets.
- [x] `activity` — `assets/icon/preview/activity.svg`, registered as `SCOPES` — open or close the scope strip.

### Home — `crates/aria-ui/src/panels/home/rail.rs` and `panes.rs`

- [x] `plus` — `assets/icon/plus.svg` — New Project tile.
- [x] `folder-open` — `assets/icon/folder-open.svg` — Open tile.
- [x] `clapperboard` / `image` / `sparkles` — `assets/icon/clapperboard.svg`, `assets/icon/library/image.svg`, and `assets/icon/shell/sparkles.svg` — Recents, Samples, and Learn rail tabs.

### Timeline — `crates/aria-ui/src/surfaces/lane_header.rs`, `panels/timeline/header.rs`, and `panels/timeline/overview.rs`

- [x] `plus` — `assets/icon/plus.svg` — add video track, at the left of the zoom bar.
- [x] `magnet` / `link` / `wrench` — `assets/icon/timeline/magnet.svg`, `link.svg`, and `wrench.svg` — snapping, linked selection, and timeline display settings in the timeline header.
- [x] `flag` — `assets/icon/timeline/flag.svg`, registered as `FLAG` — add a marker at the playhead.
- [x] `chart-spline` — `assets/icon/timeline/chart-spline.svg`, registered as `GRAPH` — open or close the graph editor.
- [x] `x` — `assets/icon/shell/x.svg`, registered as `CLOSE` — clear the property search, and clear the Effects & Presets search (`panels/effects_browser.rs`).
- [x] `eye` / `eye-off` — `assets/icon/timeline/eye.svg` — track enabled.
- [x] `volume-2` / `volume-x` — `assets/icon/timeline/volume-2.svg` — track mute.
- [x] `lock` / `lock-open` — `assets/icon/timeline/lock.svg` — track lock.
- [x] `headphones` — `assets/icon/timeline/headphones.svg` — track solo.
- [x] `chevron-down` — `assets/icon/ui/chevron-down.svg`, registered as `CHEVRON_DOWN`. One glyph for every disclosure: `components::chevron` turns it a quarter turn for a shut row — collapse and expand a lane.
- [x] `crosshair` — `assets/icon/timeline/crosshair.svg` — lane target.
- [x] `link` / `unlink` — `assets/icon/timeline/link.svg` and `assets/icon/timeline/unlink.svg` — lane sync lock.

### Media pool — `crates/aria-ui/src/panels/sidebar_bins.rs` and `sidebar_pool_bars.rs`

- [x] `import` — `assets/icon/library/import.svg`, registered as `IMPORT` — Import, at the right end of the Media tab's filter row.
- [x] `audio-lines` — `assets/icon/timeline/audio-lines.svg` — Audio only, beside Import: the pool lists its audio sources.
- [x] `image` / `list` / `layout-grid` / `move` — `assets/icon/library/image.svg`, `list.svg`, `layout-grid.svg`, and `move.svg`, registered as `IMAGE`, `LIST`, `LAYOUT_GRID`, and `MOVE` — the Tiles, List, Icons, and Freeform views in the footer.
- [x] `folder-plus` — `assets/icon/library/folder-plus.svg`, registered as `FOLDER_PLUS` — New bin, in the footer.
- [x] `layers` — `assets/icon/library/layers.svg`, registered as `LAYERS` — New multicam, in the footer (`sidebar_multicam.rs`).
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
