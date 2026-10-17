# `example.notes` (`examples/notes-plugin`)

Native C ABI plugin package (`Tier::Native`) built as a `cdylib` over
`aria-sdk`. Mounting it registers:

- Command `notes.count` (`Record seen command count`).
- Sidebar panel `notes.panel` rendered as a declarative `UiNode` tree with
  button, toggle, and text-input controls (`PanelEvent::Pressed`, `Toggled`,
  `Submitted`).
- Subscriptions to `"resolve-dropped"` (claiming `.srt` files as
  `DropAction::ImportMedia` and `https://notes.example/` URLs as
  `DropAction::Handled`) and `"command-proposed"` (counting proposed
  commands and rejecting edits that target more than eight tracks).
- Storage reads and writes under `<data dir>/plugin-data/example.notes`.

## Build, validate, pack, and install

Run in the live development loop or build a release `.ariaplugin` archive:

```text
aria plugin dev examples/notes-plugin
aria plugin check examples/notes-plugin
aria plugin pack examples/notes-plugin
```

`aria plugin pack` compiles the `cdylib`, stages `lib/<os>-<arch>.<so|dylib|dll>`,
runs the four package checks (`manifest`, `shaders`, `permissions`, `abi`), and
writes `examples/notes-plugin/example.notes-0.1.0.ariaplugin`. Install the
archive from **File > Install Plugin...**, **Settings > Plugins > Install from
File...**, or by dropping the archive onto the editor window.
