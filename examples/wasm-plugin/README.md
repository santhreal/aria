# `com.example.wasm-tools` (`examples/wasm-plugin`)

WebAssembly component plugin package (`Tier::Wasm`) targeting the
`aria:plugin/plugin@1.0.0` WIT world (`crates/aria-plugin-wasm/wit/plugin.wit`).
Mounting it registers:

- Commands `example.add-marker` (with SVG icon `icons/marker.svg` and
  `enablement = "!playing"`), `example.inspect-and-undo`,
  `example.query-project`, and `example.storage-roundtrip`, plus
  fault-injection commands used by the host test suite.
- Timeline importer and exporter `example.markers` (`.mrk` marker files).
- Media encoder `example.raw-rgba` (`.rgba` frame and audio stream).
- Stereo audio effect `example.gain` (`WASM Gain`, with sidechain input).
- Sidebar panel `example.tools-panel` (`UiNode` arena tree and `PanelEvent`
  handling).
- Viewer overlay `example.safe-frame` (`OverlayShape` vector guides and
  draggable `focus` handle).
- Background jobs started by `example.count-job` and `example.endless-job`.
- Media decoder for 8-bit 4:2:0 YUV4MPEG2 (`.y4m`) files (`src/y4m.rs`).
- AI tools `example_add_marker` (Workspace: adds a marker through
  `project-transact`) and `example_swatch` (ReadOnly: returns a 1x1 PNG).
- Event subscriptions for `"command-applied"`, `"playhead-moved"`,
  `"command-proposed"`, and `"resolve-dropped"`.

## Build, validate, pack, and install

Build the WebAssembly component (a `wasm32-unknown-unknown` core module
wrapped by the `bundle-component` binary), validate the package, and write the
`.ariaplugin` archive:

```text
./examples/wasm-plugin/rebuild.sh
aria plugin check examples/wasm-plugin
aria plugin pack examples/wasm-plugin
```

Or run the live development loop directly:

```text
aria plugin dev examples/wasm-plugin
```

Install the generated `com.example.wasm-tools-0.1.0.ariaplugin` archive from
**File > Install Plugin...**, **Settings > Plugins > Install from File...**, or
by dropping the archive onto the editor window.
