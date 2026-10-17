//! Declarative UI panel host adapter and flat `list<ui-node>` validator for WASM plugins.

use std::sync::Arc;

use aria_plugin::{
    MAX_TREE_NODES, MAX_WIDGET_ID_BYTES, PanelEvent, PanelLocation, PanelSpec, PanelView,
    PluginContext, Tone, UiNode,
};

use crate::bindings;
use crate::host::WasmPluginShared;

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Unvisited,
    Visiting,
    Visited,
}

pub(crate) struct WasmPanelView {
    shared: Arc<WasmPluginShared>,
    id: String,
}

impl PanelView for WasmPanelView {
    fn view(&self, ctx: &PluginContext) -> UiNode {
        let flat_nodes = self.shared.call_panel_view(ctx, &self.id);
        match decode_flat_ui_tree(&flat_nodes) {
            Ok(tree) => tree,
            Err(reason) => refused_flat_tree_sentinel(&reason),
        }
    }

    fn event(&self, ctx: &PluginContext, event: &PanelEvent) -> Result<(), String> {
        let wit_event = to_wit_panel_event(event);
        self.shared.call_panel_event(ctx, &self.id, &wit_event)
    }
}

pub(crate) fn register_wasm_panel(
    ctx: &PluginContext,
    shared: Arc<WasmPluginShared>,
    spec: bindings::PanelSpec,
) -> Result<(), String> {
    let id = spec.id.clone();
    let host_spec = PanelSpec {
        id: spec.id,
        title: spec.title.into(),
        location: map_panel_location(spec.location),
    };
    ctx.register_panel(host_spec, WasmPanelView { shared, id })
        .map(|_| ())
        .map_err(|err| err.to_string())
}

pub(crate) fn map_panel_location(location: bindings::PanelLocation) -> PanelLocation {
    match location {
        bindings::PanelLocation::Sidebar => PanelLocation::Sidebar,
        bindings::PanelLocation::Inspector => PanelLocation::Inspector,
        bindings::PanelLocation::Timeline => PanelLocation::Timeline,
    }
}

pub(crate) fn map_tone(tone: bindings::Tone) -> Tone {
    match tone {
        bindings::Tone::Normal => Tone::Normal,
        bindings::Tone::Muted => Tone::Muted,
        bindings::Tone::Success => Tone::Success,
        bindings::Tone::Warning => Tone::Warning,
        bindings::Tone::Danger => Tone::Danger,
    }
}

pub(crate) fn to_wit_panel_event(event: &PanelEvent) -> bindings::PanelEvent {
    match event {
        PanelEvent::Pressed { id } => bindings::PanelEvent::Pressed(id.to_string()),
        PanelEvent::Toggled { id, on } => bindings::PanelEvent::Toggled(bindings::PanelToggled {
            id: id.to_string(),
            on: *on,
        }),
        PanelEvent::Slid { id, value } => bindings::PanelEvent::Slid(bindings::PanelSlid {
            id: id.to_string(),
            value: *value,
        }),
        PanelEvent::Submitted { id, text } => {
            bindings::PanelEvent::Submitted(bindings::PanelSubmitted {
                id: id.to_string(),
                text: text.to_string(),
            })
        }
        PanelEvent::Chose { id, index } => bindings::PanelEvent::Chose(bindings::PanelChose {
            id: id.to_string(),
            index: u32::try_from(*index).unwrap_or(u32::MAX),
        }),
    }
}

fn node_children(node: &bindings::UiNode) -> &[u32] {
    match node {
        bindings::UiNode::Column(children) | bindings::UiNode::Row(children) => children,
        _ => &[],
    }
}

/// Validate a flattened `list<ui-node>` (`root = index 0`) and build the [`UiNode`] tree.
///
/// Rejects an empty list, out-of-range child index, cycle, shared/bad child index, or
/// non-root orphan with an error naming the offending node index before constructing
/// any [`UiNode`].
pub(crate) fn decode_flat_ui_tree(nodes: &[bindings::UiNode]) -> Result<UiNode, String> {
    if nodes.is_empty() {
        return Err("node 0: flat ui-node list is empty".to_owned());
    }
    if nodes.len() > MAX_TREE_NODES {
        return Err(format!(
            "node {MAX_TREE_NODES}: flat ui-node list length {} exceeds limit {MAX_TREE_NODES}",
            nodes.len()
        ));
    }

    let len = nodes.len();
    for (idx, node) in nodes.iter().enumerate() {
        for &child_raw in node_children(node) {
            let child_idx = child_raw as usize;
            if child_idx >= len {
                return Err(format!(
                    "node {idx}: child index {child_raw} is out of range (len {len})"
                ));
            }
        }
    }

    let mut state = vec![VisitState::Unvisited; len];
    let mut stack: Vec<(usize, usize)> = Vec::with_capacity(len);
    let mut post_order: Vec<usize> = Vec::with_capacity(len);

    state[0] = VisitState::Visiting;
    stack.push((0, 0));

    while let Some((node_idx, child_pos)) = stack.last_mut() {
        let parent = *node_idx;
        let children = node_children(&nodes[parent]);
        if *child_pos < children.len() {
            let child_idx = children[*child_pos] as usize;
            *child_pos += 1;
            match state[child_idx] {
                VisitState::Visiting => {
                    return Err(format!(
                        "node {parent}: cycle detected through child index {child_idx}"
                    ));
                }
                VisitState::Visited => {
                    return Err(format!(
                        "node {parent}: bad child index {child_idx} (already has a parent)"
                    ));
                }
                VisitState::Unvisited => {
                    state[child_idx] = VisitState::Visiting;
                    stack.push((child_idx, 0));
                }
            }
        } else {
            state[parent] = VisitState::Visited;
            post_order.push(parent);
            stack.pop();
        }
    }

    for (idx, visit) in state.iter().enumerate() {
        if *visit != VisitState::Visited {
            return Err(format!(
                "node {idx}: non-root orphan is not reachable from root 0"
            ));
        }
    }

    let mut built: Vec<Option<UiNode>> = (0..len).map(|_| None).collect();
    for idx in post_order {
        let ui_node = match &nodes[idx] {
            bindings::UiNode::Column(children) => {
                let mut child_nodes = Vec::with_capacity(children.len());
                for &child_raw in children {
                    let child = built[child_raw as usize]
                        .take()
                        .ok_or_else(|| format!("node {idx}: missing child {child_raw}"))?;
                    child_nodes.push(child);
                }
                UiNode::Column {
                    children: child_nodes,
                }
            }
            bindings::UiNode::Row(children) => {
                let mut child_nodes = Vec::with_capacity(children.len());
                for &child_raw in children {
                    let child = built[child_raw as usize]
                        .take()
                        .ok_or_else(|| format!("node {idx}: missing child {child_raw}"))?;
                    child_nodes.push(child);
                }
                UiNode::Row {
                    children: child_nodes,
                }
            }
            bindings::UiNode::Heading(text) => UiNode::Heading {
                text: Arc::from(text.as_str()),
            },
            bindings::UiNode::Text(text) => UiNode::Text {
                text: Arc::from(text.text.as_str()),
                tone: map_tone(text.tone),
            },
            bindings::UiNode::Field(field) => UiNode::Field {
                label: Arc::from(field.label.as_str()),
                value: Arc::from(field.value.as_str()),
            },
            bindings::UiNode::Separator => UiNode::Separator,
            bindings::UiNode::Button(button) => UiNode::Button {
                id: Arc::from(button.id.as_str()),
                label: Arc::from(button.label.as_str()),
                disabled: button.disabled,
            },
            bindings::UiNode::Toggle(toggle) => UiNode::Toggle {
                id: Arc::from(toggle.id.as_str()),
                label: Arc::from(toggle.label.as_str()),
                on: toggle.on,
            },
            bindings::UiNode::Slider(slider) => UiNode::Slider {
                id: Arc::from(slider.id.as_str()),
                label: Arc::from(slider.label.as_str()),
                value: slider.value,
                min: slider.min,
                max: slider.max,
                step: slider.step,
            },
            bindings::UiNode::Input(input) => UiNode::Input {
                id: Arc::from(input.id.as_str()),
                value: Arc::from(input.value.as_str()),
                placeholder: Arc::from(input.placeholder.as_str()),
            },
            bindings::UiNode::Choice(choice) => UiNode::Choice {
                id: Arc::from(choice.id.as_str()),
                label: Arc::from(choice.label.as_str()),
                options: choice
                    .options
                    .iter()
                    .map(|opt| Arc::from(opt.as_str()))
                    .collect(),
                selected: choice.selected as usize,
            },
            bindings::UiNode::Progress(progress) => UiNode::Progress {
                label: Arc::from(progress.label.as_str()),
                fraction: progress.fraction,
            },
            bindings::UiNode::Image(image) => UiNode::Image {
                width: image.width,
                height: image.height,
                rgba: Arc::from(image.rgba.as_slice()),
            },
        };
        built[idx] = Some(ui_node);
    }

    built[0]
        .take()
        .ok_or_else(|| "node 0: missing root ui-node".to_owned())
}

/// Construct a sentinel [`UiNode`] that [`UiNode::check`] refuses with an error
/// carrying `reason` (including the offending flat-list node index) so the host
/// keeps the last valid panel tree and records a `PluginFault` without unmounting
/// the plugin.
fn refused_flat_tree_sentinel(reason: &str) -> UiNode {
    let sanitized: String = reason
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_WIDGET_ID_BYTES)
        .collect();
    let id = if sanitized.trim().is_empty() {
        "node 0: malformed flat ui-node list".to_owned()
    } else {
        sanitized
    };
    UiNode::Choice {
        id: Arc::from(id),
        label: Arc::from(""),
        options: Vec::new(),
        selected: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_flat_ui_tree_rejects_empty_out_of_range_cycle_shared_and_orphan_nodes() {
        let empty = decode_flat_ui_tree(&[]).expect_err("empty list must fail");
        assert!(empty.contains("node 0"), "{empty}");

        let out_of_range = decode_flat_ui_tree(&[bindings::UiNode::Column(vec![3])])
            .expect_err("out of range child index must fail");
        assert!(out_of_range.contains("node 0"), "{out_of_range}");
        assert!(out_of_range.contains("child index 3"), "{out_of_range}");

        let self_cycle = decode_flat_ui_tree(&[bindings::UiNode::Column(vec![0])])
            .expect_err("self cycle must fail");
        assert!(self_cycle.contains("node 0"), "{self_cycle}");
        assert!(self_cycle.contains("cycle"), "{self_cycle}");

        let two_node_cycle = decode_flat_ui_tree(&[
            bindings::UiNode::Column(vec![1]),
            bindings::UiNode::Row(vec![0]),
        ])
        .expect_err("two-node cycle must fail");
        assert!(two_node_cycle.contains("node 1"), "{two_node_cycle}");
        assert!(two_node_cycle.contains("cycle"), "{two_node_cycle}");

        let shared_child = decode_flat_ui_tree(&[
            bindings::UiNode::Column(vec![1, 2]),
            bindings::UiNode::Row(vec![2]),
            bindings::UiNode::Separator,
        ])
        .expect_err("shared child (DAG) must fail");
        assert!(shared_child.contains("node 0"), "{shared_child}");
        assert!(shared_child.contains("bad child index 2"), "{shared_child}");

        let orphan = decode_flat_ui_tree(&[
            bindings::UiNode::Column(vec![1]),
            bindings::UiNode::Heading("kept".to_owned()),
            bindings::UiNode::Separator,
        ])
        .expect_err("orphan node must fail");
        assert!(orphan.contains("node 2"), "{orphan}");
        assert!(orphan.contains("orphan"), "{orphan}");
    }

    #[test]
    fn decode_flat_ui_tree_builds_all_variants_and_passes_host_check() {
        let flat = vec![
            bindings::UiNode::Column(vec![1, 2, 3, 4]),
            bindings::UiNode::Heading("Header".to_owned()),
            bindings::UiNode::Row(vec![5, 6, 7, 8, 9]),
            bindings::UiNode::Text(bindings::UiText {
                text: "Status".to_owned(),
                tone: bindings::Tone::Success,
            }),
            bindings::UiNode::Image(bindings::UiImage {
                width: 1,
                height: 1,
                rgba: vec![255, 0, 128, 255],
            }),
            bindings::UiNode::Field(bindings::UiField {
                label: "Key".to_owned(),
                value: "Val".to_owned(),
            }),
            bindings::UiNode::Separator,
            bindings::UiNode::Button(bindings::UiButton {
                id: "btn".to_owned(),
                label: "Click".to_owned(),
                disabled: false,
            }),
            bindings::UiNode::Toggle(bindings::UiToggle {
                id: "tog".to_owned(),
                label: "Enable".to_owned(),
                on: true,
            }),
            bindings::UiNode::Choice(bindings::UiChoice {
                id: "mode".to_owned(),
                label: "Mode".to_owned(),
                options: vec!["A".to_owned(), "B".to_owned()],
                selected: 1,
            }),
        ];
        let tree = decode_flat_ui_tree(&flat).expect("valid flat tree");
        tree.check().expect("host UiNode::check passes");
    }
}
