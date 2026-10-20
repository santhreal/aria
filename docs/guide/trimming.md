# Trimming

## Trim tools

The tool bar picks what a drag means. Five tools trim without moving a clip's
edges the way a plain drag does. Each keeps something fixed:

- **Slip (Y)** keeps the clip where it is and moves the take inside it. Drag the
  clip body: the content follows the pointer and the timeline does not change.
  A slip stops at either end of the source. A still, a freeze frame, a generated
  clip, and a clip already using its whole take have nothing to slip.
- **Slide (U)** keeps the clip's length and gives the difference to the clips
  either side of it. Drag the clip body. A neighbour keeps at least one frame, so
  the slide stops rather than deleting it. Sliding into empty space is a move.
- **Rolling edit (N)** moves the cut between two abutting clips. Drag the edge
  that is the cut: one clip grows by what the other gives up, and the pair's
  outer edges stay put. An edge with no clip against it has no cut to roll.
- **Rate stretch (R)** changes how long a clip plays without dropping any of its
  source. Drag an edge: the speed follows from the new length. A clip already
  retimed is stretched from where it is, so two drags compose.
- **Ripple trim (B)** trims an edge and moves the rest of the lane by the same
  amount, so the trim leaves no gap. Drag either edge: every clip later on that
  lane follows. Trimming the head slides the clip back to where it started, so
  the lane keeps its own start.

The five share one tool bar button, which shows the armed trim tool, or Ripple
trim when none is armed. Hold the button for 300 ms, or click its corner
triangle, to list the five; each key selects its tool directly.

Slide, a rolling edit, and a ripple trim each issue two edits and cost one undo.
If the second edit is refused the first is rolled back, so the timeline never
keeps half a gesture.

## Reference

See [Tools](../reference/tools.md) and [Controls](../reference/controls.md#tool-strip) for every trim tool and shortcut.
