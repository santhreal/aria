# Media pool and bins

## Project panel tabs

The Project panel's top row holds five groups: **Media**, **Graphics**, **Effects**,
**Library**, and **Analyze**. Choosing a group opens its first tab, and a group
with more than one tab lists its tabs in a smaller row underneath:

- **Media**: every imported file. **Audio only**, the icon beside the search
  box, lists the audio files only; see [Media pool](#media-pool).
- **Graphics**: **Titles**, **Shapes**, and **Stickers** place a generated
  clip at the playhead on a click. **Masks** applies a mask to the selected
  clip.
- **Effects**: **Effects** is every effect, transition, generator, clip type,
  and animation preset as one tree; see [Effects & Presets](effects.md#effects--presets).
  **Transitions** applies a transition at the selected clip's right cut,
  **Generators** places a generator or plugin clip type at the playhead, and
  **Animations** applies an animation preset to the selected clip.
- **Library**: **Stock** and **SFX** are the cloud libraries; see
  [Stock and sound effects](importing-media.md#stock-and-sound-effects). **Generate** sends a
  prompt to fal.ai; see [Generate](importing-media.md#generate).
- **Analyze**: **Captions** transcribes the selected clip offline; see
  [Captions](text-and-captions.md#captions). **Analysis** detects shots, silences, and beats on the
  selected clip; see [Analysis](analysis.md#analysis).

With no clip selected, or on a clip that rejects the entry, a click that
applies an entry reports why. Markers are listed in the Markers panel; see
[Markers](markers.md#markers).

Drag instead of clicking to choose the target. Drag a sticker onto a lane to place
it at that lane and time. Drag an animation, mask, or transition onto a clip to
apply it there; the clip becomes selected, so Effect Controls shows what landed. A
sticker dropped on a lane that cannot hold it keeps the time and moves to the
sticker lane.

Hover an **Animations** entry to preview it on the selected clip before choosing it.
The preview is not an edit: it makes no undo step, and moving the pointer off the
row restores the clip. With no clip selected there is nothing to preview.


## Media pool

The Media tab has one row above the list and one below it. Above: the
**Search** box, **Audio only** (the icon beside it, which lists the pool's audio
sources), and **Import**. Below: how many of the pool the filter leaves, then the
views, the order, **New bin**, and **New multicam**. Hover an icon for its
name.

The search box matches an item's name, its path, its kind, its frame size, and
its label colour, so a search for `1920`, `wav`, or `teal` finds media whose name
includes none of them. The order button reads the current order; click it for
**Name**, **Duration**, **Resolution**, or **Date added**. Duration and
resolution sort largest first, and date added sorts newest first. An item from a
project saved before import times were recorded has none and sorts last. Neither
the filter nor the order is saved with the project.

The view buttons set the pool layout; in a narrow Project frame one button cycles the four. **Tiles** puts each
item on its own row with its picture above its facts. **List** drops the picture
and reads each item as one line, name on the left and facts on the right, which
is the view to use on a pool of hundreds: no picture means no decode. **Icons**
wraps the pictures into a 96px grid with the name underneath and no facts.
**Freeform** places the pictures where you drag them. The
view is saved with your settings, not with the project, so it is the same in the
next project you open.

Click an item to select it; double-click it to open it in a Source tab (see
[Previewing a source](#previewing-a-source)). Each row states the
frame size, the length, the video codec, and either the audio codec or that the
file is silent, so a silent video is visible before it is cut in. The block under
the list reports what the pool has for the selected item: container, frame
size, frame rate, duration, video codec, audio, when it was imported (UTC),
whether it is still linked, and its path.

Ctrl+click (Cmd+click on macOS) adds an item to the selection, or drops it if it
is already in. Shift+click picks every item drawn between the last one you
clicked outright and the one you shift-click, in the order the list is showing.
No click places a clip. Selected items are bordered, and the properties block
describes the last one clicked.

The order of the selection is the order you clicked, which is the angle order of
a multicam group built from it. Opening another folder drops the selection.

The container is the file extension. A codec name is what the platform's demuxer
reported at import: on Linux, GStreamer's description of the track's caps. Where
a backend reports no codec, the row reads `unknown` rather than guessing from the
extension, which would repeat the container. Stills have no codec track and
read `unknown`.

Drag an item onto the timeline to place it where you drop it:

- Dropped on a lane, it lands on that lane at the tick under the pointer. The
  magnet applies, so a drop near a cut, a marker, or the playhead takes that
  position. A lane that cannot hold the media (audio on a video lane) passes
  it to a lane of the right kind at the same tick.
- Dropped on the thin strip above the top lane or below the bottom one, it
  creates a lane of its own kind and lands there. This is the only drop that
  adds a lane.

With Program focused in the Viewer, `,` inserts the selected items at the
playhead, end to end in the order you clicked them, and `.` overwrites with them.
Both go through the patched lanes, and the placement is one undo step. An item
whose file is missing is skipped.

Right-click a pool item for **Remove from project** and for the label swatches.
An item is removable only
while no clip references it; a removal that a clip still holds is refused with
the reason. Removal is undoable, and the undo reattaches any clips removed
alongside it.

### Bins

The pool holds folders. The folders inside the open one are listed above its
items. While a folder is open, a breadcrumb above them shows the open path
(`Pool / Footage / Day 1`) with **Rename** and **Delete bin** at its right.

- **New bin**, in the row below the list, makes a folder inside the open one,
  opens it, and starts its name in a text field. Type the name and press Enter;
  Esc leaves the field without renaming. An empty name is refused.
- Click a folder to open it. Double-click it to rename it. Click a crumb to go
  back out; `Pool` is the root.
- **Rename** and **Delete bin** act on the open folder. Deleting one moves its
  media and its folders up to its parent, so nothing leaves the pool. Both are
  undoable, and undoing a deletion puts the folder back with what it held.
- Drag a pool item onto a folder row or a crumb to file it there. Drag a folder
  onto another folder or a crumb to move it. A folder cannot be dropped into
  itself or into one of its own folders.

The list shows what is directly in the open folder. Typing in the search box
searches the open folder and everything under it, so a file one level down is
still findable, and the count below the list describes that same scope. Bins are
saved with the project; which folder is open is not.

### Previewing a source

Each pool row holds a picture of its source. Move the pointer across that
picture to scrub the file: the left edge is its first frame and the right edge
its last. Nothing is placed and nothing is edited; a still shows its one frame.
A row shows a glyph until its first frame is decoded, and an audio-only file has
no picture.

Double-click a pool item, or press `Space` with the pool focused, to open it in a
Source tab of the Viewer. While a Source tab is open, the tab row above the
picture holds **Program** first and one **Source:** tab per open item; click a tab to show it, and close a Source
tab with its close button, a middle click, or Ctrl+W with the Viewer focused.
Opening an item that already has a tab shows that tab. A tab keeps its frame and
its marks while it stays open. `Shift+2` with the Viewer focused switches between
Program and the Source tab shown last. `Space` with the pool focused shows Program
again when the item's tab is in front, **Sequence** in the corner shows Program,
and `Space` over any other panel, including Effects and the Project panel tabs that do
not list the pool, or over a pool with nothing selected, shows Program and plays
the sequence. Opening a source stops playback. Safe areas and the transform
handles belong to the sequence and are not drawn over a source. An audio-only
item has no picture and opens no tab. In a narrow Viewer a long name ends in an
ellipsis on its tab and on the name in the corner.

**Split Viewer**, under the wrench at the right end of the Viewer's control row, shows the Source
tab shown last on the left and Program on the right. The keys reach whichever tab is focused.

Double-click a media clip on the timeline to open its take in a Source tab linked
to that clip. The tab opens at the frame the sequence shows under the playhead,
or at the clip's first frame when the playhead is elsewhere, and its In and Out
are the clip's. Marking In or Out in that tab trims the clip: a new In trims the
head, a new Out the tail, and every frame that stays keeps its place on the
timeline. Clearing a mark extends the clip to that end of the take. A reversed
clip trims the opposite end, a retimed clip trims at its speed, and its linked
sound trims with it. A trim that would overlap a neighbour, start before the
sequence, or put Out before In is refused and changes nothing. Clips with a speed
ramp and freeze frames trim on the timeline only. Undo puts the clip and the
tab's marks back together. Deleting the clip leaves a plain Source tab with its
last In and Out. The same take opened from the pool, or from another clip, opens
in its own tab. `F` (Match Frame) opens the pool's tab for the take under the
playhead instead, with the clip's In and Out as plain marks.

The mini ruler under the Viewer's control row spans the tab in front: the whole
sequence on Program, with its In and Out range and its markers, and the whole
take on a Source tab, with the tab's marks. Press it to put that tab on the frame
under the pointer, and drag to scrub; the drag continues when the pointer leaves
the ruler, and Esc puts the frame back. On a Source tab the pool row's picture
follows the ruler. `I` marks the source in point and
`O` the out point while a source is loaded, so those keys mark the take you are
looking at rather than the sequence; `Ctrl+Shift+I` and `Ctrl+Shift+O` clear them, and
**Clear** clears both. An out point is exclusive, so the frame you marked is
included. The ruler shades the marked range, and the line under the picture
counts the frames kept.

A focused Source tab has its own transport. With the Viewer focused, `Space`
plays or stops the take, `Shift+Space` plays its In to Out, `J` and `L` shuttle
it at 1x, 2x, 4x, and 8x, `K` stops it, and `K` held with `J` or `L` steps one
frame. `Left` and `Right` step a frame, `Shift+Left` and `Shift+Right` five,
`Home` and `End` go to either end of the take, and `Shift+I` and `Shift+O` go to
its In and Out. The play button under the Viewer plays the tab in front of it.
Source playback has no sound. Starting the sequence stops the source, starting
the source stops the sequence, and showing another tab stops the source. Over any
other panel the same keys move the sequence.

**Insert** (`,`) puts the marked range into the sequence at the playhead on the
patched lanes and pushes the rest of every sync-locked lane right. **Overwrite**
(`.`) puts it there without moving anything: material inside the span is
dropped, material reaching into the span is trimmed to the edge it crosses, and a
clip that spans the whole window survives on both sides of it. Mark the sequence
at both ends as well as the source and either button becomes a four-point edit:
the sequence's span sets the length and the take is retimed to fill it. Drag
the frame onto the timeline to drop the marked range where you let go, covering
what is there; hold Ctrl to insert instead. Each is one undo entry. The tab stays
open, so takes can be cut in one after another.


## Example

Create a bin, preview a recording in the Source Monitor, mark an In-to-Out take, and insert it into the sequence:

1. Press <kbd>Shift+1</kbd> (`key-pane-project`) or choose **Window > Project** to focus the Project panel, then click [`bin-new`](../reference/controls.md#project-panel) (or run **New Bin** (`pal-bin-new`) in the Command Palette), type `Selects`, and press Enter. The Media pool opens the new `Pool / Selects` bin.
2. Click [`pool-view-list`](../reference/controls.md#project-panel) to switch the pool to list view, or type a query in [`media-query`](../reference/controls.md#project-panel) (or press <kbd>Ctrl+F</kbd> (`key-find`) / choose **Edit > Find…**) to filter the pool rows. The list narrows to matching items and shows the count below the list.
3. Double-click a video item in the pool to open its **Source:** tab in the Viewer ([`viewer-tab-source`](../reference/controls.md#viewer--source-monitor)), scrub to the start of the take, and press <kbd>I</kbd> (`key-mark-in`) or click [`source-mark-in`](../reference/controls.md#viewer--source-monitor). The Source Monitor mini ruler marks the source In point.
4. Scrub to the end of the take and press <kbd>O</kbd> (`key-mark-out`) or click [`source-mark-out`](../reference/controls.md#viewer--source-monitor). The Source Monitor shades the marked source span and displays the frame count kept.
5. Click [`source-insert`](../reference/controls.md#viewer--source-monitor) or press <kbd>,</kbd> (`key-source-insert`) (or choose **Sequence > Source > Insert**). Aria inserts the marked source take at the sequence playhead on the patched tracks and shifts downstream clips right.

## Reference

See [Panels](../reference/panels.md#project-panel-tabs), [Controls](../reference/controls.md#project-panel), and [Keyboard shortcuts](../keyboard-shortcuts.md) for all pool, bin, and Source Monitor controls.
