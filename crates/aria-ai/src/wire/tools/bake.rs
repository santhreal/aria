//! Bake tools: turning one clip into a file, and putting it back.
//!
//! `render_and_replace` is the one row whose work the turn does not do. The
//! file it swaps in does not exist yet and writing it takes longer than a turn
//! lasts, so the row is checked here and started by the shell, and the model is
//! told it started rather than that it finished. Everything a model needs to
//! decide the call is still decided here: which clip, whether that clip is on a
//! lane a render can stand in for, and whether a render of it is worth making.


tool_group! {
    "render_and_replace" => RenderAndReplace(RenderAndReplace),
        "Render one clip to a file and decode that file in its place (Premiere's 'Render and Replace'), so a deep effect stack is composited once instead of on every playback. The render is a background job: this call starts it and reports that it started, and the clip is replaced when the file is written. Refused for a clip that is already rendered, a clip with nothing applied to it (rendering it would only re-encode the same picture), and a lane that is neither picture nor sound. Use restore_unrendered to put the original clip back.";
    "restore_unrendered" => RestoreUnrendered(RestoreUnrendered),
        "Put back the clip a render replaced (Premiere's 'Restore Unrendered'), keeping whatever trimming and moving happened while it was rendered. Refused for a clip that was not rendered.";
}

#[cfg(test)]
use super::cases::{Expect, ToolCase};

#[cfg(test)]
pub(super) const CASES: &[ToolCase] = &[
    ToolCase {
        tool: "render_and_replace",
        args: |f| serde_json::json!({ "clip": f.clip }),
        // Not an edit: the row leaves with the work the shell has to start.
        expect: Expect::Queued(|f| crate::validate::QueuedWork::RenderAndReplace {
            clip: f.clip_id(f.clip),
        }),
        // The fixture's second clip carries nothing, so rendering it would
        // re-encode the file that is already there.
        invalid: |f| serde_json::json!({ "clip": f.next }),
        refusal: "would only re-encode it",
    },
    ToolCase {
        tool: "restore_unrendered",
        args: |f| serde_json::json!({ "clip": f.baked }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::RestoreBakedClip {
            clip: f.clip_id(f.baked),
        }),
        // A clip nothing rendered has nothing to put back.
        invalid: |f| serde_json::json!({ "clip": f.clip }),
        refusal: "was not rendered",
    },
];

#[cfg(test)]
mod tests {
    use super::super::cases;

    #[test]
    fn bake_tools_lower_remap_and_refuse() {
        cases::check_group("bake", super::CASES);
    }
}
