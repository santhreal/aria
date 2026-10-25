# Export

## Templates

The Info tab holds the template block.

- **Replace media** lists every pooled item of a compatible kind. Picking one
  repoints the clip at it and keeps the placement, transform, and effects. The
  source window keeps its length where the new media is long enough.
- **Mark as template slot** makes the clip fillable when the project is used as
  a template. Fill order steps, the media restriction is a chip row, and
  **Edit hint…** types the placeholder text. The timeline paints a slot badge on
  a marked clip.
- On a text clip, **Text editable in template** lets the text be retyped while
  the style and animation stay.


## Template bundles

A template is a finished project whose replaceable clips are marked as slots.
See [Templates](#templates) for the marks.

- **File > Save as Template…** writes a `.ariat` bundle. The project itself is
  untouched: no rename, no dirty flag, no change to the project path. The status
  line reports how many slots the bundle holds.
- **File > Use Template…**, or the Use Template tile on Home, opens a bundle,
  prompts for one file per slot in fill order, using the slot's hint as the prompt,
  and replaces the session with the filled result. Cancelling the picker stops
  prompting and applies what was chosen, so a bundle can be opened unfilled with its
  own sample media in place.

## Export and save

- Ctrl+S saves to `~/.aria/untitled.aria` the first time, or to the last
  project path.
- Ctrl+Shift+S saves to a new path.
- Ctrl+O opens a project. Ctrl+Shift+W returns to Home.
- Ctrl+M opens Export. Choose a destination or accept the default `.mp4`.
- Export writes what this platform's encoder supports. The sheet's **Format**
  and **Encoder** rows name the container, the codec of the file about to be
  written, and the backend (GStreamer on Linux, VideoToolbox on macOS, Media
  Foundation on Windows). A build with no native encoder states that and fails
  rather than starting a render it cannot finish.
- An export that would write no frames is refused before any file is created.
  A sequence with nothing on it prompts for a clip; a marked range that covers no
  frame prompts for the In and Out points to be moved apart. Neither writes a
  zero-frame file.
- Export runs in the background and opens the Jobs board. Editing and playback
  continue while it renders.

The Export sheet holds seven choices. Each row steps to its next value when
clicked, and none of them is an edit or is saved with the project:

- **Scale** renders at 100, 75, 50, or 25 per cent of the canvas. The row shows
  the resulting pixel size.
- **Frame rate** is the project's rate or a fixed 24, 25, 30, 50, or 60.
- **Codec** is H.264 or H.265. The row offers only the codecs this machine can
  write: on Linux that is probed from the installed GStreamer encoders, and on
  macOS, Windows, and Android it is H.264 alone. With one codec available the
  row fails and states it.
- **Bitrate** is the encoder's own rate control, or a target of 4, 8, 16, or 40
  Mb/s. Only GStreamer takes a target; the other backends reject the row and state
  so. A codec or bitrate the encoder cannot honor is refused by name before any
  file is created, never swapped for something else.
- **Keyframes** is the codec's own spacing or one every 15, 30, or 60 frames.
- **Audio** includes the mix or writes a video-only file.
- **Range** is the whole timeline or the marked range. Marking is required
  first: with no In and Out set, the row fails and states why.

The **Destination** row shows the file the export writes. A path wider than the
sheet loses its start to an ellipsis, and the file name stays in view.

The status line states the size, whether audio is included, and the span when the
export starts.

The sheet governs every export, whether it runs as a job or blocks: **File > Export > Media…**
sends the same choices, and `aria export` on the command line takes them as
flags. One piece of code resolves a request against the project and the machine's
encoder, so the same choices write the same file from any of the three.


## Unsaved work

New, Open, Close, Home, Quit, and a project dropped on Home all leave what is
loaded. With unsaved edits in the project, each one prompts first:

- **Save** writes the project, then runs the requested action. A save that fails
  leaves the prompt up and the edits in place.
- **Discard** runs the requested action and loses the edits. Close and Home
  replace the project with an empty one, so Quit on Home does not prompt again.
- **Stay** cancels. Nothing is saved and nothing is replaced.

While a project has unsaved edits, a copy is written to `~/.aria/recovery.aria`
every twenty seconds, in the background, so editing continues while it is
written. Saving, opening, or starting a project deletes it, as does **Discard**
on Close, Home, or Quit, so the copy only ever exists when the file on disk is
behind the session.

If the app stops without saving (a crash, a kill, a power cut), Home shows the
copy on the next launch:

- **Restore** loads it and aims Save back at the project it came from. The
  session reads unsaved, because the project file still holds the older content.
- **Discard** deletes the copy.


## Example

Export a sequence as an H.264 `.mp4` file for web delivery:

1. Mark the sequence In and Out points if exporting a sub-range, or leave the work area spanning the full cut, and press <kbd>Ctrl+S</kbd> (`key-save`) or choose **File > Save** to save the project first. The window title clears its unsaved `*` indicator.
2. Press <kbd>Ctrl+M</kbd> (`key-export`) or choose **File > Export > Media…** (or run **Export** (`pal-export`) in the Command Palette). The Export modal sheet opens over the workspace.
3. In the Export sheet, verify **Scale** is `100%`, **Codec** is `H.264`, **Bitrate** is `8 Mb/s` (or `16 Mb/s` for high-motion 1080p/4K web upload), **Audio** includes the mix, and **Range** matches the intended span, then click [`export-dest`](../reference/controls.md#export--transcode) to pick the output `.mp4` path. The **Destination** row displays the target file path and resolved pixel dimensions.
4. Click [`export-go`](../reference/controls.md#export--transcode) to start the background encode (or click [`export-cancel`](../reference/controls.md#export--transcode) to close the sheet without rendering). The Jobs board opens showing frame progress and estimated time remaining, and clicking [`job-reveal`](../reference/controls.md#modal-dialogs) once the job finishes opens the exported `.mp4` in the system file manager.

## Reference

See [Controls](../reference/controls.md#export--transcode), [Menus](../reference/menus.md#file-menu), [Commands](../reference/commands.md), and [Command line](../command-line.md) for all export controls and CLI flags.
