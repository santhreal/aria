# Multicam

## Multicam

A multicam group is several cameras of the same take, cut between as one clip.

Select the camera files in the pool and press **New multicam**, beside **New
bin**. The group takes one angle per selected item, in the order you clicked
them, and appears as a row below the folder strip. With nothing selected the
button groups the whole pool, which covers an import of a card's worth of
angles. An empty pool reports that and builds nothing.

The row states the group, counts its angles, and holds one row per camera. The
lit chip is the camera live at the playhead, read from the group's cut list as
you scrub. Clicking a chip cuts to that camera at the playhead.

**Place** puts the group on a video lane at the playhead, following the same lane
target a media drop follows. Its length is the group's own: from the group's
first frame to the last frame any of its cameras can still answer for, so a
camera that runs longer than the first one does not get cut short.

What the frame shows is the camera the cut list makes live at that point,
decoded from that camera's own file and fitted to the frame at its own size, so
a group of mixed formats is not letterboxed to whichever camera came first.

Beside each chip is that camera's sound, drawn across the whole group's span
rather than across its own file. Two strips read one above the other are two
pictures of the same stretch of time: cameras that are in sync show the same
transients at the same place, and one that is out shows them displaced by
however far out it is.

The chevrons either side of the offset move a camera on the group's clock by one
frame, and the number between them is where that camera's first frame sits: `0f`
is the start of the group, `+24f` a camera that started a second late at 24 fps.
The offset is what the frame is read through, so moving it moves what the group
shows at every tick that camera is live.

The waveform button measures it instead. The peaks already decoded for the strips
are correlated against the first camera's, and every other camera is placed
where its sound matches. The first camera is the clock and does not
move, so syncing a group twice gives the same answer, and the whole group is one
undo entry however many cameras moved.

A camera it cannot place is left where it is and named: sound still being read,
a file with no sound or none that varies, sound that does not match the first
camera's well enough to be the same take, or sound that repeats. A repeating
sound matches at every repeat, so the best match is whichever repeat came first
rather than the right one. An offset applied from a guess would move a camera
that was already in sync, and nothing on screen would indicate it had.


## Example

Build a multicam group from several camera angles, sync them by audio waveform, place the group on the timeline, and cut between angles:

1. Press <kbd>Shift+1</kbd> (`key-pane-project`) or choose **Window > Project**, Ctrl+click each camera angle in the Media pool in angle order, and click [`create-multicam-group`](../reference/controls.md#project-panel) below the pool list. A multicam group row appears under the folder strip listing each camera angle and its audio waveform strip.
2. Click [`sync-multicam-group`](../reference/controls.md#project-panel) on the multicam group row (or adjust an individual camera's frame offset by one frame with [`nudge-multicam-back`](../reference/controls.md#project-panel) and [`nudge-multicam-forward`](../reference/controls.md#project-panel)). Aria correlates each angle's decoded waveform peaks against the first angle and aligns their frame offsets in one undo step.
3. Move the sequence playhead to the start of the scene and click [`place-multicam-group`](../reference/controls.md#project-panel) on the multicam group row. A multicam clip spanning the combined coverage of the angles lands on `V1` and its linked audio lands on `A1`.
4. Scrub or play the timeline with <kbd>Space</kbd> (`key-play`) and click an angle chip ([`switch-multicam-angle`](../reference/controls.md#project-panel)) in the multicam row at each point where the camera should switch. The group's cut list records each switch at the playhead and the Viewer immediately displays the newly active camera angle.

## Reference

See [Controls](../reference/controls.md#project-panel) for all multicam pool controls.
