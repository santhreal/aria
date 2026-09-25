//! AI agent foundation for Aria (v1-roadmap M3, `ai-agent-roadmap.md`).
//!
//! Phase 1 of the agent: the machine-callable surface over the command
//! layer, with no network anywhere.
//!
//! - [`wire`]: the LLM-facing JSON vocabulary — every timeline edit the
//!   agent may request, as flat tagged objects with times in seconds and
//!   ids as plain integers, plus the generated tool schemas.
//! - [`mod@validate`]: lowering wire commands to real `aria_commands`
//!   commands against a project snapshot, with model-readable rejections.
//!   The whitelist lives here (and in the wire types themselves): no
//!   project commands, no phantom generators.
//! - [`describe`]: the compact project summary + editor context pushed
//!   into every prompt.
//! - [`provider`] / [`providers`]: the `ChatProvider` seam (blocking, tool
//!   calling, streamed text) with the generic OpenAI-compatible HTTP
//!   implementation (Ollama / llama.cpp / LM Studio / cloud gateways) and
//!   the deterministic `ScriptedProvider` test double.
//! - [`config`]: API-key resolution (`api_key_env` indirection). The config
//!   *file* is owned by `aria-settings`; keys never live in project files.
//! - [`extend`]: prompt-level extensibility — user/project rules, skills
//!   behind the read-only `read_skill` tool, and slash-command templates.
//!   Rules and skills shape *how* the closed vocabulary is used; they can
//!   never add mutation surface.
//! - [`tools`]: the host-tool registry — the embedder's own tool surface
//!   (screenshots, app control, …) offered alongside the edit vocabulary.
//!   The loop only sees specs and calls; implementations stay host-side.
//!
//! Invariant: **AI proposes, the engine disposes.** Nothing in this crate
//! mutates a project; output is validated commands for the caller to apply
//! through normal dispatch (one history group per prompt, rollback on
//! abort).
//!
//! # Growing the vocabulary (the checklist)
//!
//! The tool schema is closed and versioned; engine commands join the
//! agent's vocabulary deliberately, never by accident. Every new
//! `EditCommand` lands with **all five** of:
//!
//! 1. **Wire DTO + validation** — a serde DTO in [`wire`] shaped for LLM
//!    ergonomics (times as fractional seconds, ids as plain integers,
//!    enums as lowercase strings) and a lowering arm in [`mod@validate`] with
//!    a model-readable rejection for every way the call can be wrong.
//! 2. **Registry row** — one row in [`wire`]'s `wire_tool_registry!`,
//!    binding the tool name, DTO, `WireCommand` variant, and group. The row
//!    generates the enum, provider schema, decode, reverse name, and case-table
//!    group hook. A command variant cannot be declared outside that registry.
//! 3. **Description + group test case** — prose and a case in the named
//!    group's table: the arguments a provider sends, the engine command they
//!    lower to, arguments that must be refused, and the refusal the model
//!    reads. A row without either fails the registry tests.
//! 4. **Action-log line** — a [`agent::describe_action`] arm rendering
//!    the command in editor language ("split clip 7 at 12.40s"); it is
//!    the transcript entry, the undo tooltip, and the eval assertion
//!    format, all at once.
//! 5. **One eval case** — a scripted-provider test in
//!    `tests/agent_eval.rs` driving the new command through the full
//!    loop against a real engine, asserting the final timeline, the
//!    action log, and single-undo behavior.
//!
//! The prompt-visible schema is locked by `tests/snapshots/tools.json`:
//! re-bless it with `BLESS_TOOL_SCHEMA=1 cargo test -p aria-ai` and bump
//! [`TOOL_SCHEMA_VERSION`] when the surface changes shape, so the schema
//! only ever changes as a reviewed diff.

pub mod agent;
pub mod catalog;
pub mod config;
pub mod describe;
mod describe_window;
pub mod extend;
pub mod plan;
pub mod provider;
pub mod providers;
pub mod tools;
pub mod validate;
pub mod wire;

pub use agent::{
    ActionLogEntry, AgentConfig, AgentEvent, EngineBridge, PromptOutcome, PromptStatus, run_prompt,
    run_prompt_with_host,
};
pub use catalog::{
    DEFAULT_LOCAL_MODEL, DEFAULT_OPENROUTER_MODEL, LOCAL_MODELS, LocalModel, OPENROUTER_BASE_URL,
    OPENROUTER_MODELS, OpenRouterModel, ProviderPin, is_known_local_model,
    is_known_openrouter_model, local_model, local_models_availability, openrouter_model,
    resolve_local_installed_id,
};
pub use describe::{
    CLIP_BUDGET, EditorContext, ProjectSummary, SummaryRequest, WindowReason, WindowSummary,
    summarize, summarize_for,
};
pub use extend::{
    AgentDir, AgentExtensions, MAX_RULES_BYTES, Skill, SlashCommand, bundled_skills, compose_rules,
    expand_slash_command, load_agent_dir, merge_skills,
};
pub use provider::{ImagePart, Message};
pub use tools::{
    HostToolSpec, NullToolHost, ToolHost, ToolOutput, ToolTier, is_host_tool_name, namespace,
};
pub use validate::{Lowered, QueuedWork, Rejection, validate};
pub use wire::{TOOL_SCHEMA_VERSION, ToolSpec, WireCommand, tool_specs};
