# The assistant

## Assistant

Press <kbd>Shift+0</kbd> (`key-assistant`) or choose **Window > Assistant** to open the assistant panel. Click the prompt box, type an instruction,
and press Enter or click **Send**. The label above the box states the provider and
model the prompt goes to.

A prompt is refused before it costs anything when the box is empty, when a turn
is still running, or when no AI provider is configured. Only the last of those
opens Settings, and it states what is missing.

While a turn runs it appears on the Jobs board and the panel's **Stop** ends it.
Stop is read between deltas, so it lands mid-answer rather than at the end of
one. A stopped turn is undone: the edits it had already applied are rolled back,
the project is left where the prompt found it, and no undo entry is added. A turn
that stops because the assistant hit its edit cap is different: those edits are
kept, and Ctrl+Z undoes them as one step.

The transcript shows the answer as it streams, in the pieces the provider sends,
each edit the assistant applied, and its reasoning summary when the provider
sends one. An edit lands in the project as it is made, so the timeline updates
during the answer rather than after it. The text already streamed stays in the
transcript when a turn is stopped; the edits do not.

Up and Down in the prompt box walk back through prompts already sent. **Clear**
clears the conversation; the project is untouched.

A long conversation scrolls, and follows the answer while it streams. **Copy** in
the header copies the whole conversation with speaker labels; the same button on a
row copies that row.

### Plan only

Turn on **Plan only** and the turn reports the edits it would make without
changing the project. The plan card lists each step in order.

Each step is validated against the plan so far, not against the project as it
stands, so a step that acts on what an earlier step made is planned rather than
refused. The turn applies its steps to work that out and takes them all back
before it returns; a render is validated and not started.

- **Apply** replays that plan as one undo entry. Each step is checked again
  against the project as it now stands; if one no longer fits, nothing is
  applied and the message states the step that failed.
- **Discard** drops the plan.

Clearing the conversation drops a pending plan with it.

### Skills

The panel lists the skills installed for the agent, and **Load catalog** below
the list fetches the cloud skill catalog. Clicking an entry downloads it and
installs it as `~/.aria/agent/skills/<id>/SKILL.md`, which is where the prompt
builder reads skills from. A download that is not a valid skill is refused and
installs nothing. Skills are read at send time, so a skill installed now is
available to the next prompt, and editing a rule file takes effect on the next
prompt without a restart.

The ✓ beside each skill switches it off for this conversation. It stays
installed; the next prompt does not include it. Clearing the conversation
switches them all back on.

Rules are read from `~/.aria/agent/` at send time. The rule text is capped, and a
prompt that had to truncate it states that in the transcript instead of quietly
sending less than the files hold.

### What it can do besides edit

Timeline edits go through the validated vocabulary and land in the undo history.
Alongside them the assistant has app tools:

- **Look at a frame.** It renders the timeline at a time and looks at the picture,
  so it can check what a shot shows before changing it. The row in the
  transcript states the moment it inspected.
- **Read a media file.** Size, frame rate, length, and codecs come from the file
  itself, not from its name.
- **Move the playhead** and **save the project**.
- **Empty a cache.** Destructive, so it goes through **Autonomy**.

With **Autonomy** set to `ask`, a destructive call stops and the transcript shows
what it would do with **Allow** and **Deny**. The turn waits for the answer: it
continues on Allow, and on Deny the assistant receives the refusal and continues
the turn. **Stop** while a question is open denies it. With `full` there is no question.

### Project context in each prompt

Every prompt includes a fresh description of the project: the tracks, the clips
with their ids and times, the media pool, the markers, and where the playhead is.
It is rebuilt for each turn, so the assistant reads the timeline as it stands
rather than as it was when the conversation started.

Up to 300 clips the description holds all of them. Past that it holds the 300
clips nearest the playhead and states its own bounds, how many clips it holds,
and how many clips and markers fall outside them. The assistant can query
another stretch of the timeline by name, so a note about a clip an hour in is
answered by reading that stretch rather than by guessing. Nothing is dropped in
silence: a windowed description states what it left out.

### Conversation persistence

Each project keeps its own conversation, stored beside the app's other session
data rather than in the project file. Reopen the project and the conversation is
back, so the next prompt continues from it. Opening a different project, or
starting a new one, starts a new conversation.

- **Clear** deletes the stored copy at once.
- A project that has never been saved keeps its conversation in memory only; the
  first save starts storing it.
- A stored conversation written by another version of Aria is ignored rather than
  half-read.
- Screenshots are not stored. The assistant already drops them from history
  before each request, so keeping them would store bytes no prompt can use.

## Reference

See [Controls](../reference/controls.md#ai-assistant), [Settings](../reference/settings.md), and [Commands](../reference/commands.md) for all Assistant controls and configuration keys.
