# Aria specification

This specification defines the editor Aria is being built into: an After
Effects style workspace and animation model over a Premiere Pro style track
timeline and playback engine. Each chapter covers one concern. Every
chapter describes the target behavior; [status.md](status.md) compares the
current build with it, section by section.

Where a chapter and a reference document under `docs/` disagree, the reference
document describes the current build and this specification describes the
target.

## Chapters

| Chapter | Concern |
| --- | --- |
| [layout.md](layout.md) | Window, frames, docking, workspaces, menus, tool bar |
| [look.md](look.md) | Themes, density, type, icons, motion |
| [viewer.md](viewer.md) | Program and Source viewers, overlays, on-canvas controls |
| [timeline.md](timeline.md) | Timeline anatomy, track headers, clip bars, property rows |
| [editing.md](editing.md) | Tools, selection, trimming, three-point editing, snapping |
| [animation.md](animation.md) | Properties, keyframes, interpolation, graph editor, drivers |
| [playback.md](playback.md) | Playback engine, resolution, caching, shuttle, sync |
| [audio.md](audio.md) | Waveforms, levels, meters, mixer, audio effects |
| [panels.md](panels.md) | Every panel other than the viewer and timeline |
| [keyboard.md](keyboard.md) | The shipped key map |
| [plugins.md](plugins.md) | Plugin tiers, host API, contribution points, lifecycle |
| [performance.md](performance.md) | Latency and throughput targets, and how each is measured |
| [status.md](status.md) | The current build against this specification |

## Terms

| Term | Meaning |
| --- | --- |
| Sequence | A timeline: a frame size, a frame rate, an audio layout, and tracks. |
| Track | A horizontal lane in a sequence. Video tracks stack upward, audio tracks stack downward. |
| Clip | An item on a track with a source, an in point, an out point, and a position. |
| Layer | A clip seen through its properties: transform, effects, masks, audio. |
| Property | A value on a clip that can hold keyframes. |
| Frame | A panel container in the window. A frame holds one or more panels as tabs. |
| Panel | A dockable surface: Project, Effect Controls, Viewer, Timeline, and the rest. |
| Workspace | A named arrangement of frames and panels. |
| Work area | The sequence In and Out points. Rendering, looping, and export read it. |
| Plugin | A unit of code that adds capability through the host API in [plugins.md](plugins.md). |
