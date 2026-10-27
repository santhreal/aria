# Trimming

## Trim tools

The tool bar picks what a drag means. Five tools trim without moving a clip's
edges the way a plain drag does. Each keeps something fixed:

- **Slip (Y)** keeps the clip where it is and moves the take inside it. Drag the
  clip body: the content follows the pointer and the timeline does not change.
  A slip stops at either end of the source. A still, a freeze frame, a generated
  clip, and a clip already using its whole take have nothing to slip.
- **Slide (U)** keeps the clip's length and gives the difference to the clips
  either side of it. Drag the clip body. A neighbor keeps at least one frame, so
  the slide stops rather than deleting it. Sliding into empty space is a move.
- **Rolling Edit (N)** moves the cut between two abutting clips. Drag the edge
  that is the cut: one clip grows by what the other gives up, and the pair's
  outer edges stay put. An edge with no clip against it has no cut to roll.
- **Rate Stretch (R)** changes how long a clip plays without dropping any of its
  source. Drag an edge: the speed follows from the take and the new length, and
  the other edge stays where it is. A clip already retimed is stretched from
  where it is, so two drags compose.
- **Ripple Trim (B)** trims an edge and moves the rest of the lane by the same
  amount, so the trim leaves no gap. Drag either edge: every clip later on that
  lane follows. Trimming the head slides the clip back to where it started, so
  the lane keeps its own start.

A trim stops where its edit stops: at the end of the take, at the clip beside
the edge, at the sequence start, or at one frame. The edge follows the pointer
up to that point and stays there, drawn red, while the pointer goes further,
and the readout states where the release lands. While the button is down the
timeline draws each clip the trim changes, and the rest of a rippled lane,
where the release leaves them. A ripple trim moves the rest of the lane out of
its way, so its edge stops only at the take or at one frame. A roll and a
slide stop where a clip they change runs out of take or down to one frame, and
a rate stretch stops at the clip beside the edge it drags or at one frame. A
key that trims the selected edit point stops at the same place.

The five share one tool bar button, which shows the armed trim tool, or Ripple
Trim when none is armed. Hold the button for 300 ms, or click its corner
triangle, to list the five; each key selects its tool directly.

Slide, a rolling edit, and a ripple trim each issue two edits and cost one undo.
If the second edit is refused the first is rolled back, so the timeline never
keeps half a gesture.


## Example

Trim a cut between two clips and adjust the incoming take with the trim tools:

1. Press <kbd>B</kbd> (`key-tool-ripple`) or click [`tool-ripple`](../reference/controls.md#tool-strip) in the Tool Strip, then drag the outgoing edge of the first clip on `V1` left by 10 frames. The clip shortens by 10 frames and every downstream clip on the track and sync-locked tracks ripples left to close the gap.
2. Press <kbd>N</kbd> (`key-tool-roll`) or click [`tool-roll`](../reference/controls.md#tool-strip) in the Tool Strip, then drag the shared cut between the first and second clips right by 6 frames. The first clip grows by 6 frames and the second clip trims its head by 6 frames while the combined sequence duration stays unchanged.
3. Press <kbd>Y</kbd> (`key-tool-slip`) or click [`tool-slip`](../reference/controls.md#tool-strip), then drag the body of the second clip left or right. The clip's source In and Out points shift together in the Viewer while the clip's start and length on the timeline remain fixed.
4. Press <kbd>U</kbd> (`key-tool-slide`) or click [`tool-slide`](../reference/controls.md#tool-strip), then drag the second clip along `V1` between the first and third clips. The second clip moves while its adjacent neighbors trim to preserve their outer bounds.
5. Press <kbd>R</kbd> (`key-tool-rate`) or click [`tool-rate`](../reference/controls.md#tool-strip), then drag the tail of the last clip to match a marker on the ruler. The clip's timeline duration changes and its playback speed percentage updates on the clip name strip.
6. Press <kbd>V</kbd> (`key-tool-pointer`) or click [`tool-pointer`](../reference/controls.md#tool-strip) to return to the Select tool. The timeline shows the updated cut points and speed badge, and the Viewer displays the frame at the playhead.

## Reference

See [Tools](../reference/tools.md) and [Controls](../reference/controls.md#tool-strip) for every trim tool and shortcut.
