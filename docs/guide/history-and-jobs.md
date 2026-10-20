# History and jobs

## History

**Window > History**, **History** (`pal-history`) in the command palette, or <kbd>Ctrl+Shift+H</kbd> (`key-history`) brings
the History panel forward. It lists every step this session took, oldest first,
starting with **Project opened**.

Each row is one undo step, named after the command that made it: `Split clip`,
`Move clip`, `Add effect`. A gesture that ran several commands is one row named
after its first, with the count of the rest: `Move clip +2` undoes all three.

The row the project is at is marked **current**. Clicking any other row moves
the project to the state after that step, in either direction; it is undo and
redo without counting keystrokes. A step that has been undone stays in the list,
dimmed, because redo can still reach it. Making a new edit while dimmed rows are
present replaces them, as Ctrl+Z then a new edit always has.

History is per session. Saving does not clear it; opening or loading a project
does.

## Jobs

**Window > Jobs** docks the Jobs panel, which lists background work: exports,
cloud downloads, transcription, and analysis. **Jobs** (`pal-jobs`) in the command palette
opens the same list as a dialog. While work runs, the centre of the status bar
shows the first running job, its progress, and how many others are running;
click it to bring the Jobs panel forward.

Each row shows the label, the state, a detail line such as `frame 240/1800`,
and the elapsed time. A running job that reports a fraction also shows the time
left at its rate so far: `Running · 12.4s · 38s left`. A job that reports a fraction draws a filled bar; one
that cannot measure its total draws a moving bar and reports what it has done
so far.

**Stop** appears while the job can still be stopped, on the board and on the
panel row that started the work, and beside a render in the export sheet. The
same job shows the same control everywhere. It disappears once the job has
reached its publication boundary, the point where it starts writing its result:
an export's finished file, a transcript, or a set of detected moments.
Everything before that point is discarded on stop; a stop that arrives after it
is refused, so a job never both writes its result and reads as cancelled.
Stopping an export discards the render; the destination file is not written.

A request that is not a job has no Stop and does not need one: the AI
connection probe and the model list are bounded at twenty seconds, so a host
that accepts the connection and then sends nothing is reported as unreachable
rather than leaving Settings waiting.

A finished job keeps its row and its outputs. **Reveal** on an output path opens
the containing folder in the file manager.

## Reference

See [Panels](../reference/panels.md), [Controls](../reference/controls.md#modal-dialogs), and [Commands](../reference/commands.md) for all History and Jobs controls.
