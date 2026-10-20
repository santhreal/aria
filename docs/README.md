# Documentation index

Reference first, then design notes. A document owns one concern; if a claim
appears twice, the document listed here for that concern is the one to edit.

## Using Aria

| Document | Concern |
| --- | --- |
| [install.md](install.md) | Per-platform install, codec support, build from source |
| [features.md](features.md) | What the editor does, and what it does not |
| [getting-started.md](getting-started.md) | Every surface, in the order a new project reaches it |
| [keyboard-shortcuts.md](keyboard-shortcuts.md) | Every registered chord |
| [command-line.md](command-line.md) | The headless CLI |
| [catalogs.md](catalogs.md) | Effect, transition, filter, and animation data files |
| [reference/README.md](reference/README.md) | Built-in effects, generators, transitions, audio effects, filters, animations, stickers, masks, tools, workspaces, panels, menus, commands, settings, and named controls |

## Extending Aria

| Document | Concern |
| --- | --- |
| [architecture.md](architecture.md) | Crates, the UI boundary, decode and GPU paths |
| [plugins/README.md](plugins/README.md) | Registries a plugin contributes to |
| [theme-tokens.md](theme-tokens.md) | Colour and spacing tokens |
| [animation-catalog.md](animation-catalog.md) | Motion specs and their call sites |
| [icons.md](icons.md) | Icon names and text placeholders |

## Specification

The target editor, chapter by chapter. [spec/status.md](spec/status.md)
compares the current build with it.

| Document | Concern |
| --- | --- |
| [spec/README.md](spec/README.md) | Index, terms, and every chapter |
| [spec/plugins.md](spec/plugins.md) | Plugin tiers, host API, contribution points |
| [spec/playback.md](spec/playback.md) | Playback engine and caching |
| [spec/performance.md](spec/performance.md) | Targets and measurement scenarios |
| [spec/keyboard.md](spec/keyboard.md) | Target key map |

## Design notes and roadmaps

These describe intent, not shipped behaviour. A claim here is not a feature.

| Document | Concern |
| --- | --- |
| [ai-agent-roadmap.md](ai-agent-roadmap.md) | Planned assistant capability |
| [mcp-design.md](mcp-design.md) | Model Context Protocol surface, planned |
| [cloud-roadmap.md](cloud-roadmap.md) | Planned cloud catalog work |
| [lottie-design.md](lottie-design.md) | Planned Lottie import |
| [deferred.md](deferred.md) | Items parked with the reason, and what would land them |
