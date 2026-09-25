//! Marker tools: the named anchors on the timeline ruler.


tool_group! {
    "add_marker" => AddMarker(AddMarker),
        "Drop a named, colored marker on the timeline ruler at a position in seconds. Omit color to cycle the palette.";
    "remove_marker" => RemoveMarker(RemoveMarker),
        "Remove a ruler marker by id.";
    "set_marker" => SetMarker(SetMarker),
        "Move, rename, or recolor a ruler marker. Omitted fields keep their current value.";
}

#[cfg(test)]
use super::cases::{Expect, ToolCase};

#[cfg(test)]
pub(super) const CASES: &[ToolCase] = &[
    ToolCase {
        tool: "add_marker",
        args: |_| serde_json::json!({ "at": 4.0, "name": "cut here", "color": "red" }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::AddMarker {
            at: f.at(4.0),
            name: "cut here".into(),
            color: Some(aria_models::MarkerColor::Red),
        }),
        invalid: |_| serde_json::json!({ "at": -1.0 }),
        refusal: "at must not be negative",
    },
    ToolCase {
        tool: "remove_marker",
        args: |f| serde_json::json!({ "marker": f.marker }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::RemoveMarker {
            marker: f.marker_id(f.marker),
        }),
        invalid: |_| serde_json::json!({ "marker": 4_404 }),
        refusal: "marker 4404 does not exist",
    },
    ToolCase {
        tool: "set_marker",
        args: |f| serde_json::json!({ "marker": f.marker, "at": 5.0, "name": "outro" }),
        // Omitted fields keep the marker's current color and note.
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetMarker {
            marker: f.marker_id(f.marker),
            at: f.at(5.0),
            name: "outro".into(),
            color: aria_models::MarkerColor::Teal,
            comment: String::new(),
        }),
        invalid: |f| serde_json::json!({ "marker": f.marker, "at": -2.0 }),
        refusal: "at must not be negative",
    },
];

#[cfg(test)]
mod tests {
    use super::super::cases;

    #[test]
    fn marker_tools_lower_remap_and_refuse() {
        cases::check_group("markers", super::CASES);
    }
}
