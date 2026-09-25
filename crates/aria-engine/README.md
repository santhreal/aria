# aria-engine

`aria-engine` is the headless editing engine for Aria. It owns an editable project session, applies structured commands, records undo/redo history, produces preview frames, and exports timelines.

This crate is the main integration point for non-UI callers. The desktop editor, smoke-test CLI, tests, and AI bridge all rely on the engine instead of mutating project state directly.

## Responsibilities

- Create and manage an `Engine` session.
- Apply `aria-commands` values against the current `aria-models` project.
- Validate edits that depend on current timeline state.
- Build inverse commands for undo/redo.
- Import media through probing and metadata refresh.
- Resolve timeline content into compositing layers.
- Rasterize generated text, solids, and shapes for preview and export.
- Decode media frames and use the on-disk frame cache for preview.
- Produce `RgbaFrame` preview frames.
- Export timelines to MP4 through `aria-encoder`.

## Main APIs

- `Engine`: session object for applying commands and reading project state.
- `EngineConfig`: cache and runtime configuration.
- `ApplyOutcome`: result of applying a command.
- `ExportSettings` and `ExportProgress`: export configuration and progress reporting.
- `export_project`, `export_timeline`, and related helpers: direct export entry points.
- `bake_project` and `proxy_project`: one clip, or one pool source, as a project of its own, so an ordinary export renders it.
- `BakePass` and `BakeMedium`: that project plus which medium it renders to, picture or sound, decided by the lane's own kind row.
- `RgbaFrame`: preview frame returned by the engine.

## How it fits

`aria-engine` depends on most lower-level crates:

- `aria-models` for project and timeline state.
- `aria-commands` for edit requests.
- `aria-probe` and `aria-decoder` for media metadata and frames.
- `aria-cache` for decoded frame storage.
- `aria-compositor` for GPU compositing.
- `aria-encoder` for final MP4 output.

The engine should stay UI-agnostic. Panels, widgets, file dialogs, and interaction state belong in `aria-ui`.

## Testing and benchmarks

Run the engine tests with:

```bash
cargo test -p aria-engine
```

Run local preview and export benchmarks with:

```bash
cargo bench -p aria-engine --bench preview
cargo bench -p aria-engine --bench export
```

Engine tests should cover both the forward edit and the inverse edit when behavior is undoable.
