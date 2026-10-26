# aria-ai

`aria-ai` is the prompt-to-edit layer for Aria. It turns model responses into validated editor commands that callers can apply through the normal engine path.

The crate does not mutate projects directly. It describes project state for the model, accepts tool calls or command-shaped responses, validates them against a project snapshot, and returns `aria-commands` values for the caller to apply.

## Responsibilities

- Define the LLM-facing wire command vocabulary.
- Generate tool schemas for supported edit operations.
- Summarize the current project and editor context for prompts.
- Validate model-requested edits before they reach the engine.
- Return model-readable rejections for invalid requests.
- Provide a provider abstraction for chat and tool-calling backends.
- Support OpenAI-compatible HTTP endpoints and deterministic test providers.
- Run the prompt loop and report agent events to the caller.

## Main APIs

- `run_prompt`: execute one assistant prompt flow.
- `AgentConfig`: prompt-loop configuration.
- `AgentEvent`: streamed events emitted while a prompt runs.
- `PromptOutcome` and `PromptStatus`: final prompt result.
- `EngineBridge`: trait used by the agent loop to inspect and apply edits through a host.
- `summarize`: compact project summary for model context.
- `validate`: lower wire commands into engine commands or return rejections.
- `tool_specs` and `TOOL_SCHEMA_VERSION`: tool schema surface exposed to providers.
- `WireCommand`: model-facing edit request type.
- `AgentExtensions`, `load_agent_dir`, `bundled_skills`, `merge_skills`: prompt extensibility (rules, skills, slash commands).

## Rules, skills, and slash commands

Users can extend the assistant through `~/.aria/agent/` (reloaded before every prompt, no restart needed):

- `rules/*.md`: always-on rules injected into the system prompt, capped at `MAX_RULES_BYTES` (over-budget rules truncate with a visible warning in the agent panel).
- `skills/<id>/SKILL.md`: on-demand procedural workflows with YAML frontmatter (`name`, `description`). Only the name/description index enters the system prompt; the model fetches a body through the read-only `read_skill` tool.
- `commands/*.md`: slash-command prompt templates. Typing `/name args` in the chat panel expands the template client-side (`$ARGUMENTS` substitution).

Per-project rules live in `ProjectMetadata.agent_rules`, edited from the agent panel, and travel with the project file. The panel shows them (never silently applies them) when a project arrives from elsewhere. Three first-party skills ship embedded from `assets/skills/`; user skills with the same id win.

All of this is prompt-level only: the closed command vocabulary, validation, dry-run, and one-undo-group invariants are untouched, so a bad rule or skill can at worst propose bad edits.

## Configuration

The desktop app reads AI settings from `~/.aria/config.toml`:

```toml
[ai]
base_url = "http://localhost:11434/v1"
model = "qwen3:14b"
# api_key_env = "OPENAI_API_KEY"
```

Secrets should stay in user config or environment variables. They should not be stored in project files or committed to the repository.

## Adding agent capabilities

Every model-visible tool is one row in a domain group under `src/wire/tools/` (tracks, clips, timeline, effects, transitions, animation, retime, audio, look, markers, project). The registry is the only enumeration of the surface: the specs sent to a provider, the tool-call dispatch, and the tests all read it. Adding an operation means a wire DTO with its validation, a registry row in the group that defines it, and a case in that group's table giving the arguments a provider sends, the engine command they lower to, and the rejection an invalid call returns. A row without a case fails its group's test, and a `WireCommand` variant without a row fails the registry census, so a tool cannot reach a model untested. The checklist in `src/lib.rs` is the full form.

## Testing

Run tests with:

```bash
cargo test -p aria-ai
```

If the tool schema intentionally changes, re-bless the snapshot with:

```bash
BLESS_TOOL_SCHEMA=1 cargo test -p aria-ai
```
