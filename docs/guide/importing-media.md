# Importing media

## Dropped files

What a dropped file becomes follows its type, on Home and in Aria alike:

| File | Result |
| --- | --- |
| `.aria` | Opens as a project, replacing the session |
| `.edl`, `.fcpxml`, `.otio`, `.srt`, `.vtt`, or another extension a timeline plugin reads | Read into the open timeline, one undo step per file |
| Media (see the list above) | Imported into the media pool |
| `.cube` | Applied to the selected clip; with no clip selected, read as a LUT filter over the sequence |
| `SKILL.md` | Installed as an assistant skill |
| Anything else | Imported as media, which reports why it cannot be |

A project file dropped alongside other files wins: opening one replaces the
session, so the rest would be discarded.

## Timeline files

**File > Import Timeline…** reads another editor's cut into the open project,
and **File > Export > Timeline…** writes the open timeline as one. The shipped
Timeline interchange plugin reads and writes these formats:

| Format | Extension |
| --- | --- |
| CMX 3600 EDL | `.edl` |
| Final Cut Pro XML | `.fcpxml` |
| OpenTimelineIO | `.otio` |
| SubRip subtitles | `.srt` |
| WebVTT subtitles | `.vtt` |

A plugin can add more. The dialogs list every format a mounted plugin reads or
writes. **File > Import…** and a drop onto the window also read a timeline file
into the timeline rather than the media pool.

**File > Import Recent** lists the last ten files imported through
**File > Import…**, newest first, and imports the chosen one again. A file that
no longer exists is listed disabled. **Clear Recent Imports** empties the list.
The list is saved as `import-recents.json` in the data folder.

An import adds lanes and clips to the open project as one undo step, and
imports the media files listed in it. The export format follows the extension of the
name you give; a name with no extension takes the first format listed. An
export that fails leaves an existing file at that path as it was.

A format holds less than an Aria timeline. An EDL holds one picture track and
four sound channels, for example, and no format holds a generated clip. What a
run places or writes the nearest way, or leaves out, is listed in the
**Timeline file** panel in Effect Controls, one line per item. The panel stays
until you click **Dismiss**, and the next import or export replaces it.

A subtitle file adds a text track named after the file, with one text clip per
cue on the nearest frame. Cues that overlap go to further text tracks, numbered
`(2)`, `(3)`, and so on. Styling tags and WebVTT cue settings are dropped.
Exporting subtitles writes the text clips of every shown text track as cues in
start order; hidden text tracks and clip styling are left out and listed.

**File > Import Timeline…** also lists **Cube LUT** (`.cube`), read by the
Effects plugin. It adds a filter track named after the file with one LUT
filter bar across the sequence, or five seconds long on an empty sequence. A
file that is not a 3D Cube LUT is refused and the project is unchanged.

With the Timeline interchange plugin turned off in the **Plugins** section of **Settings** and no
other timeline plugin installed, both rows report that no importer or exporter
is mounted.

## Missing media

If a media file moves after save, its pool row is marked missing. Click
**Relink** on the row to point it at the file, or **Relink All** to pick one
folder and repair every missing row from it.

Relink All searches the folder and up to four levels below it, matching by file
name, then checks each same-named file against what the project recorded for
that row: frame size, duration, and whether it has audio. A row is repaired
only when one file matches. Anything else leaves the row missing and
states why on it:

| Row text | Meaning |
| --- | --- |
| not in that folder | No file there has that name |
| N files there match | Several files match everything recorded; relink that row by hand |
| the file there has a different frame size / duration / audio | The name matched, the file is another source |
| could not read the file there | The file exists and did not open |

## Stock and sound effects

The Project panel's **Library** group holds the **Stock**, **SFX**, and
**Generate** tabs.

Stock searches video, photo, and audio libraries. Click the query box, type, and
press Enter or click **Search** beside the box. Escape leaves the box. The
**Video**, **Photo**, and **Audio** chips under the box choose what to search
for; switching chips clears the results, which answered the previous kind.

Before the first search, the lines under the chips state the route. With a
Pexels or Pixabay key set in Settings, the query goes straight to that provider
and Aria's servers never receive it. With no key, the query goes through Aria's
anonymous search endpoint.

SFX lists the sound-effect catalog. Click **Load catalog** to fetch it.

A failed feed shows the reason instead of an empty list. Clicking a result
downloads it into the download cache and pins it, so **Clear cache** cannot delete
a file the project references. What happens next depends on the catalog.

## Cloud LUTs, templates, and text presets

Three more catalogs load the same way, with **Load catalog**:

- **Cloud LUTs** in the **Look** tab applies the picked `.cube` to the selected clip. With
  nothing selected the click is rejected and reports why.
- **Cloud templates** under **Samples** on **Home** downloads a bundle and then prompts for one
  file per slot, as **File > Use Template…** does.
- **Cloud presets** in the **Text** tab downloads a *pack*. The pack's presets then list
  below it; clicking one writes its font, size, and fill onto the selected title
  and applies its animations. A preset the pack lists that this build does not include
  is skipped, not refused.

Stock results and SFX import into the media pool instead.

## Generate

The Project panel's **Generate** tab sends a prompt to fal.ai with your own key.

Click the prompt box, type a description, and press Enter or click **Generate**
beside the box. The **Image**, **Video**, and **Voice** chips choose what to
produce; switching chips keeps the prompt. Escape leaves the box.

Each submit becomes a job. Its row appears under the prompt and on the Jobs
board with the queue state and the download progress, and Cancel stops it. The
result downloads into the download cache, is pinned, and imports into the media
pool.

Without a `fal` key the tab states which key to add and offers no **Generate**
button. **Open Settings** under the line opens Settings at the **Keys** section.
See [Provider keys](settings-and-storage.md#provider-keys) for where the key goes.

**Lottie animations** under the jobs lists the Lottie catalog. Click **Load
catalog** to fetch it; clicking an animation places it at the playhead.


## Example

Import a video file, merge a subtitle timeline file, and relink a moved source file:

1. Choose **File > Import…** or press <kbd>Ctrl+I</kbd> (`key-import`) (or click [`media-import`](../reference/controls.md#project-panel) in the Project panel) and select a `.mp4` recording. The Media pool adds the clip tile and displays its frame size, duration, and codecs.
2. Choose **File > Import Timeline…** and select a `.srt` subtitle file. Aria creates a text track named after the file and places one title clip per subtitle cue at its timecode on the timeline.
3. If a source file moves on disk and its pool row reads missing, click [`relink-all`](../reference/controls.md#project-panel) (or choose **File > Relink All…** or run **Relink All** (`pal-relink`) from the Command Palette) and choose the parent folder. The missing badge clears on every matched pool row and the timeline clips draw their frames in the Viewer.
4. If the import placed any item the nearest way it could, the **Timeline File** dialog lists each one. Click [`timeline-report-close`](../reference/controls.md#modal-dialogs) once you have read them. The dialog closes and its report is dropped.

## Reference

See [Menus](../reference/menus.md#file-menu), [Controls](../reference/controls.md#project-panel), and [Commands](../reference/commands.md) for every import and relink entry.
