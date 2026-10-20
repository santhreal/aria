# Getting started

Aria opens on Home. Start a new project, open a `.aria` file, or drop
media onto the window.

## New project

1. Click **New Project**, or press Ctrl+Alt+N (Cmd+Option+N on macOS).
2. Aria opens with an empty timeline.
3. Click **Import** in the Project panel, or drag files onto the window.
4. Drag a media tile onto a lane, or select it and press `,` to insert it at
   the playhead.
5. Titles, Shapes, and Stickers tabs place a generator at the playhead.

Supported import types: `mp4`, `mov`, `mkv`, `webm`, `wav`, `mp3`, `png`,
`jpg`, `jpeg`, `webp`. Linux video and audio decode through the host
GStreamer plugins. Stills work without them.


## Play and export

1. Press <kbd>Space</kbd> (`key-play`) or click [`play`](reference/controls.md#viewer--source-monitor) in the Viewer control row to play and pause the sequence.
2. Press <kbd>Ctrl+S</kbd> (`key-save`) or choose **File > Save** to write the project to disk.
3. Press <kbd>Ctrl+M</kbd> (`key-export`) or choose **File > Export > Media…** to open the Export dialog, then click [`export-go`](reference/controls.md#export--transcode) to render the sequence in the background.

## User guide

The [User Guide](guide/README.md) covers each editor surface and workflow in its own chapter.

See [keyboard-shortcuts.md](keyboard-shortcuts.md) for the full shortcut list and [reference/README.md](reference/README.md) for the reference manual.
