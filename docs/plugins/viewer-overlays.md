# Viewer overlays

A viewer overlay draws over the sequence frame in the Viewer: guides, a
tracked region, a motion path, labels, and draggable handles. A plugin
returns an `OverlayScene`, and the Viewer sends each drag of one of its
handles back as an `OverlayEvent`.

```rust
use parking_lot::Mutex;

use aria_plugin::{
    HandleLook, OverlayColor, OverlayEvent, OverlayScene, OverlayShape, OverlaySpec,
    PluginContext, ViewerOverlay,
};

/// A crosshair the user drags, in canvas pixels.
struct Crosshair(Mutex<[f32; 2]>);

impl ViewerOverlay for Crosshair {
    fn scene(&self, _: &PluginContext) -> OverlayScene {
        let [x, y] = *self.0.lock();
        let accent = OverlayColor::Accent;
        OverlayScene {
            shapes: vec![
                OverlayShape::Line { from: [x - 40.0, y], to: [x + 40.0, y], color: accent, width: 1.0 },
                OverlayShape::Line { from: [x, y - 40.0], to: [x, y + 40.0], color: accent, width: 1.0 },
                OverlayShape::Text {
                    at: [x + 8.0, y + 8.0],
                    text: format!("{x:.0}, {y:.0}").into(),
                    color: OverlayColor::Text,
                    size: 12.0,
                },
                OverlayShape::Handle { id: "center".into(), at: [x, y], color: accent, look: HandleLook::Cross },
            ],
        }
    }

    fn event(&self, _: &PluginContext, event: &OverlayEvent) -> Result<(), String> {
        let OverlayEvent::Drag { at, .. } = event;
        *self.0.lock() = *at;
        Ok(())
    }
}

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_viewer_overlay(
        OverlaySpec {
            id: "example.crosshair".into(),
            title: "Crosshair".into(),
        },
        Crosshair(Mutex::new([960.0, 540.0])),
    )?;
    Ok(())
}
```

Shapes are placed in canvas pixels: the sequence frame's own coordinates,
origin at the top left, `x` right and `y` down. The Viewer maps them through
its fit, zoom, and pan. Stroke widths and text sizes are window pixels and keep
their size at any zoom. Overlays draw in registration order above the frame and
the transform handles, clipped to the picture area. Nothing is drawn over a
Source tab.

| Shape | Drawn as |
| --- | --- |
| `Line { from, to, color, width }` | A straight stroke |
| `Rect { origin, size, color, width, fill }` | An axis-aligned outline from its top-left corner, filled when `fill` is set |
| `Ellipse { center, radii, color, width, fill }` | An outline, filled when `fill` is set |
| `Path { points, closed, color, width }` | Straight segments through `points`, back to the first when `closed` |
| `Text { at, text, color, size }` | One line of text with its top-left corner at `at` |
| `Handle { id, at, color, look }` | A draggable `Dot`, `Square`, or `Cross`, 12 pixels across |

Lines, rectangles, ellipses, and paths paint in scene order; text and handles
draw above them. A colour is a theme colour (`Accent`, the default, `Text`,
`Muted`, `Success`, `Warning`, `Danger`) or `Rgba([r, g, b, a])`, straight-alpha
sRGB. In JSON a shape is tagged by `kind` (`"line"`, `"rect"`, `"ellipse"`,
`"path"`, `"text"`, `"handle"`), and an omitted field takes its default: colour
`"accent"`, width 1, no fill, text size 12, look `"dot"`.

A press on a handle sends `Drag { handle, phase: Start, at }`. Each pointer
step with the button held sends `Move`, and the release sends `End`, wherever
the pointer is. `at` is the pointer in canvas pixels and may lie off the frame.
The Viewer draws the scene the overlay returns after the event, so the plugin
moves the handle, constrains it, or snaps it. Steps queued while the handler
still runs merge into the latest, so a slow handler receives fewer steps
instead of falling behind.

`scene` and `event` run on the event delivery thread. The scene is built when
the Viewer first reads the overlay, after every event, and after
`ctx.refresh_viewer_overlay(id)`, which a plugin calls when its state changes
outside a drag. An overlay that follows the playhead or the selection
subscribes to `PlayheadMoved` or `SelectionChanged` and refreshes there.
Refreshes queued before a build starts share that build.

A press must name a handle of the scene on screen; otherwise
`send_overlay_event` returns `OverlayError::NoHandle` and the status bar shows
it. A plugin sends events to and refreshes only its own overlays. An `Err` from
`event` is shown in the status bar after the overlay's title. A scene that
fails `OverlayScene::check` is not drawn: the overlay keeps its last scene, the
status bar shows why, and the refusal is recorded as a fault of the plugin.
`check` refuses:

- more than `MAX_SHAPES` (4096) shapes, or more than `MAX_PATH_POINTS` (65 536)
  points across all paths;
- a point that is not finite or lies more than `MAX_COORDINATE` (1 000 000)
  canvas pixels from the origin, and a size or radius that is negative, not
  finite, or over 2 000 000;
- a stroke width not above 0 or over `MAX_STROKE` (32), and a path of fewer
  than 2 points;
- text over `MAX_TEXT_BYTES`, holding a line break, or sized outside
  `TEXT_SIZE_RANGE` (6 to 96), and more than `MAX_TREE_TEXT_BYTES` of text in
  the scene;
- an empty handle id, one over `MAX_WIDGET_ID_BYTES` or holding a control
  character, and two handles sharing an id.

A panic in `scene` or `event` fails the plugin for the session, and its
overlays leave the Viewer with its other registrations.
`register_viewer_overlay` returns `PluginError::OverlayRefused(message)` when
the id or title is empty or the id is already registered.

## WebAssembly and native C ABI plugins

A WebAssembly component registers an overlay during `mount` with `register-viewer-overlay(overlay-spec { id, title })`, builds the shape list in `overlay-scene(id) -> list<overlay-shape>`, handles pointer drags in `overlay-event(id, event) -> result<_, string>`, and schedules rebuilds with `refresh-viewer-overlay(id)`:

```rust
register_viewer_overlay(&OverlaySpec {
    id: "example.safe-frame".to_owned(),
    title: "Safe Frame & Focus".to_owned(),
})?;
```

A native `cdylib` plugin built with `aria-sdk` registers an overlay during `Plugin::mount` with `host.register_viewer_overlay(id, title)`, implements `Plugin::overlay_scene` and `Plugin::overlay_event` with `aria_sdk::OverlayScene`, `OverlayShape`, and `OverlayEvent`, and schedules rebuilds with `host.refresh_viewer_overlay(id)`:

```rust
impl aria_sdk::Plugin for MyPlugin {
    fn mount(&self, host: &aria_sdk::Host) -> Result<(), String> {
        host.register_viewer_overlay("my.overlay", "My Overlay")
    }

    fn overlay_scene(&self, _host: &aria_sdk::Host, _id: &str) -> aria_sdk::OverlayScene {
        aria_sdk::OverlayScene::new(vec![aria_sdk::OverlayShape::Handle {
            id: "focus".into(),
            at: [960.0, 540.0],
            color: aria_sdk::OverlayColor::Accent,
            look: aria_sdk::HandleLook::Cross,
        }])
    }

    fn overlay_event(
        &self,
        host: &aria_sdk::Host,
        id: &str,
        _event: &aria_sdk::OverlayEvent,
    ) -> Result<(), String> {
        host.refresh_viewer_overlay(id)
    }
}
```
