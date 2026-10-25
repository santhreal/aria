# The viewer

## Viewing the preview

The control row under the Viewer holds, left to right: the playhead timecode
field, the zoom controls, the resolution menu, the Proxies toggle, the transport
(go to start, step back, play, step forward, go to end), the Loop toggle, the
shuttle readout, **In** and **Out** while Program is in front, the Transform,
Crop and Mask handle toggles, the Scopes toggle, and the peak meter. The toggles
are icons; hover one for its name, and a lit icon is on. The transport acts on
the tab in front of the Viewer: with a Source tab in front, the buttons move its
take and the sequence playhead stays where it was. **In** and **Out** mark the
sequence at the playhead; a Source tab marks its take from its own bar. In a
narrow Viewer the row wraps onto a second line before the transport or before
the handle toggles.

The Viewer fits the frame to the panel by default. The control row holds
`−`, the zoom readout, and `+`. The readout shows `Fit` or a whole percentage;
clicking it returns to fit. The buttons walk a fixed ladder: 25, 50, 75, 100,
150, 200, 400 per cent, and stop at either end.

Above 100 per cent the frame can be panned: the cursor becomes an open hand and
dragging moves the picture. Returning to fit, or zooming back to 100 per cent or
below, re-centers it.

The Scopes toggle opens a strip beside the Viewer with the waveform, the vectorscope,
and the histogram of the frame on screen; the Scopes panel lays the same
readings across its frame. The line above the scopes states the frame they
measure, in the sequence's time display, and reads `no opaque pixels` when
that frame has nothing to count. The chips over the waveform choose what it counts:
**Luma**, **RGB** for a red, green, and blue parade, or **YCbCr** for a luma,
Cb, and Cr parade. **View > Scope Waveform** makes the same choice, and the
choice is saved. The vectorscope draws a skin tone line and a target box for
each primary and secondary at 75 and 100 per cent.

**View > Viewer Display Settings** holds the overlays, each independent of
the others. The wrench at the right end of the Viewer's control row opens the same
submenu under itself: **Split Viewer**, **Show Rulers**, **Show Guides**, **Snap
to Guides**, **Clear Guides**, **Title Safe**, **Action Safe**, **Transparency
Grid**, and **Grid**. A check mark shows each overlay that is on.

- **Title safe** marks the inner 90 per cent of the frame.
- **Action safe** marks the inner 93 per cent.
- **Grid** draws thirds.
- **Transparency Grid** draws a grid of two grays, in 8-pixel squares of the
  rendered frame, where no layer covers the canvas, in place of the sequence's
  background color. A partly transparent layer shows the grid through it.
  Export, Export Frame, and timeline thumbnails keep the background color.
  While the grid is on, each Viewer frame is read back from the GPU, as with
  Scopes open. The setting is saved.

**Show Rulers** under the wrench or **View > Viewer Display Settings**, or
Ctrl+Shift+R, draws rulers along the top and left of the Program view, numbered in
canvas pixels and following the zoom and pan. Drag from the top ruler for a
horizontal guide and from the left ruler for a vertical one; drag a guide to move
it, and drop it back on its ruler or off the frame to delete it. With **Snap to Guides** on, a layer moved by its transform handle snaps its edges or its
center onto a guide within 6 pixels. **Show Guides** off hides the guides
and stops snapping to them without deleting them; pulling a new guide shows them
again. **Clear Guides** deletes every guide. Guides are not saved with the
project and are not edits.

**View > Compare** holds a reference beside the Program frame. **Current
Frame** holds the sequence frame at the playhead; move the playhead to compare
another frame with it. **Still…** prompts for an image file. **Source Tab** shows
the Source monitor's frame at its own playhead, and is disabled with no source
open. **Vertical Wipe** draws the reference left of a divider and Program right
of it, **Horizontal Wipe** draws the reference above and Program below, and
**Side by Side** draws the reference in a pane of its own left of Program. Drag
the divider to move it. The reference keeps the Program view's zoom and pan, and
a caption states its name. **Stop Comparing** returns to Program alone. A still that
cannot be read stops the comparison with an error that states the file.

**View > Fullscreen Preview** fills the window with the viewer, hiding the
other panels. Escape gives them back. A dialog opened by a
shortcut still appears over the picture, and the status line stays visible so a
refusal remains readable.

### Resolution

The resolution chip in the control row lists two settings. **Playback** is
the size frames render at during playback: **Full**, **1/2**, **1/4**, **1/8**,
or **Auto**. Auto starts at Full, drops one level after three dropped frames
within a second, and rises one level after five seconds of playback with no
drop. **Paused** is the size a stopped or scrubbed frame renders at: **Full** or
**1/2**. A reduced frame is rendered small and scaled up to the pane, so it
costs less to render and shows softer.

The chip and the status line show the playback setting, and under Auto the
level it is at, such as `Auto 1/2`. **View > Resolution > Playback Resolution** steps through
the playback choices and **View > Resolution > Paused Resolution** switches between Full and
1/2. Both settings are saved with the other UI settings. Export always renders
at the sequence size.

### Proxies

Right-click a pool item and choose **Decode from Proxy** to render a
smaller intra-frame copy as a background job; the command palette's **Proxy
Every Pool Item** (`pal-proxy-create-all`) does the same for the whole pool. The Proxies toggle in the
control row, and **View > Proxies**, switch the viewer between proxies and the
original files. The toggle is lit while proxies are on. The
choice is saved with the other UI settings. Export always reads the original
files, and a proxy missing from disk reads the original.

### Render files

**Sequence > Render In to Out** (<kbd>Enter</kbd> (`key-render-in-to-out`)) renders every uncached frame in the
work area (`In` to `Out`, or the whole sequence when unset) at full sequence
resolution with motion blur enabled and writes each frame as a content-addressed
QOI file in the `render-files` cache. Preview reads a matching render file from
disk and scales it to the preview size instead of running the compositor; export
renders directly from the timeline and ignores render files. **Sequence > Delete
Render Files** removes the cached render files for the open sequence.

A `3px` render bar along the bottom of the timeline ruler colors frame runs
green when a matching render file is on disk, red when unrendered and the frame
holds a CPU or stateful effect, a nested sequence, motion blur, or more than
four picture layers, and yellow otherwise. Editing a clip changes its frame keys
and turns its span back to yellow or red until rendered again.

### Output meter

Two bars at the right of the control row show the peak level of what the
device is playing, left above right. A full bar is 0 dBFS. The bars turn red and
stay red once a sample reaches full scale; click them to clear that. The meter
shows output, not the selected clip, so soloing or muting a track is visible in
it. Stopping playback drops the bars to silence.

Zoom, pan, the overlays, and fullscreen are viewing positions, not edits: none
of them enters the undo history or changes what an export produces.

### Info

**Window > Info** or Shift+8 docks the Info panel. Over the Viewer it shows the
canvas pixel under the pointer and the color of the frame on screen there: a
swatch, the hex value, the RGB channels, and alpha. The color is read from the
picture as displayed, at the preview resolution, so a reduced resolution reads
the pixel that covers the canvas point. Below that it shows the selected clip's
name, In, Out, Duration, source, and Source In and Out; during a trim, the tool,
the clip, and the change; during a drag in the Viewer, the move, scale, crop,
mask, or tangent being dragged. The Errors section lists driver faults with
their clip and property, plugins that failed to mount, and plugin faults.

The Last frame section states what the most recent frame cost to render: one
row per render stage (Decode, Raster, Composite), the whole frame, and Scales
with size, the part a lower preview resolution buys back. A frame that drew
effect passes adds the pass count and the bind groups built for them. Once
playback delivers a frame, the Playback section states the rate shown against
the sequence's rate and how many frames rendered late. The rate is green while
playback holds it and amber once more than one frame in twenty renders late or
is dropped. **Last Frame Cost** (`pal-render-cost`) in the command palette
prints the Last frame rows on the status line.


## Framing

The Transform tab holds the framing block below the transform rows:

- Crop insets for the left, top, right, and bottom edges. Each stops at the
  frame rather than rejecting the edit, and the kept region never collapses.
- Flip H and Flip V mirror the content after the crop. **Reset framing**
  restores the whole frame and clears both mirrors.
- Motion blur, with a shutter angle in degrees and a sub-frame sample count.

The Viewer's control row holds three icon toggles, Transform, Crop, and Mask,
one per set of on-canvas handles; the lit one is the live set. Click one to make
it live, or choose one under **View > Canvas Handles**. Only one set is drawn at a time, because a
crop corner and a scale corner sit on the same pixel when the crop is full.

In Transform mode, dragging the picture moves it and dragging a corner scales it.
A corner scale is uniform: both axes keep their ratio. Use the Transform tab rows
for a per-axis scale. The scale stops at 0.02 and at 20. Releasing without moving
writes nothing.

In Crop mode, the preview draws crop handles on the selected clip: one dot per edge and one per
corner. Drag a dot to move that edge; a corner moves both of its edges. While you
drag, the preview shows the uncropped frame, so the cut edge stays visible
into. The window stops before it collapses instead of inverting. Escape abandons
the drag and writes nothing; releasing commits the whole drag as one edit, so one
undo restores the frame you started from.

A generated clip whose size follows the canvas gets no handles; crop it with the
inset rows.

### Align and distribute

**Window > Align** docks the Align panel. **Align to** selects what the selected
clips line up against: the box around the selection, the frame, or the first
selected clip. **Left**, **Center**, and **Right** move each clip horizontally;
**Top**, **Middle**, and **Bottom** move it vertically. A single selected clip
aligns to the frame. **Across** and **Down** under **Distribute** keep the two
outermost of three or more clips in place and space the centers evenly between
them.

Bounds are what the compositor draws at the playhead: a rotated clip aligns by
the box around its turned corners, a title by the box its text renders into,
and a parented clip where its parents place it. A clip whose Position is
keyframed gets a keyframe at the playhead, keeping the curve and handles of a
keyframe already there; an unkeyed clip gets a new constant Position. Clips on locked lanes, clips with a
Position driver, and clips not drawn at the playhead stay where they are. One
press is one undo step. A press that would move nothing writes nothing and
reports why on the status line.


## Example

Frame and crop a clip with safe margins, rulers, and crop handles, then render the work area:

1. Choose **View > Viewer Display Settings > Title Safe** (or click [`viewer-display-settings`](../reference/controls.md#viewer--source-monitor) and toggle **Title Safe**) and press <kbd>Ctrl+Shift+R</kbd> (`key-rulers`) or choose **View > Viewer Display Settings > Show Rulers**. The Viewer draws the inner title-safe rectangle and pixel rulers along the top and left edges.
2. Select a clip on `V1` and click [`preview-handles-crop`](../reference/controls.md#viewer--source-monitor) in the Viewer control row (or choose **View > Canvas Handles > Crop**). The Viewer draws four edge and four corner crop handles around the selected clip's frame.
3. Drag the left and right crop handles inward, or click [`crop-clear`](../reference/controls.md#inspector) in Effect Controls (or choose **Clip > Transform > Reset Framing** / run **Reset Framing** (`pal-crop-reset`)) to reset the crop edges. The Viewer updates the cropped frame in real time.
4. Choose **Window > Align**, click [`align-to`](../reference/controls.md#alignment) to select frame alignment, and click **Center** and **Middle**. The selected clip centers horizontally and vertically inside the sequence canvas.
5. Mark an In and Out range on the sequence and press <kbd>Enter</kbd> (`key-render-in-to-out`) or choose **Sequence > Render In to Out** (or run **Render In to Out** (`pal-render-in-to-out`)). Aria renders QOI cache frames for the span and turns the timeline ruler render bar green across the work area.

## Reference

See [Controls](../reference/controls.md#viewer--source-monitor), [Menus](../reference/menus.md#view-menu), and [Panels](../reference/panels.md) for all Viewer, Scopes, Info, and Align controls.
