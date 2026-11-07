# Settings and storage

## Canvas

Settings holds a Canvas section while a project is open. The section holds five
groups of choices, each under a row that reads the value in force: **Aspect**,
**Size**, **Frame rate**, **Audio layout**, and **Time display**. Pick an aspect:
`auto` follows the largest video on the timeline; the rest are fixed shapes.
Preview and export both follow the choice, and it is undoable.

The **Size** row reads the size the project renders at and where it came from,
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
- **Move storage…** picks a new root, moves every cache that has no path of its own
  into it, and records the root for the next launch. The cache directories under the
  new root must not already exist. A refused move leaves everything where it was. A
  cache that fails partway stays where it was and is listed in the error.

Cached downloads are re-fetchable, so clearing them costs bandwidth, not work.

While a cloud download is in flight, **Clear cache** and **Move storage…** draw dimmed
with the arrow cursor: either one would delete or move the partial file the download
is still writing. A press posts `A download is in flight. Clear and Move run once it
finishes` and opens no folder picker. Both return when the download ends, whether it
succeeded or failed.

## Caches

Settings holds a Caches section listing every cache the app keeps: preview frames,
library thumbnails, timeline filmstrips and waveforms, proxies, render files,
analysis state, AI model weights, downloads, catalog data, and LUTs.

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
  has no Clear: there is nothing on disk to remove, and the row states that. Clear on
  Downloads keeps the files an open project uses.
- **Move…** moves that cache's files to a directory you pick and then records the new
  path. The files are copied or renamed first; the path is written only after they
  arrive, so a failed move leaves the old location serving.
- **Redirect…** points the cache at a directory without moving what is already there.
  Downloads has no Redirect; it changes directory only with **Move…**.
- **Use default** appears on a redirected cache and returns it to the storage root.
  On Downloads it moves the files back.

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

The switch at the right end of a plugin's row turns it off or on. Off drops
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
  value alone. Tab saves the field and selects the next one, Shift+Tab the one
  before; a refused value keeps the focus. A URL must start with `http://` or
  `https://`. A key variable specifies an environment variable holding the key,
  which is preferred over storing the key itself; a stored key is shown as its
  length, never its text.
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
  then the app tools (the loop's `describe_project`, `commit_progress` and
  `read_skill`, then the host tools listed in the
  [assistant guide](assistant.md#built-in-loop-and-host-tools)), with the
  read-only ones and the ones that prompt first in parentheses, then the plugin
  tools. The assistant's tool list draws the same tools, grouped. The counts are
  taken from those tables, not written here, so they cannot claim a capability
  this build does not have.
- **Active** lists which parts of a turn are switched on right now: edits,
  read-only tools, reasoning when a summary is requested, and whether a
  destructive call prompts.
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

**Window > Workspaces > Save Workspace** saves the current arrangement to the **Custom** slot.
There is one slot; saving again replaces it. The Custom row appears on the Window menu
once saved, and **Forget** in the **Workspaces** section of **Settings** removes it again.
The **Workspaces** section of **Settings** lists every workspace with its measurements, and marks the
workspace last chosen, as the tool bar's workspace switcher and the Window menu
do. A fresh session marks Editing. The workspace keeps the dividers you drag and
the panels you move, so it stays marked through them and through a click on
another panel.

## Finding a setting

The box at the top of Settings filters the sections by name. It narrows as you type,
Escape clears it, and it is never written to the config file. A built-in section's
whole title, such as `Canvas` or `Keyboard Shortcuts`, shows that section alone; any
other text shows each section whose title contains it and lists the key bindings
whose name or chord contains it. **Edit > Keyboard Shortcuts…** (Ctrl+Alt+K)
opens Settings with that section's title in the box, and **Edit > Preferences…**
(Ctrl+,) opens it with the box empty.

The Appearance section holds four groups, each under a row that reads its
current value. **Mode** chooses System, Light, or Dark. **Accent** chooses the
family used for the accent, the playhead, the selection, and the caret (Blue,
Ember, Teal, or Graphite). **Theme** chooses a theme file from
`~/.aria/themes`, a theme a plugin installed, or the shipped palette (see
[Theme files](../theme-tokens.md#theme-files)). **Motion** chooses Full, Brisk,
Relaxed, or Off (see [Animation catalog](../animation-catalog.md)). The accent
choice persists in `[appearance]`, the mode, theme, and motion in
`ui-settings.json`, and each applies immediately.

The Cloud section holds the backend base URL. Left empty it uses the shipped
default.

In the Timeline section, **Zoom bar knobs** sets what a drag on a knob of the
timeline's zoom bar holds still. **Hold playhead**, the default, keeps the
playhead at its place in the view while the playhead is inside the view;
**Hold other end** keeps the other end of the view in place (see
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

Aria saves `config.toml`, `ui-settings.json`, `recents.json`, and the
workspace, key set, preset, assistant conversation, and skill files under
`~/.aria` by writing a temporary file in the same folder, flushing it to disk,
and renaming it over the old file. An interrupted save leaves the previous copy
intact. `config.toml` is written with mode `0600`; every other file keeps its
existing mode. A file that is a symbolic link, such as a `config.toml` linked
from a dotfiles checkout, stays a link: the save replaces the file the link
points at, in that file's folder.

A `ui-settings.json` or `recents.json` that does not parse is renamed to
`<name>.unreadable-<unix seconds>` before anything is saved over it, and the
status line states the new name. The session uses the default layout and keys,
or an empty recents list. Copy what you want back into the new file, or delete
the renamed one.

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

## Project files (`.aria`) and schema versions

A saved project is a UTF-8 JSON document with the `.aria` extension (`PROJECT_FILE_EXTENSION = "aria"`). Every document begins with a `schema` header recording `kind = "aria.project"` (`PROJECT_SCHEMA_KIND`) and `version = 4` (`PROJECT_SCHEMA_VERSION` in `crates/aria-models/src/schema.rs`), followed by `id`, `name`, `metadata`, `plugin_data`, `media`, `timeline`, `sequences`, `active_sequence`, `bins`, and `transcripts`:

```json
{
	"schema": {
		"version": 4,
		"kind": "aria.project"
	},
	"id": 1,
	"name": "short-film",
	"metadata": {},
	"media": [],
	"timeline": {
		"frame_rate": { "num": 24, "den": 1 },
		"tracks": [],
		"order": [],
		"clip_index": []
	}
}
```

| Schema version | Shape changes |
| --- | --- |
| `1` | Initial project document (`media`, `timeline`, `sequences`, `bins`, constant parameters, and single per-clip `mask`). |
| `2` | Animatable parameters may serialize as `{"kf": [...]}` keyframe curves in addition to bare constant values. |
| `3` | Per-clip `mask` is replaced by the `masks` array (`MAX_CLIP_MASKS`), and expression drivers on mask parameters address `{"mask": {"mask": <index>, "param": <name>}}`. |
| `4` | Per-project plugin key-value storage (`plugin_data`, keyed by plugin ID then key, with byte values encoded as lowercase hex strings; omitted when empty). |

On load, documents at schema versions `1`, `2`, and `3` migrate step by step (`v1 -> v2 -> v3 -> v4`) before typed deserialization; saving writes schema version `4`. A document with an unknown `schema.kind` or version `0` is refused with the kind and version it holds and the versions this build reads (1 to 4); a document with a `schema.version` greater than `4` is refused with both versions and a prompt to update Aria. A `.ariat` template is checked the same way, and its refusal states the template and the `.ariat` fix. The file is left unchanged.

Aria saves a project or a `.ariat` template by writing a temporary file in the same folder and renaming it over the old file. A new file takes the umask default mode, as an export does; a file saved over keeps its existing mode.

## Example

Configure project canvas settings, measure and clear a storage cache, and save a custom workspace arrangement:

1. Press <kbd>Ctrl+,</kbd> (`key-settings`) or choose **Edit > Preferences…**. Settings opens with the box empty and lists every section, **Canvas** among them while a project is open. **File > Project Settings…** and **Sequence > Sequence Settings…** open it with `Canvas` in the box, which shows that section alone; Escape clears the box.
2. Pick an aspect ratio and frame rate for the open sequence, then scroll to the **Caches** section and click [`caches-measure`](../reference/controls.md#settings) to compute disk usage across all cache directories. Each cache row fills in its measured byte size and file count.
3. Click [`cache-clear`](../reference/controls.md#settings) on an unpinned cache row (or click [`storage-clear`](../reference/controls.md#settings) in the **Storage** section and adjust the download quota with [`quota-dec`](../reference/controls.md#settings) or [`quota-inc`](../reference/controls.md#settings)), then click [`settings-done`](../reference/controls.md#settings) to close Settings. The status line reports the reclaimed bytes.
4. Drag panel tabs or splitters to arrange the dock layout, then choose **Window > Workspaces > Save Workspace** (or click [`settings-workspace-save`](../reference/controls.md#settings) in the **Workspaces** section of Settings). Aria keeps the layout in `ui-settings.json` as the **Custom** workspace, and a **Custom** row appears under **Window > Workspaces** that restores it at any time.

## Reference

See [Settings](../reference/settings.md), [Workspaces](../reference/workspaces.md), and [Controls](../reference/controls.md#settings) for every `config.toml` key, `ui-settings.json` field, and Settings control.
