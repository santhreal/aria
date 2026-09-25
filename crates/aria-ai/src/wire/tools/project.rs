//! Project tools: settings that belong to the sequence rather than a clip.


tool_group! {
    "set_canvas" => SetCanvas(SetCanvas),
        "Set the project canvas: aspect ratio preset ('auto' follows the footage; '16:9', '9:16', '1:1', '4:5', '21:9' reshape it), the background color shown where no clip covers the canvas, and/or an explicit output size in pixels ([width, height], each edge even and 16-8192; [0, 0] derives the size from the aspect or the footage). Omitted fields keep their current value.";
}

#[cfg(test)]
use super::cases::{Expect, ToolCase};

#[cfg(test)]
pub(super) const CASES: &[ToolCase] = &[ToolCase {
    tool: "set_canvas",
    args: |_| serde_json::json!({ "aspect": "9:16", "background": [10, 10, 10] }),
    // An omitted resolution keeps whatever the canvas already derives.
    expect: Expect::Edit(|f| aria_commands::EditCommand::SetCanvas {
        aspect: aria_models::CanvasAspect::Tall9x16,
        background: [10, 10, 10],
        resolution: f.project.timeline().canvas().resolution,
    }),
    // An odd edge cannot be encoded, so the refusal names the rule.
    invalid: |_| serde_json::json!({ "resolution": [1921, 1080] }),
    refusal: "is not encodable",
}];

#[cfg(test)]
mod tests {
    use super::super::cases;

    #[test]
    fn project_tools_lower_remap_and_refuse() {
        cases::check_group("project", super::CASES);
    }
}
