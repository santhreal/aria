# Settings and storage

## Canvas

Settings holds a Canvas section while a project is open. Pick an aspect:
`auto` follows the largest video on the timeline; the rest are fixed shapes.
Preview and export both follow the choice, and it is undoable.

The section also shows the size the project renders at and where it came from,
`explicit` or `derived`. Pick **Derived** to hand the size back to the aspect
(1080 on the shorter edge) or, under `auto`, to the largest video on the
timeline. Pick a baseline row, 720p through 4320p, to set the size in pixels.
The baseline is the shorter edge, so 1080p is 1920×1080 in a wide project and
1080×1920 in a vertical one. A baseline whose long edge the encoder cannot take
is not offered.

Changing the aspect keeps the baseline and reshapes the frame. Changing it to
`auto` clears the explicit size. Both are one undo step.

The section then holds the sequence frame rate: the rate now, one row per
offered rate from 23.976 through 60, and a mismatch line when the largest video
in the pool runs at another rate. Picking a rate retimes every placement on the
timeline so each clip keeps the time it occupied, along with fades, transitions,
markers, and keyframes, so an animation runs over the same seconds it did
before. It is one undo step.

Then the audio layout: Mono, Stereo, or 5.1 Surround. The layout sets how many
channels the sequence mixes to and what export writes.

Last is the time display: **Timecode** (`00:00:12:04`), **Frames** (`292`), or
**Feet + Frames 35mm** (`0018+04`, sixteen frames to the foot). The timeline
ruler, the timecode over the track headers, the Viewer's playhead field, and the
trim readout all read in it. It is saved with the sequence and is one undo step.

## Provider keys

Settings has a Provider keys section listing every BYOK provider Aria can use
(Pexels, Pixabay, ElevenLabs, fal), with what each one powers and where its
key comes from. A provider with no key is listed as `Not set` rather than omitted.

Set a key in `~/.aria/config.toml`:

```toml
[providers.pexels]
api_key_env = "PEXELS_API_KEY"

[providers.fal]
api_key = "..."
```

`api_key_env` specifies an environment variable and is preferred. A variable that is not
set counts as no key, and the section states that.

## Storage

Settings holds a Storage section. It shows the storage root, the download cache
size against its quota, and how many cached files are pinned because an open
project uses them.

- **−512 MiB** / **+512 MiB** change the quota. The new quota is written to
  `~/.aria/config.toml` and enforced at once, so lowering it evicts the
  least-recently-used files down to the new figure. A value outside the accepted
  range is rejected and states the range.
- **Clear cache** deletes every unpinned cached file and reports how much it removed
  and how many files it kept.
- **Move storage…** picks a new root, moves the cache into it, and records the root
  for the next launch. The destination must not already exist. A failed move leaves
  everything where it was.

Cached downloads are re-fetchable, so clearing them costs bandwidth, not work.

The **Maintenance** row reads `held` while a cloud download is in flight. Clear and
Move are refused then: either one would delete or move the partial file the download
is still writing. It returns to `available` when the download ends, whether it
succeeded or failed.

## Caches

Settings holds a Caches section listing every cache the app keeps: preview frames,
library thumbnails, timeline filmstrips and waveforms, proxies, render files,
analysis state, AI model weights, downloads, catalog data, LUTs, Lottie assets,
and templates.

Each row states where the bytes are stored (Memory or Disk), what is lost by clearing it
(regenerated locally, downloaded again, or your own data), when it is evicted
relative to the others, and its resolved directory.

Library thumbnails outlive the session. The resting tile picture of each pool item
is written to disk once, keyed by the file's path, length and modification time, so
reopening a project draws its pool without decoding anything. A file whose size or
header does not match is refused and the frame is decoded again, and a scrubbed or
monitor-sized frame is never written: one picture per source per tile size.

- **Measure** walks every cache directory and fills in the sizes and file counts. It
  is a button rather than automatic because the walk is the slow part; sizes stay as
  measured until you press it again.
- **Copy paths** puts the storage root and every resolved cache path on the
  clipboard, for a bug report.
- **Clear** empties one cache's directory and reports what it removed. A memory cache
  has no Clear: there is nothing on disk to remove, and the row states that.
- **Move…** moves that cache's files to a directory you pick and then records the new
  path. The files are copied or renamed first; the path is written only after they
  arrive, so a failed move leaves the old location serving.
- **Redirect…** points the cache at a directory without moving what is already there.
- **Use default** appears on a redirected cache and returns it to the storage root.

Per-cache paths are stored under `[storage.paths]` in `config.toml`. A relative path
is refused, and so is one that would put two caches in the same directory: one
cache's Clear would delete the other's files.

A hand-edited value Aria cannot use keeps its default and is listed in the
Caches section as **Config refused**, stating the key, the value, and the reason.
**Copy paths** puts the same lines in the report. Four reasons are refused this
way: a value of the wrong kind, a name outside the set a key accepts, a relative
path, and a number outside a key's range. Aria still starts.

## Plugins

Every capability of the app is a plugin: storage, settings, decoding, encoding,
compositing, effects, transitions, generators, rendering, the engine, cloud,
analysis, jobs, transcription, and the assistant. Settings holds a Plugins
section listing all of them in mount order.

A row states the plugin, its version, and the services it registered. A plugin that
did not mount shows the reason instead: a missing GPU, an unreadable config file, a
dependency that failed first. A failed plugin does not stop the app: the rest mount
and the capability is absent, not broken.

The switch at the right of a plugin's action line turns it off or on. Off drops
the plugin's services from the registry, and the surfaces that need them
disappear; on mounts it again. Storage, settings, and the engine have no
switch: Aria cannot run without them.

The first two rows of the section are the limits a WASM plugin runs under, each
with **Halve** and **Double** steppers: `WASM fuel per call` (500,000,000
instructions) and `WASM memory` (64 MiB). They persist to the `[plugin_limits]`
table in `config.toml` and are clamped to 1,000,000 to 100,000,000,000
instructions and 16 to 4,096 MiB. A change remounts every mounted WASM plugin
under the new limits; one that does not mount under them is listed as failed.

## AI

Settings holds an AI section.

- **Source** picks **Local** (an OpenAI-compatible server such as Ollama or LM Studio),
  **OpenRouter**, or **Custom** for any other endpoint.
- **AI base URL**, **AI model**, **AI key**, and **AI key variable** are typed
  fields. Click one to edit it, press Enter to save, and Escape to leave the saved
  value alone. A URL must start with `http://` or `https://`. A key variable specifies an
  environment variable holding the key, which is preferred over storing the key
  itself; a stored key is shown as its length, never its text.
- **Protocol** picks **Chat Completions** or **Responses**, and **Reasoning** picks
  **Auto** or **Off** for whether a Responses model requests a reasoning summary.
  Local and OpenRouter always use Chat Completions.
- **Autonomy** picks **Ask** or **Full**: whether destructive assistant tools need a
  confirmation.
- **Status** is the same check the assistant runs. It states what is missing (no base
  URL, an unsupported model, a key variable that is not set) rather than reporting a
  bare "not configured". No request is made; whether the endpoint answers is found out
  when a prompt runs.
- **Limits** shows the caps one prompt is held to: edit-tool calls, host-tool calls,
  provider turns, and images. They come from the AI plugin, so with that plugin
  disabled the row states that instead of listing a cap nothing enforces.
- **Can do** counts the tools a prompt is offered: the validated edit vocabulary,
  then the app tools, split into the ones that only read and the ones that prompt
  first. The counts are taken from those tables, not written here, so they cannot
  claim a capability this build does not have.
- **Active** lists which parts of a turn are switched on right now: edits, senses,
  reasoning when a summary is requested, and whether a destructive call prompts.
- **Agent files** shows where skills, rules, and slash commands are stored, and **Reveal
  agent files** opens it.

## Workspaces

Aria area is a tree of splits and tabbed frames. Each frame has a 24 px
tab bar at the top. Click a tab to activate it, click `×` or middle-click to
close it, drag a tab within its bar to reorder it, drag a tab onto another
frame's center or edge (`Left`, `Right`, `Top`, `Bottom`) to dock or split it,
or drag a tab outside the main window (or choose **Undock Panel** from the frame
menu `≡`) to float it in a separate window. A floating window's **Dock** puts
its panels back in their default frames; **Close Window**, or the window
manager's close control, closes them, and **Window** opens each one again.
Double-click a tab bar or press `` ` `` to maximize the focused frame, and
repeat to restore the split tree. **Add Panel** in the frame menu adds a panel
the frame does not hold as a new tab.

A workspace is a named arrangement of frames, tabs, and pane sizes. Five ship:
Editing, Animation, Color, Audio, and Effects; see
[Workspaces](../reference/workspaces.md) for their frame trees and dimensions.

Pick one from **Window > Workspaces**, from the **Workspaces** section of **Settings**, by <kbd>Shift+Alt+1</kbd> (`key-workspace-1`) through
<kbd>Shift+Alt+9</kbd> (`key-workspace-9`), or from the command palette by name. Edits made to a workspace's
frames and splits are saved for that workspace; **Window > Workspaces > Reset to Saved
Layout** restores the workspace's default arrangement. Double-clicking a splitter
restores that split's saved workspace ratio.

**Window > Workspaces > Save Workspace** saves the current arrangement to the **Custom** slot
and opens the inline **Save workspace as** prompt; committing a name writes
`<data dir>/workspaces/<name>.json`. The Custom row appears on the Window menu
once saved, and **Forget** in the **Workspaces** section of **Settings** removes it again.
The **Workspaces** section of **Settings** lists every workspace with its measurements, and marks the
workspace last chosen, as the tool bar's workspace switcher and the Window menu
do. A fresh session marks Editing. The workspace keeps the dividers you drag and
the panels you move, so it stays marked through them and through a click on
another panel.

## Finding a setting

The box at the top of Settings filters the sections by name. It narrows as you type,
Escape clears it, and it is never written to the config file.

In the Appearance section, **System**, **Light**, and **Dark** choose the
appearance, **Accent** chooses the family used for the accent, the playhead,
the selection, and the caret (Blue, Ember, Teal, or Graphite), and **Theme**
chooses a theme file from `~/.aria/themes`, a theme a
plugin installed, or the shipped palette (see [Theme files](../theme-tokens.md#theme-files)). The accent choice
persists in `[appearance]`, the theme in `ui-settings.json`, and both apply
immediately.

The Cloud section holds the backend base URL. Left empty it uses the shipped
default.

In the Timeline section, **Zoom bar knobs** sets what a drag on a knob of the
timeline's zoom bar holds still. **Hold Playhead**, the default, keeps the
playhead at its place in the view while the playhead is inside the view;
**Hold Other End** keeps the other end of the view in place (see
[Timeline header](timeline-and-editing.md#timeline-header)). The choice
persists in `ui-settings.json` and applies from the next press on a knob.

## Checking for updates

**Help > Check for Updates** queries the cloud endpoint for the newest published
version. The status line reports either that the installed version is current, or
which version is newer. A failed check states the failure and still reports the
installed version.


## Configuration files and schema versions

Settings are stored in `~/.aria/config.toml`. UI chrome widths persist in
`~/.aria/ui-settings.json`. Recents persist in `~/.aria/recents.json`.

Each table in `config.toml` holds a schema version, recorded in one table:

```toml
[schema]
ai = 2
appearance = 1
```

A missing key keeps its default, so a file an older build wrote still opens. A
table recording an older version is migrated to the current shape, and the
current version is recorded on the next save. A table recording a version this
build does not read is rejected: the load fails stating the table and both
versions, the settings fall back to defaults, and a save leaves the file in
place instead of rewriting a meaning it cannot read.

Two tables have changed meaning so far. `[ai]` version 1 had no `source` key and
read a base URL with a model as the freeform endpoint; version 2 reads an absent
`source` as the local server. `[cloud]` version 1 kept the API host in
`[account].base_url`; version 2 keeps it in `[cloud].base_url`.


## Example

Configure project canvas settings, measure and clear a storage cache, and save a custom workspace arrangement:

1. Choose **File > Project Settings…** or **Sequence > Sequence Settings…** (or press <kbd>Ctrl+,</kbd> (`key-settings`) / choose **Edit > Preferences…**). The Settings dialog opens on the **Canvas** section.
2. Pick an aspect ratio and frame rate for the open sequence, then switch to the **Caches** section and click [`caches-measure`](../reference/controls.md#settings) to compute disk usage across all cache directories. Each cache row fills in its measured byte size and file count.
3. Click [`cache-clear`](../reference/controls.md#settings) on an unpinned cache row (or click [`storage-clear`](../reference/controls.md#settings) in the **Storage** section and adjust the download quota with [`quota-dec`](../reference/controls.md#settings) or [`quota-inc`](../reference/controls.md#settings)), then click [`settings-done`](../reference/controls.md#settings) to close Settings. The status line reports the reclaimed bytes.
4. Drag panel tabs or splitters to arrange the dock layout, then choose **Window > Workspaces > Save Workspace** (or click [`settings-workspace-save`](../reference/controls.md#settings) in the **Workspaces** section of Settings). Aria writes the layout to `<data dir>/workspaces/` and enables the **Custom** workspace slot; choosing **Window > Workspaces > Reset to Saved Layout** restores the saved arrangement at any time.

## Reference

See [Settings](../reference/settings.md), [Workspaces](../reference/workspaces.md), and [Controls](../reference/controls.md#settings) for every `config.toml` key, `ui-settings.json` field, and Settings control.
