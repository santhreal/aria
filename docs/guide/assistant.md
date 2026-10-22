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


### Model catalogs (`LOCAL_MODELS` and `OPENROUTER_MODELS`)

`LOCAL_MODELS` (`crates/aria-ai/src/catalog.rs`, default `DEFAULT_LOCAL_MODEL = "qwen3:14b"`) lists the curated Ollama model presets offered when `ai.source` is `"local"`:

| Model ID (`id`) | Display (`display`) | Aliases (`aliases`) |
| --- | --- | --- |
| `qwen3:14b` | `Qwen3 14B` | `qwen3:14b, qwen3-14b` |
| `qwen3:32b` | `Qwen3 32B` | `qwen3:32b, qwen3-32b` |
| `llama3.3:70b` | `Llama 3.3 70B` | `llama3.3:70b, llama-3.3-70b` |
| `gpt-oss:20b` | `GPT-OSS 20B` | `gpt-oss:20b, gpt-oss-20b` |
| `gpt-oss:120b` | `GPT-OSS 120B` | `gpt-oss:120b, gpt-oss-120b` |

`OPENROUTER_MODELS` (`crates/aria-ai/src/catalog.rs`, default `DEFAULT_OPENROUTER_MODEL = "openai/gpt-5.6-sol"`, gateway `OPENROUTER_BASE_URL = "https://openrouter.ai/api/v1"`) lists the curated OpenRouter model presets offered when `ai.source` is `"openrouter"`:

| Model ID (`id`) | Display (`display`) | Vendor (`vendor`) | Provider pin (`pin`) |
| --- | --- | --- | --- |
| `openai/gpt-5.5` | `GPT-5.5` | `OpenAI` | `none` |
| `openai/gpt-5.6-sol` | `GPT-5.6` | `OpenAI` | `none` |
| `google/gemini-3.5-flash` | `Gemini 3.5 Flash` | `Google` | `none` |
| `google/gemini-3.1-pro-preview` | `Gemini 3.1 Pro` | `Google` | `none` |
| `google/gemini-3-flash-preview` | `Gemini 3 Flash` | `Google` | `none` |
| `anthropic/claude-sonnet-4` | `Claude Sonnet 4` | `Anthropic` | `none` |
| `anthropic/claude-sonnet-4.5` | `Claude Sonnet 4.5` | `Anthropic` | `none` |
| `anthropic/claude-sonnet-4.6` | `Claude Sonnet 4.6` | `Anthropic` | `none` |
| `anthropic/claude-opus-4.8` | `Claude Opus 4.8` | `Anthropic` | `none` |
| `anthropic/claude-fable-5` | `Claude Fable 5` | `Anthropic` | `none` |
| `x-ai/grok-4.5` | `Grok 4.5` | `xAI` | `none` |
| `x-ai/grok-4.3` | `Grok 4.3` | `xAI` | `none` |
| `x-ai/grok-4.20` | `Grok 4.20 Reasoning` | `xAI` | `none` |
| `moonshotai/kimi-k3` | `Kimi K3` | `Kimi` | `moonshotai` |
| `moonshotai/kimi-k2.6` | `Kimi K2.6` | `Kimi` | `moonshotai` |
| `moonshotai/kimi-k2.7-code` | `Kimi K2.7 Code` | `Kimi` | `moonshotai` |
| `moonshotai/kimi-k2.7-code-highspeed` | `Kimi K2.7 Code Highspeed` | `Kimi` | `moonshotai` |
| `z-ai/glm-5` | `GLM-5` | `GLM` | `z-ai` |
| `z-ai/glm-5.1` | `GLM-5.1` | `GLM` | `z-ai` |
| `z-ai/glm-5.2` | `GLM-5.2` | `GLM` | `z-ai` |
| `z-ai/glm-4.5` | `GLM-4.5` | `GLM` | `z-ai` |
| `openai/gpt-oss-120b` | `GPT-OSS 120B` | `Open weight` | `groq` |
| `meta-llama/llama-3.3-70b-instruct` | `Llama 3.3 70B` | `Open weight` | `groq` |

### Bundled skills (`bundled_skills()`)

`aria_ai::bundled_skills()` (`crates/aria-ai/src/extend.rs`) returns the built-in workflow playbooks that `read_skill` loads on demand; a project or profile skill file with the same ID overrides the bundled definition:

| Skill ID (`id`) | Name (`name`) | Summary |
| --- | --- | --- |
| `social-media-cut` | `Social media cut` | Short vertical 9:16 cut with an opening hook, caption-style title, and fast pacing. |
| `podcast-cleanup` | `Podcast cleanup` | Spoken-word recording edit: voice denoise, level matching, music ducking, and fades. |
| `highlights-reel` | `Highlights reel` | Montage built from marked timeline moments with crossfades and an end title. |

### Built-in loop and host tools

Each turn offers the loop tools `describe_project`, `commit_progress` and `read_skill` and the desktop host tools in `crates/aria-ui/src/app/assistant_host.rs`, besides the timeline edit tools:

| Tool (`name`) | Tier (`tier`) | Summary |
| --- | --- | --- |
| `describe_project` | `ReadOnly` | Inspects a time window or track slice of the active sequence in full clip-level detail. |
| `commit_progress` | `ReadOnly` | Marks the edits applied so far as a named undo phase within a multi-stage prompt. |
| `read_skill` | `ReadOnly` | Loads the markdown playbook body for a named skill from the active skill catalog. |
| `media_screenshot_timeline` | `ReadOnly` | Renders the sequence frame at `at_seconds` to a downscaled PNG and attaches it to the turn. |
| `media_probe` | `ReadOnly` | Reads a media file's dimensions, frame rate, duration, and codecs from `path`. |
| `app_seek` | `Workspace` | Moves the sequence playhead to `at_seconds` (in seconds, clamped to `>= 0`). |
| `app_save_project` | `Workspace` | Saves the open project to its current `.aria` path on disk (fails when untitled). |
| `system_clear_cache` | `System` | Empties one cache directory (`cache`) on disk so its files rebuild on demand. |

### Timeline edit tool groups (`tool_groups()` / `ai_tool_groups()`)

`aria_ai::wire::tool_groups()` (`crates/aria-ai/src/wire.rs`) and `AppState::ai_tool_groups()` (`crates/aria-ui/src/panels/ai_tools.rs`) partition the validated timeline edit tools (`TOOL_SCHEMA_VERSION = 51`) into groups:

| Group (`name`) | Count | Tools (`tools`) |
| --- | --- | --- |
| `tracks` | `7` | `add_track`, `remove_track`, `set_track_enabled`, `set_track_muted`, `set_track_locked`, `set_track_targeted`, `set_track_sync_lock` |
| `clips` | `9` | `add_clip`, `extract_audio`, `duplicate_clip`, `add_generated`, `set_generator`, `split_clip`, `trim_clip`, `move_clip`, `remove_clip` |
| `timeline` | `7` | `ripple_delete`, `shift_clips`, `ripple_insert`, `overwrite_clip`, `fit_to_fill`, `link_clips`, `unlink_clips` |
| `effects` | `4` | `add_effect`, `remove_effect`, `move_effect`, `set_effect_param` |
| `transitions` | `3` | `add_transition`, `remove_transition`, `set_transition` |
| `animation` | `5` | `set_param_keyframe`, `remove_param_keyframe`, `set_param_constant`, `set_param_driver`, `apply_easing_preset` |
| `retime` | `3` | `set_clip_speed`, `set_speed_curve`, `set_clip_pitch` |
| `audio` | `3` | `set_clip_audio`, `set_denoise`, `set_audio_role` |
| `routing` | `5` | `set_track_gain`, `set_track_output`, `add_send`, `set_send`, `remove_send` |
| `look` | `15` | `set_clip_transform`, `set_clip_crop`, `add_clip_mask`, `set_clip_mask`, `remove_clip_mask`, `move_clip_mask`, `set_clip_chroma`, `set_clip_matte`, `set_clip_stabilize`, `set_clip_filter`, `set_clip_blend_mode`, `set_motion_blur`, `set_layer_styles`, `set_clip_adjustments`, `set_clip_animation` |
| `markers` | `3` | `add_marker`, `remove_marker`, `set_marker` |
| `bake` | `2` | `render_and_replace`, `restore_unrendered` |
| `project` | `1` | `set_canvas` |

## Example

Preview a multi-step edit in Plan only mode and apply the validated plan:

1. Press <kbd>Shift+0</kbd> (`key-assistant`) or choose **Window > Assistant** to open the Assistant panel in the centre frame. The Assistant panel shows the active provider and model above the prompt box.
2. Click [`ai-plan-only`](../reference/controls.md#ai-assistant) to enable **Plan only** mode, click [`ai-prompt`](../reference/controls.md#ai-assistant), type an editing instruction, and press Enter or click [`ai-send`](../reference/controls.md#ai-assistant) (or click [`ai-stop`](../reference/controls.md#ai-assistant) while a turn is streaming to cancel and roll back). The assistant validates each proposed edit step against the project state without modifying the timeline and renders a numbered plan card in the transcript.
3. Review the steps listed on the plan card and click [`ai-plan-apply`](../reference/controls.md#ai-assistant) to execute the entire plan as a single undo entry (or click [`ai-plan-discard`](../reference/controls.md#ai-assistant) to drop the plan without changing the project). The timeline applies all planned edits at once and the Viewer updates to the resulting frame.
4. If a tool call prompts for confirmation under `ask` autonomy, click [`ai-ask-allow`](../reference/controls.md#ai-assistant) or [`ai-ask-deny`](../reference/controls.md#ai-assistant); click [`ai-copy-all`](../reference/controls.md#ai-assistant) to copy the transcript or [`ai-clear`](../reference/controls.md#ai-assistant) to clear the conversation history.

## Reference

See [Controls](../reference/controls.md#ai-assistant), [Settings](../reference/settings.md), and [Commands](../reference/commands.md) for all Assistant controls and configuration keys.
