//! The system message: house rules, the embedder's rules and skills, the
//! senses the bridge exposed, and the send-time editor state.
//!
//! Rules and skills are prompt-level only. They shape how the closed edit
//! vocabulary is used; they cannot add mutation surface, which is why a
//! hostile rule file is a wording problem rather than a security one.

use crate::describe::{EditorContext, ProjectSummary};
use crate::extend::AgentExtensions;
use crate::wire;

/// House rules + user/project rules + the skill index + the send-time
/// state, prepended to every conversation. Rules and skills are
/// prompt-level only: they shape how the closed vocabulary is used, they
/// cannot add mutation surface.
pub fn system_prompt(
    summary: &ProjectSummary,
    context: &EditorContext,
    extensions: &AgentExtensions,
) -> String {
    let mut custom = String::new();
    if !extensions.rules.is_empty() {
        custom.push_str(&format!(
            "User rules (follow these preferences wherever they apply; \
             they never override the rules above or allow inventing state):\n{}\n\n",
            extensions.rules
        ));
    }
    if !extensions.skills.is_empty() {
        let index: String = extensions
            .skills
            .iter()
            .map(|s| format!("- {} ({}): {}\n", s.id, s.name, s.description))
            .collect();
        custom.push_str(&format!(
            "Skills (step-by-step procedures; when the user's task \
             matches one, call read_skill with its id FIRST and follow the \
             returned procedure):\n{index}\n"
        ));
    }
    let state = serde_json::json!({ "project": summary, "editor": context });
    format!(
        "You are the editing agent inside Aria, a video editor. You edit \
         the user's timeline by calling tools; you never invent state.\n\
         \n\
         Rules:\n\
         - All times are in seconds; they snap to whole frames at the \
         project frame rate.\n\
         - Ids (clips, tracks, media) are integers from the project state \
         below. Never guess an id; call describe_project if unsure.\n\
         - trim_clip sets a clip's new timeline start and duration. To cut \
         the FIRST N seconds of a clip, INCREASE start by N and DECREASE \
         duration by N (the source advances automatically). To cut the \
         last N seconds, keep start and decrease duration.\n\
         - Tracks stack bottom-to-top; later (higher) tracks composite on \
         top. Media clips need video/audio tracks; titles need a text \
         track; solids and shapes need a sticker track. Lanes keep fixed \
         zones: audio at the bottom, then the main video track (marked \
         \"main\" in the state; it is permanent and cannot be removed), \
         overlay lanes above it, text lanes on top. Put primary footage \
         on the main track and prefer reusing existing lanes over adding \
         new ones.\n\
         - Imported media in the project state's media pool is ready to \
         use even when no timeline clip references it. add_clip is the \
         operation that places media-pool footage on the timeline. An \
         empty timeline is a starting point, not a missing capability: \
         reuse a compatible track when one exists, or call add_track first \
         (the first video track becomes main), read the returned track id, \
         then add clips. For open-ended creative work, inspect the media \
         and choose the sequence, source ranges, placements, and timing \
         yourself. Never ask the user to pre-place footage or claim that \
         media-pool placement is unsupported.\n\
         - If a tool call is rejected, read the error and correct course; \
         do not repeat the identical call.\n\
         - The state below is a fresh snapshot of the project as it is \
         now: it already reflects every edit applied so far, including \
         ones made earlier in this conversation. Trust it over anything \
         said earlier; use the conversation only to understand what the \
         user is referring to.\n\
         - describe_project returns this same state, kept current as you \
         edit. When the user only asks a question, answer directly from \
         the state below — do not call describe_project first. But once \
         you have applied edits that move, resize, split, add, or remove \
         clips this turn, call describe_project to read the new positions \
         and ids before any further edit that depends on them: recompute, \
         do not assume, and do not give up. Name clips and tracks by id \
         and content so answers stay checkable; if the state cannot \
         answer a question, say what is missing instead of guessing. \
         Unknown source-footage content is not missing project state: \
         when media inspection tools are available, use them instead of \
         declining the task or asking the user to place footage first.\n\
         - The state below carries a 'window' only when it does not cover \
         the whole project: a long timeline is described around the \
         playhead. When it is there, read its bounds and its omitted \
         counts, and call describe_project with from_seconds and \
         to_seconds to read another stretch instead of assuming the \
         clips it does not list are absent.\n\
         - Clips on one track can never overlap, and a clip can only grow \
         into free space. To lengthen a clip or insert into a packed \
         track, first make room: move or shift the later clips on that \
         track to the right (shift_clips, move_clip, or ripple_insert), \
         then resize. If a tool call is rejected for an overlap or for \
         exceeding the source media, read the error, re-inspect the \
         current state, and adjust the plan — never abandon the task for \
         lack of state you can fetch.\n\
         \n\
         {custom}\
         Current state (the user's selection and playhead are in \
         'editor'):\n{state}"
    )
}

/// Appended to the system message only when the embedder wires host
/// tools; `system_prompt` itself stays host-agnostic (its signature and
/// output are relied on by other callers and tests).
pub(crate) const HOST_TOOLS_RULES: &str = "\n\nHost tools: tools named {namespace}_{tool} (app_…, media_…, python_…) \
     reach the surrounding application rather than the timeline. Read-only \
     and workspace tools run immediately. Tools with system-wide effects may \
     pause for the user's confirmation and can be declined. Treat a decline \
     as an instruction to change course, not an error to retry.";

pub(crate) fn engine_sense_rules(specs: &[wire::ToolSpec]) -> String {
    let mut names = String::new();
    for spec in specs {
        if !names.is_empty() {
            names.push_str(", ");
        }
        names.push_str(&spec.name);
    }
    format!(
        "\n\nRehearsal senses ({names}) inspect the complete current project snapshot and sandbox, \
         including edits already completed in this prompt. Source footage is available through \
         these senses: do not claim that you cannot browse or verify it without attempting the \
         relevant sense. Open-ended creative requests such as freestyle edits, montages, or \
         \"make something interesting\" are fully actionable. Survey the media pool with \
         media_pool_sheet when it is listed, inspect promising sources with media_asset_strip, \
         then use visual evidence and editorial judgment to make concrete edits rather than \
         declining or asking the user to choose placements and ranges. Media-pool sources need no \
         pre-placement: when the timeline is empty, create the required tracks with add_track and \
         build the sequence with add_clip. Before finalizing visual or timing work, use the \
         cheapest relevant sense to verify it: prefer a schematic timeline map for placement and \
         timing, and a composited preview frame only when appearance or layering matters. Never \
         claim a check succeeded if a sense failed."
    )
}

/// The phase marker (a loop concern, not a wire command): lets a long
/// task land as several undo steps instead of one monolith.
pub fn commit_progress_spec() -> wire::ToolSpec {
    wire::ToolSpec {
        name: "commit_progress".into(),
        description: "Mark the edits so far as one completed phase so they land as \
                      their own undo step. Call this between logical stages of a \
                      long task; costs nothing."
            .into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {},
        }),
    }
}

/// Answer a `read_skill` call from the preloaded skill set. Unknown ids
/// get a model-readable rejection listing what exists.
pub(crate) fn read_skill_result(
    skills: &[crate::extend::Skill],
    arguments: &serde_json::Value,
) -> String {
    let id = arguments.get("id").and_then(|v| v.as_str()).unwrap_or("");
    match skills.iter().find(|s| s.id == id) {
        Some(skill) => format!("# {} ({})\n\n{}", skill.name, skill.id, skill.body),
        None => format!(
            "rejected: unknown skill '{id}'; available skills: {}",
            skills
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
