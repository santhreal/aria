# Panels and dialogs

## Panels

Every panel the editor draws is registered, including the shipped ones.
`aria.ui` mounts a `PanelHost` and adds the Project panel, the Timeline, and
Effect Controls to it; the shell then resolves each slot from that host. A plugin
contributes a panel to the same host:

```rust
use aria_ui::dock::DefaultFrame;
use aria_ui::plugins::panel_host::{host_of, ALWAYS, DockSpec, PanelEntry};
use aria_ui::types::ActivePanel;

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    if let Some(host) = host_of(ctx) {
        host.add(PanelEntry {
            id: "example.panel.notes",
            title: "Notes",
            location: PanelLocation::Inspector,
            render: draw_notes,
            visible: ALWAYS,
            dock: Some(DockSpec::new(
                DefaultFrame::Right,
                DockSpec::DEFAULT_MIN_SIZE,
                ActivePanel::Inspector,
            )),
        });
    }
    Ok(())
}
```

`render` is an `aria-ui` function pointer, so `aria-plugin` stays free of GPUI.
Registering an id that already exists replaces that panel, which is how a plugin
overrides a built-in. Mount after `aria.ui` to reach the host.

A slot holds the panels in registration order, stacked. `Overlay` has no
built-in entry: an overlay contribution is drawn over the viewer.

### Body slots

`Viewer` and `Home` are slots whose entries are alternatives rather than a
stack. The shipped set puts the preview and the assistant in `Viewer` and the
launch screen in `Home`. `visible`, a `fn(&AppState) -> bool` the entry
holds, selects which one draws:

```rust
host.add(PanelEntry {
    id: "example.panel.scope",
    title: "Scope",
    location: PanelLocation::Viewer,
    render: draw_scope,
    visible: |state| state.active_panel == ActivePanel::Preview,
    dock: DockSpec::for_location(PanelLocation::Viewer),
});
```

A docked pane uses `ALWAYS`. Two entries whose predicates hold at once both
draw, so a replacement body must either take over an existing id or narrow its
own predicate to a state no shipped entry claims.

### Declared panels

A plugin that does not link `aria-ui` declares a panel as a `UiNode` tree. The
window draws the tree with the editor's own controls and sends each interaction
with a control back to the plugin as a `PanelEvent`. Native and WASM plugins
declare panels the same way through their own tiers ([Native
plugins](native.md), [WebAssembly plugins](wasm.md)). In `aria-sdk`, a panel
implements `aria_sdk::Panel` (also exported as `PanelView`) with
`view(&self, host) -> UiNode` and `event(&self, host, event)`, and
`Host::register_panel(PanelSpec, panel)` registers it. The SDK `UiNode`,
`PanelEvent`, and `Tone` have the variants of the types on this page; the SDK
`PanelLocation` is `Inspector`, `Sidebar`, or `Timeline`.

```rust
use parking_lot::Mutex;

use aria_plugin::{PanelEvent, PanelLocation, PanelSpec, PanelView, PluginContext, UiNode};

struct Counter(Mutex<u32>);

impl PanelView for Counter {
    fn view(&self, _: &PluginContext) -> UiNode {
        let count = *self.0.lock();
        UiNode::column([
            UiNode::field("Presses", count.to_string()),
            UiNode::button("press", "Press"),
        ])
    }

    fn event(&self, _: &PluginContext, event: &PanelEvent) -> Result<(), String> {
        if let PanelEvent::Pressed { .. } = event {
            *self.0.lock() += 1;
        }
        Ok(())
    }
}

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_panel(
        PanelSpec {
            id: "example.counter".into(),
            title: "Counter".into(),
            location: PanelLocation::Sidebar,
        },
        Counter(Mutex::new(0)),
    )?;
    Ok(())
}
```

The returned `Registration` is held on the terms of `register_effect`. A
declared panel registers at one of `DOCKED_LOCATIONS` (`Sidebar`, `Inspector`,
or `Timeline`) and becomes a movable dock tab in the corresponding default
frame (`Left`, `Right`, or `Bottom`) and an entry in the **Window** menu.

| Node | Drawn as | Event |
| --- | --- | --- |
| `Column`, `Row` | Children top to bottom; left to right, wrapping | |
| `Heading`, `Text`, `Field`, `Separator` | Title, paragraph in a `Tone`, label and value, rule | |
| `Button { id, label, disabled }` | Push button | `Pressed { id }` on click |
| `Toggle { id, label, on }` | Switch | `Toggled { id, on }` with the opposite of `on` |
| `Slider { id, label, value, min, max, step }` | Track and value | `Slid { id, value }` on release, snapped to `step` |
| `Input { id, value, placeholder }` | One-line text box | `Submitted { id, text }` on Enter; Escape sends nothing |
| `Choice { id, label, options, selected }` | A row of chips | `Chose { id, index }` |
| `Progress { label, fraction }` | Bar filled to `fraction` | |
| `Image { width, height, rgba }` | Straight-alpha RGBA8 pixels, narrowed to fit | |

A `Tone` is `Normal`, `Muted`, `Success`, `Warning`, or `Danger`, drawn in the
theme's text, muted text, success, warning, and danger colors. `Normal` is the
default.

`UiNode::column(children)`, `row(children)`, `heading(text)`, `text(text)`,
`toned(text, tone)`, `field(label, value)`, `button(id, label)`,
`toggle(id, label, on)`, `slider(id, label, value, min..=max)`,
`input(id, value, placeholder)`, `choice(id, label, options, selected)`, and
`progress(label, fraction)` build the nodes above. A built button is enabled,
and a built slider has a `step` of 0, which clamps without snapping.
`widget_id()` returns the id of a control node, and `find(id)` returns the
control with `id` in a subtree.

`view` and `event` run on the event delivery thread, never on the thread that
draws. The tree is built when the window first reads the panel, after every
event the panel receives, and after `ctx.refresh_panel(id)`, which a plugin
calls when its state changes outside an event. Refreshes queued before a build
starts share that build. The window shows `Loading` until the first tree lands.

An event is held to the tree on screen before the plugin receives it: a control
that is not in the tree, a press on a disabled button, a toggle sent to a
button, or a choice index past the options is refused with a `PanelError`, and
the window reports it. A slider value is clamped and snapped, and submitted
text stops at `MAX_TEXT_BYTES`. A plugin receives only values its own tree
could produce, and sends events to and refreshes only its own panels.

An `Err` from `event` is drawn in the panel until the next event the handler
accepts. A tree that fails `UiNode::check` is not drawn: the panel keeps its
last tree, shows why the new one was rejected, and the refusal is recorded as a
fault of the plugin. `check` rejects:

- nesting deeper than `MAX_TREE_DEPTH` (24) or more than `MAX_TREE_NODES` (4096)
  nodes;
- a string over `MAX_TEXT_BYTES` (16 KiB), or more than `MAX_TREE_TEXT_BYTES`
  (1 MiB) of text in the tree;
- an empty control id, one over `MAX_WIDGET_ID_BYTES` (128) or holding a
  control character, and two controls sharing an id;
- a slider with a non-finite bound, `min` not below `max`, a step outside
  `0..=max - min`, or a value outside its range;
- a choice with no options, more than `MAX_CHOICE_OPTIONS` (64), or a
  selection past them, and a progress fraction outside `0.0..=1.0`;
- an image side outside `1..=MAX_IMAGE_SIDE` (2048), a byte count other than
  `width * height * 4`, or more than `MAX_TREE_IMAGE_BYTES` (64 MiB) of pixels.

A panic in `view` or `event` fails the plugin for the session, and its panels
leave the window with its other registrations. The window decodes an image
once and reuses it while a rebuilt tree holds the same `Arc<[u8]>`, so a plugin
keeps the pixels it draws in an `Arc` rather than copying them each build.

`register_panel` returns `PluginError::PanelRefused(message)` when the id or
title is empty, the id is already registered, or the location is not docked.

A package lists its panels in `[[panels]]` with `id`, `title`, and `location`
(`sidebar`, `inspector`, or `timeline`). While the package waits for an
activation event, each is drawn as a card with an Open button that loads it
([Activation](activation.md)).

## Dialogs and overlays

Dialogs are registered the same way panels are. `aria.ui` mounts an
`OverlayHost` holding Export, Settings, the palette, Jobs, the context menu, and
the rename bar; the shell draws whatever the host has for the current state and
names no dialog module of its own.

```rust
use aria_ui::app::DialogKind;
use aria_ui::plugins::overlay_host::{host_of, OverlayEntry, OverlayWhen};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    if let Some(host) = host_of(ctx) {
        host.add(OverlayEntry {
            id: "example.dialog.notes",
            when: OverlayWhen::Dialog(DialogKind::Plugin("example.dialog.notes")),
            render: draw_notes_dialog,
        });
    }
    Ok(())
}
```

Open it with `state.open_dialog(DialogKind::Plugin("example.dialog.notes"))`
from a command, a menu item, or a key binding. One dialog is on screen at a
time, so opening yours closes whatever was up, and Escape closes it.

`OverlayWhen::ContextMenu` and `OverlayWhen::Rename` stack: everything
registered for them draws while that condition holds. A dialog does not stack;
the first entry registered for a kind is the one drawn.
