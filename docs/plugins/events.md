# Events

`ctx.events()` is an `EventBus`. It dispatches three kinds of event:

- Notifications (`Event`). `post` queues the event and returns. One delivery
  thread per bus runs the handlers in post order.
- Waterfalls (`WaterfallEvent`). `emit_waterfall` runs the handlers on the
  calling thread. A handler can rewrite the event or stop the chain.
- Serial events (`SerialEvent`). `serial` runs the handlers on the calling
  thread; the first non-`None` result wins.

Subscribe during `mount` with `ctx.on_event`, `ctx.on_event_main`,
`ctx.on_waterfall`, `ctx.on_serial`, or `ctx.on_serial_with_priority`. The
context keeps the `Registration` handle for the life of the plugin, the same as
a service. The bus methods (`subscribe`, `subscribe_main`, `waterfall`,
`on_serial`, `on_serial_with_priority`) return the handle to the caller, so a
bare `ctx.events().subscribe(…)` in `mount` unsubscribes immediately.

Notification and waterfall handlers run in registration order, which is mount
order. Serial handlers run ordered by descending priority (`default = 0`, higher
first), with ties broken by mount order.

## Delivery

- A notification reaches the handlers registered when it was posted. A handler
  whose registration drops before delivery is skipped.
- A handler that panics is logged, and delivery continues to the rest.
- A slow handler delays later notifications, never the thread that posted.
- `ctx.on_event_main` runs the handler on the editor's UI thread instead of the
  delivery thread, in the same order.
- `events.flush(within)` waits until every notification posted before the call
  has run. It returns `false` when `within` elapses first, and at once when
  called from a handler.

```rust
ctx.on_event::<MediaImported, _>(|imported| {
    tracing::info!(path = %imported.path.display(), "imported");
});
```

## Editor notifications

| Event | Posted when | Fields |
| --- | --- | --- |
| `CommandApplied` | A command can no longer be rolled back | `command`, `outcome`, `revision`, `origin` |
| `ProjectClosed` | A session is about to be replaced; followed by `ProjectOpened` | `path` |
| `ProjectOpened` | A project became the session: Open, Load, a template, or a new project | `path`, `project` |
| `ProjectSaved` | The project was written | `path` |
| `MediaImported` | A file was added to the pool | `media`, `path` |
| `MediaRelinked` | A pool item reads a new file | `media`, `path` |
| `MediaOffline` | The pool first shows a file missing | `media`, `path` |
| `SelectionChanged` | The clip selection changed | `clips` |
| `PlayheadMoved` | The playhead moved | `at` |
| `InOutChanged` | The sequence In or Out was set or cleared | `mark_in`, `mark_out` |
| `PlaybackStarted` | Playback started, or its speed changed while playing | `at`, `speed` |
| `PlaybackStopped` | Playback stopped | `at` |
| `ExportStarted` | An export started writing | `path` |
| `ExportFinished` | An export that started has ended | `path`, `result` |
| `RenderFinished` | A clip render or an image sequence ended | `kind`, `path`, `result` |
| `JobProgress` | A background job was queued, started, or reported progress | `job`, `label`, `progress`, `detail` |
| `JobFinished` | A background job ended | `job`, `label`, `outcome`, `detail` |
| `PluginActivated` | A plugin finished mounting | `id` |
| `PluginDeactivated` | A plugin was unmounted | `id` |

`CommandApplied` is posted when a lone command applies, or when the outermost
group that contains it commits. A group that rolls back posts nothing. The
session and media events follow the `CommandApplied` of the command that caused
them, and an `Export` command posts `ExportStarted` and `ExportFinished` before
its `CommandApplied`. The editor redraws on a `CommandApplied` whose origin is
not `Origin::Editor`.

The view events are posted at most once per drawn frame, one per part that
changed. `MediaOffline` is posted again for a file that returns and goes
missing a second time. A render queue entry cancelled before it starts posts no
export events. `result` is `RenderResult::Written`, `Failed(reason)`, or
`Cancelled`, and agrees with the job's final state on the Jobs board.

## Editor view

`EditorView` is a service holding the view as of the last drawn frame. The
engine plugin registers it; a plugin declares `uses: &["EditorView"]` and calls
`current()`, which returns a `ViewState` (`selection`, `playhead`, `mark_in`,
`mark_out`, `playing`, `speed`), or `None` before the first frame.

## Context menus

`ContextMenuRequested` is a waterfall run when a context menu opens. `target`
is a `MenuTarget`: `Clip`, `Track`, `Transition`, `Viewer`, `Media`,
`Recent`, `Effect`, or `ClipBadge`. `Effect { clip, index }` is the effect at
`index` in `clip`'s chain, opened from its header in the Effects tab of Effect
Controls. `ClipBadge { clip, track }` is the fx badge on `clip`'s timeline bar.
`project` is the project the menu was opened over.
`request.add(label, run)` adds a row. Plugin rows are drawn after the editor's
own, below a rule, in mount order.

Handlers run on the UI thread while the menu opens. A handler reads the request
and returns; the work belongs in `run`, which runs on a thread of its own when
the row is clicked. A handler that panics ends the round, and the rows added
before it are kept.

```rust
let host = ctx.get::<ProjectHost>();
ctx.on_waterfall::<ContextMenuRequested, _>(move |request| {
    if let MenuTarget::Clip { clip, .. } = request.target {
        let host = Arc::clone(&host);
        request.add("Remove clip", move || {
            let command = Command::Edit(EditCommand::RemoveClip { clip });
            let _ = host.apply(&Origin::plugin("example.tidy"), command);
        });
    }
});
```

## Hooks

`CommandProposed` is a waterfall run before every command reaches the engine:

| Field | Contents |
| --- | --- |
| `command` | The command; a handler can edit it in place |
| `project` | The project the command applies to, as an `Arc<Project>` |
| `origin` | `Origin::Editor`, `Origin::Assistant`, or `Origin::Plugin(id)` |

`proposed.refuse(reason)` stops the chain: the command is never applied, nothing
enters history, and the reason is shown as written. `EditCommand::targets()`
names the clips and tracks a command touches, so a policy handler does not match
on variants; `ignores_track_lock()` marks the commands a lock must not block. The
engine plugin uses both, with `proposed.project`, to refuse an edit on a locked
lane.

The engine is locked while the chain runs. A handler reads `proposed.project`,
or `ProjectHost::snapshot()`, which returns the same value there; a handler that
locks the engine deadlocks the edit. An edit cannot start inside a handler:
`transact` returns `TransactError::Reentrant`, and `apply`, `group`, `undo`,
`redo`, `history_jump`, and `replace_session` panic.

```rust
ctx.on_waterfall::<CommandProposed, _>(|proposed| {
    if let Command::Edit(edit) = &proposed.command {
        if edit.targets().tracks.contains(&guarded_track) {
            proposed.refuse("That lane is under review");
        }
    }
});
```

`CommandApplied` handlers run on the delivery thread with no lock held, so a
handler can edit through the host.

`ResolveDropped` is a serial event with `item: Dropped::Path(PathBuf) |
Dropped::Url(String)` (`event.item.as_path()` and `event.item.as_url()` accessors). Return
`Some(DropAction)` to claim the item: `OpenProject`, `ImportMedia`, `ApplyLut`,
`InstallSkill`, `ImportTimeline`, or `Handled` (the plugin processed the path
or URL directly). Handlers run ordered by priority (higher first), then mount
order. The decoder claims media extensions, the effects plugin claims `.cube`,
and the AI plugin claims `SKILL.md`. An unclaimed path is imported as media; an
unclaimed URL sets the status strip to `No plugin accepts <url>`. The `.aria`
project extension is opened by the host before `ResolveDropped` runs.

```rust
ctx.on_serial_with_priority::<ResolveDropped, DropAction, _>(10, |event| {
    if let Some(url) = event.item.as_url() {
        return url.starts_with("https://example.com/").then_some(DropAction::Handled);
    }
    event
        .item
        .as_path()
        .and_then(|p| p.extension())
        .is_some_and(|ext| ext == "srt")
        .then_some(DropAction::ImportMedia)
});
```

## External plugins

A native or WASM plugin subscribes to an event by kind (`WireKind::ALL` in `crates/aria-plugin/src/event_wire.rs`) during `mount`:
`host.subscribe(kind)` in `aria-sdk`, `subscribe(kind)` in the WIT world. The
event type name is accepted in place of the kind (`"CommandApplied"` for
`"command-applied"`), and `"resolve-dropped-path"` is accepted for
`"resolve-dropped"`. `"resolve-dropped:<n>"` subscribes at priority `n`, a
32-bit integer; no other kind takes a priority. Any other name fails the
`subscribe` call, and the error lists the kinds.

The plugin's `event` callback receives the kind in the first column, whichever
spelling subscribed, and the event as a JSON object with the fields listed.
Native and WASM plugins receive the same kind and payload.

| Kind | Event | Payload | Reply |
| --- | --- | --- | --- |
| `command-proposed` | `CommandProposed` | `command`, `origin`, `target_tracks`, `target_clips` | Refusal or replacement command |
| `resolve-dropped` | `ResolveDropped` | `kind`, `path`, `extension`, `url` | Drop action |
| `command-applied` | `CommandApplied` | `command`, `outcome`, `revision`, `origin` | Ignored |
| `project-opened` | `ProjectOpened` | `path` | Ignored |
| `project-closed` | `ProjectClosed` | `path` | Ignored |
| `project-saved` | `ProjectSaved` | `path` | Ignored |
| `selection-changed` | `SelectionChanged` | `clips` | Ignored |
| `playhead-moved` | `PlayheadMoved` | `at` | Ignored |
| `in-out-changed` | `InOutChanged` | `mark_in`, `mark_out` | Ignored |
| `playback-started` | `PlaybackStarted` | `at`, `speed` | Ignored |
| `playback-stopped` | `PlaybackStopped` | `at` | Ignored |
| `media-imported` | `MediaImported` | `media`, `path` | Ignored |
| `media-relinked` | `MediaRelinked` | `media`, `path` | Ignored |
| `media-offline` | `MediaOffline` | `media`, `path` | Ignored |
| `render-finished` | `RenderFinished` | `kind`, `path`, `result` | Ignored |
| `export-started` | `ExportStarted` | `path` | Ignored |
| `export-finished` | `ExportFinished` | `path`, `result` | Ignored |
| `job-progress` | `JobProgress` | `job`, `label`, `progress`, `detail` | Ignored |
| `job-finished` | `JobFinished` | `job`, `label`, `outcome`, `detail` | Ignored |

### Payload fields

- `command` is the command in the JSON form `project_transact` takes.
- `origin` is `"the editor"`, `"the assistant"`, or `"plugin <id>"`.
- `target_tracks` and `target_clips` are the ids `EditCommand::targets()`
  returns. Both are empty for a project command.
- `outcome` in `command-applied` is the engine's `ApplyOutcome`, adjacently
  tagged: `{"type": "Saved"}`, `{"type": "Imported", "value": {"media": 3}}`.
- `revision` is the engine revision after the command.
- `at`, `mark_in`, and `mark_out` are times:
  `{"value": 60, "rate": {"num": 24, "den": 1}}`. A cleared mark is `null`.
- `speed` is a multiple of real time. A negative speed plays backwards.
- `path` is a string. In `project-opened` and `project-closed` it is `null`
  for a session that was never saved. Each invalid UTF-8 sequence in a path is
  replaced by U+FFFD.
- `clips`, `media`, `job`, and the entries of `target_tracks` and
  `target_clips` are integer ids.
- `kind` in `resolve-dropped` is `"path"`, sent with `path` and `extension`
  (empty when the file name has none), or `"url"`, sent with `url`.
- `kind` in `render-finished` is `{"clip": <id>}` for a clip render, or
  `"image-sequence"`.
- `result` is `{"status": "written"}`,
  `{"status": "failed", "reason": "<reason>"}`, or `{"status": "cancelled"}`.
- `progress` is a number from 0 to 1, or `null` before the job measures it.
- `outcome` in `job-finished` is `"done"`, `"failed"`, or `"cancelled"`.

A command that has no JSON form, such as one naming a path that is not UTF-8,
is not delivered; the host logs a warning naming the kind.

### Replies

- `command-proposed`: `{"veto": "<reason>"}` or `{"refuse": "<reason>"}`
  rejects the command and shows the reason as written. `{"rewrite": <command>}`
  or a bare command replaces it. An empty reply, or any other, leaves the
  command as proposed. In `aria-sdk` these are `EventResponse::Refuse` and
  `EventResponse::RewriteCommand`.
- `resolve-dropped`: the drop action as a JSON string, a bare word, or
  `{"claim_drop": "<action>"}`. The actions are `open-project`,
  `import-media`, `apply-lut`, `install-skill`, `import-timeline`, and
  `handled`; each `DropAction` variant name is accepted too. In `aria-sdk`
  this is `EventResponse::ClaimDrop`. Any other reply claims nothing.
- Every other kind ignores the reply.

In `aria-sdk`, `EventResponse::Pass` (the default) sends no reply, and
`EventResponse::Json(text)` sends `text` as the reply unchanged.
