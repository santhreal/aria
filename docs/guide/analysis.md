# Analysis

## Analysis

The Project panel's **Analysis** tab runs three detectors on the selected clip and keeps
their results in a moments index under `analysis/` in the data directory.

- **Shots** samples the video at 2 frames per second and finds the cuts.
- **Silence** finds the spans quieter than −50 dBFS.
- **Beats** finds the beat times in the audio.

The **Detect** row has one button per detector. A detector with stored results
shows a `×` beside its button that clears its results for that
media file. A run is a job: it decodes and detects on a background thread, reports
progress, and can be cancelled. A run fails before it starts, by name, when
nothing is selected, the clip is generated, the media is offline, or an audio
detector is run on media with no audio track. 10-bit video is rejected for
shot detection rather than reported as having no cuts.
The palette's **Detect Shots** (`pal-detect-shots`), **Detect Silence**
(`pal-detect-silence`), and **Detect Beats** (`pal-detect-beats`) rows start the
same runs as the **Detect** buttons.

A beat run also writes its grid onto the clip it ran on, as one undo entry. The
grid is stored in source ticks, so it rides trims, splits, and speed changes: a
trimmed clip offers only the beats it kept, and the grid saves and reloads with
the project. Clearing the beats analysis clears that grid too. A run that finds
nothing stores nothing, which also clears a grid left by an older run.

Results are rows with a timecode and a confidence. The **Show** chips filter by
detector and the **Confidence** chips set a floor, both without re-reading the index.
**Refresh** re-reads the index, which is how a run that finished while the tab
was showing another clip appears without re-detecting anything.
**Previous** and **Next** move the playhead between the visible rows and stop at
the ends; the palette's **Previous Detected Moment** (`pal-previous-moment`) and
**Next Detected Moment** (`pal-next-moment`) rows do the same. Stored beats also
draw as hairlines on the ruler and act as snap magnets; they are not clickable.

Two rows turn a detection into an edit:

- **Split at shots** cuts the selected clip at every shot boundary inside it.
  Boundaries outside the clip are skipped.
- **Ripple-delete silence** removes every silent span inside the selected clip and
  closes the gaps. Each span is its own undo entry.

**Measure** in the **Loudness** row measures the clip's loudness contour, prints
the peak level beside the button with a clipping flag, and draws the contour in
the Effect Controls panel's Audio tab, one column per window. The contour is
measured on demand, not per frame. **Measure Loudness** (`pal-loudness`) in the
palette does the same.

Results are keyed on the media file, identified by its path, length, and
modification time rather than a digest of its bytes: hashing a multi-gigabyte file
would cost more than re-analyzing it. A byte-identical copy under a second path is
therefore analyzed twice. Each detector also stores its own identity and version.
When a later build raises a detector's version, its old rows are labeled
`stale (analyzer vN)` and offer a re-run; they are never silently trusted or
dropped. A transcription stores its segments in the same index, so a committed
transcript is queryable by kind and confidence alongside the detectors.

The **Storage** section of **Settings** shows the analysis cache size and its path, and **Clear
analysis** deletes the whole index. Per-media clears are on the Analysis tab.

### Ducking music under voice

A duck dips the music wherever someone is talking. It is measured, not authored:
the result is ordinary volume keyframes you can drag, retime, or undo.

1. Right-click the voice lane's header and pick **Duck Source**, or run **Toggle
   Track as Duck Source** (`pal-track-duck`) in the palette. Only an audio lane can be the source.
2. Select the music clip or clips.
3. Open the Effect Controls panel's **Audio** tab and use **Duck under voice**.

Three rows above the button shape the dip. **Dip to** is the level the music
holds while the voice plays, as a share of that clip's own volume, so ducking a
clip already at half volume never raises it. **Attack** is how long the dip takes
to arrive, ending where the voice starts; **Release** is how long it takes to let
go after the voice stops. Each row cycles through five choices.

The line above the button states the lane being listened to. When the run cannot
happen, the reason is written under the button rather than the button being dead:
nothing selected, no lane flagged, no audio on the flagged lane, or a selection
that is only the voice itself.

The measurement is a job: every audio clip on the voice lane is decoded, its
silence is detected, and the voice is what is left. Speech that is closer together
than attack plus release becomes one dip, because there is no room for the music
to come back up between two sentences. Voice that starts before a music clip does
leaves that clip open already ducked, and voice still running at its tail leaves
it ducked to the last frame.

The whole run is one undo entry however many clips and keyframes it wrote. Running
it again replaces the envelope rather than ducking the dip: the level the music
returns to is the top of whatever is on the clip now.


## Example

Detect silence and beats, then duck music under a voiceover track:

1. Select a dialogue clip on `A1`, run **Analysis** (`pal-analysis`) in the Command Palette to open the Project panel's **Analysis** tab, and press <kbd>Shift+Alt+Q</kbd> (`key-detect-silence`) or run **Detect Silence** (`pal-detect-silence`) in the Command Palette. The background job detects silent spans below `-50 dBFS` and lists them with timecodes; click [`analysis-prev`](../reference/controls.md#project-panel) and [`analysis-next`](../reference/controls.md#project-panel) to step the playhead through the detected moments, or click [`analysis-cut-silence`](../reference/controls.md#project-panel) to ripple-delete silent pauses.
2. Select a music clip on `A2` and press <kbd>Shift+Alt+B</kbd> (`key-detect-beats`) or choose **Clip > Analyze > Detect Beats** (or click [`analysis-loudness`](../reference/controls.md#project-panel) / choose **Sequence > Analyze > Measure Loudness** to inspect its loudness contour). Beat hairlines appear along the timeline ruler as snap targets.
3. Right-click the `A1` voiceover track header and choose **Duck Source**, or select `A1` and run **Toggle Track as Duck Source** (`pal-track-duck`) in the Command Palette. The `A1` track header marks `A1` as the duck source.
4. Select the music clip on `A2`, adjust the dip depth with **Sequence > Ducking > Cycle Duck Amount** (or press <kbd>Alt+D</kbd> (`key-duck-amount`)), and click [`duck-run`](../reference/controls.md#inspector) in the Effect Controls **Audio** tab (or press <kbd>Shift+Alt+D</kbd> (`key-duck-apply`) / run **Duck Under Voice** (`pal-duck`)). Aria measures the voice spans on `A1` and writes volume keyframes on the `A2` music clip, dipping the music line on the timeline wherever speech is present.

## Reference

See [Controls](../reference/controls.md#project-panel), [Menus](../reference/menus.md#sequence-menu), and [Commands](../reference/commands.md#job-commands) for all analysis and ducking actions.
