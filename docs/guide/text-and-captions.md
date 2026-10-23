# Text and captions

## Text

Select a title. The Text tab shows its content and **Edit text…**, which renames
the text in place. Below it, the Effect chips are the text presets: neon, shadow,
outline, glow, retro, and chrome. Click a chip to apply it, click the active chip
to clear it. A preset writes stroke, shadow, and background onto the style, so
clearing the chip stops marking the preset but leaves what it wrote; edit those
rows to change them. A preset writes only when chosen: an edited stroke stays
when the title is retyped or the same preset is sent again.

Under **Font**, click a family to set it; **Next font** and **Previous font**
step through the installed families. **Style** holds Bold, Italic, Underline,
and Kerning, then the case: Normal, Upper, Lower, Title. **Treatments** switches
the stroke, the shadow, and the card on and off; switching one clears the
preset mark. **Size and spacing** holds size, tracking, leading, and the
treatment sizes, and **Colour** holds a swatch row per colour: fill, then the
stroke, shadow, and card colours while those are on. A colour keyframed at the
playhead takes a new keyframe; any other colour is set for the whole clip.
**Alignment** sets the lines Left, Center, Right, or Justify across the frame
and the block Top, Middle, or Bottom down it; **Wrap** breaks long lines at the
frame edge.

**Window > Character** and **Window > Paragraph** dock the same controls as
panels, in the right frame beside Effects & Presets in the Editing workspace.
They edit whichever title is selected.

With the Type tool (T), click empty picture to place a new title; its
placeholder text is selected, so typing replaces it. Click a title to place the
caret between two characters, drag across it to select, and Shift+click to
extend the selection. The arrow keys, Home, and End move the caret, Shift
extends the selection, Enter starts a new line, and Esc stops typing. Each
burst of typing is one undo step. With characters selected, Bold, Italic,
Underline, the font, size, tracking, and fill colour apply to those characters
only, and the controls show their values; with no characters selected they
apply to the whole title. Leading, case, alignment, and the treatments always
apply to the whole title.

Retyping a title keeps its font, size, colour, alignment, and effect.


## Captions

Transcription runs offline. Install a model first: the **Transcription** section of **Settings**
lists each Whisper model with its size and state, and **Install** downloads it
into `models/` under the data directory. The download is a job with byte
progress and can be cancelled. A file that fails size or checksum verification
reads as `Corrupt` with the reason, and the action becomes **Re-download**.

Three timeouts govern the download, each with **Halve** and **Double** steppers
in the same section: `Connect` (15 s), `Read` (30 s), and `Total` (1800 s). They
persist to the `[transcription]` table in `config.toml`. Values are clamped to
between 1 second and 24 hours, so a stepper cannot write a value the downloader
rejects. A slow link needs a larger `Read`; a large model on a slow link needs a
larger `Total`.

With a model installed, select a clip whose media has an audio track and open
the Project panel's **Captions** tab. Set a language code such as `en` in the box, or
leave it empty to detect one. The thread chips choose how many CPU threads the
decoder uses. Press Enter or click **Transcribe**.

The run is a job: it decodes the clip's source range as 16 kHz mono, transcribes
it, and lists the segments with their timecodes. Cancel stops it and leaves the
project untouched. A run fails before it starts, by name, when nothing is
selected, the clip is generated, the media has no audio track, the media is
offline, no model is installed, or the language is one Whisper does not support.

**Text clips** and **Markers** choose what a commit writes. **One clip per word**
splits each segment by its word timings; a segment the model gave no word timings
for stays whole. **Commit** writes the units at their own times: caption clips
onto the text lane, markers onto the timeline. Each unit is its own undo entry.

Limits: audio longer than 30 minutes is refused, the window comes from the clip's
source range rather than its speed-warped output, and no model ships with the
app.


## Example

Style a title on the canvas and caption a spoken clip with offline Whisper transcription:

1. Press <kbd>T</kbd> (`key-tool-type`) or click [`tool-type`](../reference/controls.md#tool-strip) in the Tool Strip, click in the Viewer to create a title clip at the playhead, type the heading text, and press Esc (or click [`text-edit`](../reference/controls.md#text) in the Effect Controls **Text** tab and step through fonts with [`font-step`](../reference/controls.md#text) or **Window > Character**). A Mango-coloured title clip appears on the text track and renders in the Viewer.
2. Choose **Edit > Preferences…** or press <kbd>Ctrl+,</kbd> (`key-settings`), select **Transcription**, and click [`model-install`](../reference/controls.md#settings) beside a Whisper model to download and verify it in `models/`. The model row updates to installed once the download job finishes.
3. Select a dialogue clip with audio on the timeline and run **Captions** (`pal-captions`) in the Command Palette (or choose **Window > Project** and open the **Captions** tab under **Analyze**). The Project panel opens the **Captions** tab for the selected clip.
4. Click [`caption-run`](../reference/controls.md#captions) (or run **Transcribe Selected Clip** (`pal-transcribe`) in the Command Palette) to transcribe the clip's audio in a background job. Once the segments list appears with timecodes, select **Text clips** and click [`caption-commit`](../reference/controls.md#captions). Aria places timed caption clips across the text track and the Viewer renders each caption over the picture as the playhead crosses it.

## Reference

See [Animations, text effects, easings, and speed presets](../reference/animations.md#text-style-presets-text_effectstoml), [Controls](../reference/controls.md#captions), [Panels](../reference/panels.md), and [Commands](../reference/commands.md) for all text and transcription controls.
