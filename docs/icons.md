# Icon registry

Every icon the editor draws is a row of `icon_assets!` in
`crates/aria-ui/src/icons.rs`: a constant, the file under `assets/icon/`, and
the path the asset source serves it under. This page lists each constant once,
with its file and the controls that draw it.

## Adding an icon

1. Fetch the SVG from [Lucide](https://lucide.dev) (ISC, single stroke, 2 px
   default stroke), or from [Tabler](https://tabler.io/icons) (MIT) when
   Lucide has no match.
2. Save it under `assets/icon/`, in the subdirectory of the surface that draws
   it.
3. Add a row to `icon_assets!` and draw the control with the new constant.
4. Add the constant to the table of that surface below.
   `every_registered_icon_has_one_row_in_icons_md` in
   `crates/aria-ui/src/icons.rs` fails when a constant has no row, when a row
   names a constant the registry does not hold, and when a row's file differs
   from the registry's.

A control may ship with a short text placeholder before its icon exists. Its
label is plain text and it has no row here until the icon lands.

## Files

All UI icons are under `assets/icon/`: transport and the brand mark at the
top level, library glyphs in `library/`, and Lucide and Tabler fetches under
`shell/`, `timeline/`, `text/`, `inspector/`, `preview/`, `chat/`, and `ui/`.
Media scratch files for local development and tests are in the gitignored
`local-assets/assets/`.

An icon sets `currentColor` on `stroke` or `fill`, never on the root `color`
attribute: usvg cannot read `color="currentColor"` and logs a warning each
time gpui rasterizes the glyph at a new size.
`every_registered_glyph_parses_without_a_warning` in
`crates/aria-ui/src/icons.rs` parses every registered SVG through gpui's
renderer and fails on any usvg warning. An icon paints its glyph in the text
color of the control around it.

`assets/icon/aria-icon.png` is the brand mark. `icons::brand_mark` draws it in
the window, Linux packages install it, `scripts/package-macos.sh` turns it
into `AppIcon.icns`, and the Xcode asset catalog under `apps/aria-ios-macos`
contains the resized copies.

`assets/generated/` contains the hand-authored 32 px workflow icon set added
in repository commit `9e55722`. The files use square line caps, a 2.5 px
stroke, and `currentColor`; no external generator or upstream icon package
produced them.

## Registry

### Window

Defined in `crates/aria-ui/src/chrome.rs` and `chrome/tool_strip.rs`.

| Constant | File | Use |
| --- | --- | --- |
| `BRAND_MARK` | `assets/icon/aria-icon.png` | The brand mark, drawn by `icons::brand_mark` in the titlebar (`chrome.rs`) and at the top of the Home rail (`panels/home/rail.rs`). |
| `MINUS` | `assets/icon/shell/minus.svg` | Minimize the window. |
| `SQUARE` | `assets/icon/shell/square.svg` | Maximize or restore the window. |
| `CLOSE` | `assets/icon/shell/x.svg` | Close the window, a dock tab (`shell/dock_tabs.rs`), and a Viewer Source tab (`panels/preview_tabs.rs`); clear the timeline property search (`panels/timeline/header.rs`) and the Effects & Presets search (`panels/effects_browser.rs`). |
| `UPLOAD` | `assets/icon/shell/upload.svg` | Export, on the tool bar. |
| `WRENCH` | `assets/icon/timeline/wrench.svg` | Display settings: the Viewer's control row holds View > Viewer Display Settings under it, and the Timeline header holds View > Timeline Display Settings (`display_btn` in `chrome/tool_strip.rs`). |
| `SETTINGS` | `assets/icon/shell/settings.svg` | Drawn only by the control gallery (`crates/aria-ui-gallery`). |
| `FIT` | `assets/icon/fullscreen.svg` | Drawn only by the control gallery (`crates/aria-ui-gallery`). |

### Tools

Defined in `crates/aria-ui/src/tool_catalog.rs` and drawn on the tool bar.

| Constant | File | Use |
| --- | --- | --- |
| `POINTER` | `assets/icon/timeline/mouse-pointer-2.svg` | Select tool. |
| `CHEVRONS_RIGHT` | `assets/icon/timeline/chevrons-right.svg` | Track Select Forward tool, and a dock frame's tab overflow menu, which lists the tabs its tab row has no room for (`shell/frame_menu.rs`). |
| `CHEVRONS_LEFT` | `assets/icon/timeline/chevrons-left.svg` | Track Select Backward tool. |
| `SCISSORS` | `assets/icon/timeline/scissors.svg` | Razor tool. |
| `ZOOM_IN` | `assets/icon/timeline/zoom-in.svg` | Zoom tool, and the Viewer's zoom in step (`panels/preview.rs`). |
| `HAND` | `assets/icon/timeline/hand.svg` | Hand tool. |
| `SLIP` | `assets/icon/timeline/move-horizontal.svg` | Slip tool. |
| `SLIDE` | `assets/icon/timeline/columns-3.svg` | Slide tool. |
| `ROLL` | `assets/icon/timeline/grip-vertical.svg` | Rolling Edit tool. |
| `RATE` | `assets/icon/timeline/gauge.svg` | Rate stretch tool. |
| `RIPPLE` | `assets/icon/timeline/fold-horizontal.svg` | Ripple trim tool. |
| `PEN` | `assets/icon/inspector/spline.svg` | Pen tool. |
| `TYPE` | `assets/icon/library/text.svg` | Type tool. |

### Viewer

Defined in `crates/aria-ui/src/panels/preview.rs`, `preview_controls.rs`,
`preview_resolution.rs`, `preview/handles.rs`, and `preview/scopes.rs`.

| Constant | File | Use |
| --- | --- | --- |
| `PLAY` | `assets/icon/play.svg` | Play. |
| `PAUSE` | `assets/icon/pause.svg` | Pause, in place of Play during playback. |
| `SKIP_BACK` | `assets/icon/skip-back.svg` | Go to start. |
| `SKIP_FORWARD` | `assets/icon/skip-forward.svg` | Go to end. |
| `CHEVRON_LEFT` | `assets/icon/chevron-left.svg` | Step one frame back, and move a multicam angle one frame earlier on the group's clock (`panels/sidebar_multicam.rs`). |
| `CHEVRON_RIGHT` | `assets/icon/chevron-right.svg` | Step one frame forward, and move a multicam angle one frame later (`panels/sidebar_multicam.rs`). |
| `ZOOM_OUT` | `assets/icon/timeline/zoom-out.svg` | The Viewer's zoom out step. |
| `TRANSFORM` | `assets/icon/preview/scaling.svg` | The Transform handle set. |
| `CROP` | `assets/icon/preview/crop.svg` | The Crop handle set. |
| `MASK` | `assets/icon/preview/square-dashed.svg` | The Mask handle set. |
| `SCOPES` | `assets/icon/preview/activity.svg` | Open or close the scope strip. |
| `LOOP` | `assets/icon/timeline/repeat.svg` | Loop playback. |
| `PROXY` | `assets/icon/preview/replace.svg` | Decode through proxies where they exist, or through the original media. |

### Home

Defined in `crates/aria-ui/src/panels/home/rail.rs` and `panes.rs`.

| Constant | File | Use |
| --- | --- | --- |
| `PLUS` | `assets/icon/plus.svg` | The New rail tab and the New Project tile, and Add Track at the left of the timeline zoom bar (`panels/timeline/overview.rs`). |
| `FOLDER_OPEN` | `assets/icon/folder-open.svg` | The Open rail tab and the Open tile. |
| `CLAPPERBOARD` | `assets/icon/clapperboard.svg` | The Recents rail tab, and the Sample Project and Use Template… tiles (`panels/home/panes.rs`). |
| `IMAGE` | `assets/icon/library/image.svg` | The Samples rail tab, and the media pool's Tiles view (`panels/sidebar_pool_bars.rs`). |
| `SPARKLES` | `assets/icon/shell/sparkles.svg` | The Learn rail tab. |

### Timeline

Defined in `crates/aria-ui/src/surfaces/lane_header.rs`,
`panels/timeline/header.rs`, and `components.rs`.

| Constant | File | Use |
| --- | --- | --- |
| `EYE` | `assets/icon/timeline/eye.svg` | A shown lane. |
| `EYE_OFF` | `assets/icon/timeline/eye-off.svg` | A hidden lane. |
| `VOLUME` | `assets/icon/timeline/volume-2.svg` | An audible lane. |
| `VOLUME_X` | `assets/icon/timeline/volume-x.svg` | A muted lane, and the Viewer's shuttle badge, which a press returns to 1x (`panels/preview_shuttle.rs`). |
| `HEADPHONES` | `assets/icon/timeline/headphones.svg` | Lane solo. |
| `LOCK` | `assets/icon/timeline/lock.svg` | A locked lane. |
| `LOCK_OPEN` | `assets/icon/timeline/lock-open.svg` | An unlocked lane. |
| `CROSSHAIR` | `assets/icon/timeline/crosshair.svg` | Lane target. |
| `LINK` | `assets/icon/timeline/link.svg` | A lane with sync lock on, and linked selection in the Timeline header. |
| `UNLINK` | `assets/icon/timeline/unlink.svg` | A lane with sync lock off. |
| `MAGNET` | `assets/icon/timeline/magnet.svg` | Snapping, in the Timeline header. |
| `FLAG` | `assets/icon/timeline/flag.svg` | Add a marker at the playhead, or open Marker Edit for the marker already on the playhead's frame. |
| `GRAPH` | `assets/icon/timeline/chart-spline.svg` | Open or close the Graph Editor. |
| `CHEVRON_DOWN` | `assets/icon/ui/chevron-down.svg` | Every disclosure: a lane, a property group, an effect's parameter group (`components::disclosure_button`), an Effects & Presets folder, and a bin row. `components::chevron` turns it a quarter turn for a shut row. It also ends a chip that opens a list, such as the Viewer's resolution menu (`components::list_chip`), and moves an effect or an audio insert one place later. |

### Media pool

Defined in `crates/aria-ui/src/panels/sidebar_pool_bars.rs`,
`sidebar_multicam.rs`, and `surfaces/bin_strip.rs`.

| Constant | File | Use |
| --- | --- | --- |
| `IMPORT` | `assets/icon/library/import.svg` | Import: the file picker, filtered to the formats the pool reads. |
| `AUDIO_LINES` | `assets/icon/timeline/audio-lines.svg` | Audio only, beside the search field, which lists the pool's audio sources; and sync a multicam group by the sound its cameras recorded (`panels/sidebar_multicam.rs`). |
| `LIST` | `assets/icon/library/list.svg` | The pool's List view. |
| `LAYOUT_GRID` | `assets/icon/library/layout-grid.svg` | The pool's Icons view. |
| `MOVE` | `assets/icon/library/move.svg` | The pool's Freeform view. |
| `FOLDER_PLUS` | `assets/icon/library/folder-plus.svg` | New bin, on the folder strip. |
| `LAYERS` | `assets/icon/library/layers.svg` | Create a multicam group (`panels/sidebar_multicam.rs`). |

### Inspector

Defined in `crates/aria-ui/src/panels/inspector/tabs/effects.rs`.

| Constant | File | Use |
| --- | --- | --- |
| `ROTATE_CCW` | `assets/icon/inspector/rotate-ccw.svg` | Reset an effect, the transform (`tabs/transform.rs`), a color wheel (`blocks/wheels.rs`), and a property row (`blocks/widgets_keys.rs`, `panels/timeline/property_rows.rs`). |
| `ELLIPSIS` | `assets/icon/inspector/ellipsis.svg` | An effect's menu. |
| `TRASH` | `assets/icon/inspector/trash-2.svg` | Remove an effect, and remove an audio insert (`panels/insert_chain.rs`). |
| `CHEVRON_UP` | `assets/icon/ui/chevron-up.svg` | Move an effect one place earlier, and move an audio insert one place earlier (`panels/insert_chain.rs`). |
| `STAR` | `assets/icon/ui/star.svg` | Favorite an entry in Effects & Presets (`panels/effects_browser.rs`). |

## Text labels

Timecode separators, the zoom readout, and word buttons in dialogs (Choose…,
Cancel, Export, Done, New Project, Import) are text and have no icon.
