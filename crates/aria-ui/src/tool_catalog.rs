//! Every timeline tool, as data.
//!
//! A tool used to be an enum variant plus a hand-written arm wherever something
//! cared: its name, its control id, the key that selects it, the glyph in the
//! strip, the cursor over the lanes, and what a drag with it means. Seven
//! matches over one axis in five files, three of them wildcard-terminated. The
//! cursor one gave `Hand` and `Zoom` a cursor and let the other seven fall
//! through to an arrow; the glyph one shipped a bare letter for five tools,
//! which the SVG-only icon path could not draw at all, so five buttons rendered
//! blank.
//!
//! A row here carries the tool's identity and every predicate those arms were
//! deciding, including the ones written as identity checks against a specific
//! variant (`tool == EditorTool::Hand`) scattered over four files. Panning is a
//! column, not a variant name, so a tool that pans says so once.
//!
//! # Fail-by-default
//!
//! `index_of` is an exhaustive match over [`EditorTool`], so a new tool cannot
//! compile until it names a row, and [`ToolSpec`] has no `Default`, so the row
//! cannot be written without deciding every column.
//!
//! The two gesture columns are sub-axes rather than booleans. A body drag is
//! `Option<BodyDrag>` and an edge drag is `Option<EdgeDrag>`, so the handlers
//! match exhaustively on the sub-axis with `None` meaning the plain gesture —
//! no wildcard arm, and a new [`BodyDrag`] or [`EdgeDrag`] variant fails to
//! compile until both handlers implement it.
//!
//! [`tool_specs`] is the only list of tools in the crate. The strip, the key
//! audit, and the tooltip audit all read it, so a row appears in the UI by
//! existing rather than by being copied into a second array that could disagree
//! about how many tools there are.

use gpui::CursorStyle;

use crate::icons;
use crate::types::EditorTool;

/// What a drag on a clip body does, when it is not a plain move.
///
/// Both of these move frames rather than the clip: the distinction is which
/// side absorbs the change, which is why they write different edits and cannot
/// collapse into one column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyDrag {
    /// Move the take inside the clip. The clip's edges do not move.
    Slip,
    /// Move the clip and give the difference to its neighbours.
    Slide,
}

/// What a drag on a clip edge does, when it is not a plain trim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeDrag {
    /// Move the cut between two abutting clips.
    Roll,
    /// Change how long the clip plays without dropping any source.
    Rate,
    /// Trim the edge and move the rest of the lane so no gap is left.
    Ripple,
}

/// What one tool is and what a drag with it means.
///
/// Every column is a question some surface used to answer with its own match
/// arm or its own comparison against a variant. No `Default`: a new row must
/// decide all of them.
#[derive(Debug, Clone, Copy)]
pub struct ToolSpec {
    /// The tool this row describes.
    pub tool: EditorTool,
    /// The control id in the tool strip, which is also the tooltip key.
    pub id: &'static str,
    /// The name in the tooltip and the status line.
    pub label: &'static str,
    /// The single key that selects it, as the key table binds it.
    pub key: &'static str,
    /// The embedded asset path of the glyph in the strip.
    pub icon: &'static str,
    /// The cursor over the lanes while this tool is armed.
    pub cursor: CursorStyle,
    /// A press on empty lane space starts a marquee selection.
    ///
    /// Escape returns to the tool with this column set rather than to a named
    /// variant.
    pub selects: bool,
    /// A drag scrolls the timeline instead of moving anything in it.
    pub pans: bool,
    /// A drag changes the zoom instead of moving anything.
    pub zooms: bool,
    /// A click cuts every selected lane at the playhead.
    pub cuts: bool,
    /// What a drag on a clip body does. `None` is a plain move.
    pub body: Option<BodyDrag>,
    /// What a drag on a clip edge does. `None` is a plain trim.
    pub edge: Option<EdgeDrag>,
}

/// The shipped tools, in strip order.
///
/// Order is left-to-right in the strip and top-to-bottom in every list derived
/// from it. `Pointer` is first because it is the default and Escape returns to
/// it.
const SHIPPED: &[ToolSpec] = &[
    ToolSpec {
        tool: EditorTool::Pointer,
        id: "tool-pointer",
        label: "Select",
        key: "v",
        icon: icons::PLUS,
        cursor: CursorStyle::Arrow,
        selects: true,
        pans: false,
        zooms: false,
        cuts: false,
        body: None,
        edge: None,
    },
    ToolSpec {
        tool: EditorTool::Razor,
        id: "tool-razor",
        label: "Razor",
        key: "b",
        icon: icons::SCISSORS,
        // The cut lands on one exact frame, so the cursor is the one that shows
        // a position rather than the one that shows a hit target.
        cursor: CursorStyle::Crosshair,
        selects: false,
        pans: false,
        zooms: false,
        cuts: true,
        body: None,
        edge: None,
    },
    ToolSpec {
        tool: EditorTool::Hand,
        id: "tool-hand",
        label: "Hand",
        key: "h",
        icon: icons::FIT,
        cursor: CursorStyle::OpenHand,
        selects: false,
        pans: true,
        zooms: false,
        cuts: false,
        body: None,
        edge: None,
    },
    ToolSpec {
        tool: EditorTool::Zoom,
        id: "tool-zoom",
        label: "Zoom",
        key: "z",
        icon: icons::ZOOM_IN,
        cursor: CursorStyle::Crosshair,
        selects: false,
        pans: false,
        zooms: true,
        cuts: false,
        body: None,
        edge: None,
    },
    ToolSpec {
        tool: EditorTool::Slip,
        id: "tool-slip",
        label: "Slip",
        key: "y",
        icon: icons::SLIP,
        // Horizontal both ways: the take can move either direction inside a
        // clip whose edges do not move.
        cursor: CursorStyle::ResizeLeftRight,
        selects: false,
        pans: false,
        zooms: false,
        cuts: false,
        body: Some(BodyDrag::Slip),
        edge: None,
    },
    ToolSpec {
        tool: EditorTool::Slide,
        id: "tool-slide",
        label: "Slide",
        key: "u",
        icon: icons::SLIDE,
        cursor: CursorStyle::ResizeLeftRight,
        selects: false,
        pans: false,
        zooms: false,
        cuts: false,
        body: Some(BodyDrag::Slide),
        edge: None,
    },
    ToolSpec {
        tool: EditorTool::Roll,
        id: "tool-roll",
        label: "Rolling edit",
        key: "n",
        icon: icons::ROLL,
        // A roll drags the divider between two abutting clips, which is what
        // `col-resize` means everywhere else.
        cursor: CursorStyle::ResizeColumn,
        selects: false,
        pans: false,
        zooms: false,
        cuts: false,
        body: None,
        edge: Some(EdgeDrag::Roll),
    },
    ToolSpec {
        tool: EditorTool::Rate,
        id: "tool-rate",
        label: "Rate stretch",
        key: "r",
        icon: icons::RATE,
        cursor: CursorStyle::ResizeLeftRight,
        selects: false,
        pans: false,
        zooms: false,
        cuts: false,
        body: None,
        edge: Some(EdgeDrag::Rate),
    },
    ToolSpec {
        tool: EditorTool::Ripple,
        id: "tool-ripple",
        label: "Ripple trim",
        key: "p",
        icon: icons::RIPPLE,
        cursor: CursorStyle::ResizeLeftRight,
        selects: false,
        pans: false,
        zooms: false,
        cuts: false,
        body: None,
        edge: Some(EdgeDrag::Ripple),
    },
];

/// Which row describes `tool`.
///
/// Exhaustive on purpose. This is the gate: a tool added to `types.rs` and
/// nowhere else does not compile.
const fn index_of(tool: EditorTool) -> usize {
    match tool {
        EditorTool::Pointer => 0,
        EditorTool::Razor => 1,
        EditorTool::Hand => 2,
        EditorTool::Zoom => 3,
        EditorTool::Slip => 4,
        EditorTool::Slide => 5,
        EditorTool::Roll => 6,
        EditorTool::Rate => 7,
        EditorTool::Ripple => 8,
    }
}

/// Every tool, in strip order.
///
/// The only list of tools in the crate. The strip is drawn from it, so a new
/// tool appears there by existing.
pub const fn tool_specs() -> &'static [ToolSpec] {
    SHIPPED
}

/// The row describing `tool`.
pub const fn tool_spec(tool: EditorTool) -> &'static ToolSpec {
    &SHIPPED[index_of(tool)]
}

/// The row with this control id, for surfaces that address a tool by name.
pub fn tool_spec_by_id(id: &str) -> Option<&'static ToolSpec> {
    SHIPPED.iter().find(|spec| spec.id == id)
}

/// Everything a surface can ask a tool, answered by its row.
///
/// These live here rather than beside the enum because every answer is a
/// column: a method that did not read the row would be a second place a tool
/// gets described, which is the drift this file exists to remove. It also keeps
/// `gpui` out of [`crate::types`], which holds plain view-model data.
impl EditorTool {
    /// Every tool, in strip order.
    ///
    /// Reads [`tool_specs`], the one list of tools, so a tool cannot be in the
    /// enum and missing from the strip, the key table, or their tests.
    pub fn all() -> impl Iterator<Item = Self> {
        tool_specs().iter().map(|spec| spec.tool)
    }

    /// The row describing this tool.
    pub const fn spec(self) -> &'static ToolSpec {
        tool_spec(self)
    }

    /// The name in the tooltip and the status line.
    pub const fn label(self) -> &'static str {
        self.spec().label
    }

    /// The control id in the tool strip, which is also the tooltip key.
    pub const fn control_id(self) -> &'static str {
        self.spec().id
    }

    /// The single key that selects it, as the key table binds it.
    pub const fn key(self) -> &'static str {
        self.spec().key
    }

    /// The glyph in the strip.
    pub const fn icon(self) -> &'static str {
        self.spec().icon
    }

    /// The cursor over the lanes while this tool is armed.
    ///
    /// Every tool names one. This replaced a match that gave `Hand` and `Zoom`
    /// a cursor and let the other seven fall through to an arrow.
    pub const fn cursor(self) -> CursorStyle {
        self.spec().cursor
    }

    /// A press on empty lane space starts a marquee selection.
    pub const fn selects(self) -> bool {
        self.spec().selects
    }

    /// A drag scrolls the timeline instead of moving anything in it.
    pub const fn pans(self) -> bool {
        self.spec().pans
    }

    /// A drag changes the zoom instead of moving anything.
    pub const fn zooms(self) -> bool {
        self.spec().zooms
    }

    /// A click cuts every selected lane at the playhead.
    pub const fn cuts(self) -> bool {
        self.spec().cuts
    }

    /// What a drag on a clip body does. `None` is a plain move.
    pub const fn body_drag(self) -> Option<BodyDrag> {
        self.spec().body
    }

    /// What a drag on a clip edge does. `None` is a plain trim.
    pub const fn edge_drag(self) -> Option<EdgeDrag> {
        self.spec().edge
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exhaustive `index_of` match is the compile-time gate; this is the
    /// proof that each arm points at its *own* row rather than a neighbour's,
    /// which would compile and silently give a tool another tool's cursor,
    /// glyph, and key.
    #[test]
    fn a_row_per_tool() {
        for (index, spec) in SHIPPED.iter().enumerate() {
            assert_eq!(
                index_of(spec.tool),
                index,
                "the '{}' row resolves to '{}' instead",
                spec.id,
                SHIPPED[index_of(spec.tool)].id
            );
            assert_eq!(
                tool_spec(spec.tool).id,
                spec.id,
                "{:?} does not resolve to its own row",
                spec.tool
            );
        }
    }

    /// Two tools sharing a key or a control id would make one of them
    /// unreachable: the key table would bind the first and the strip would
    /// mount two buttons with one id.
    #[test]
    fn keys_and_ids_are_unique() {
        let count = SHIPPED.len();

        let mut keys: Vec<&str> = SHIPPED.iter().map(|spec| spec.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count, "two tools claim the same key");

        let mut ids: Vec<&str> = SHIPPED.iter().map(|spec| spec.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "two tools claim the same control id");
    }

    /// Every row has something to draw and something to say. An empty label
    /// renders an unnamed button and an empty key binds nothing.
    #[test]
    fn every_row_is_drawable() {
        for spec in SHIPPED {
            assert!(!spec.label.is_empty(), "{:?} has no name", spec.tool);
            assert!(!spec.key.is_empty(), "{:?} has no key", spec.tool);
        }
    }

    /// Every glyph is a registered asset.
    ///
    /// The icon path is SVG-only — `icons::icon` builds `svg().path(..)` and an
    /// unregistered path loads as `None`, which draws a blank button rather than
    /// failing. Five tools shipped bare letters (`"Y"`, `"U"`, …) and rendered
    /// as five empty squares in the strip, so this asserts the glyph is
    /// something the renderer can actually find.
    #[test]
    fn every_glyph_is_a_loadable_asset() {
        use gpui::AssetSource as _;

        for spec in SHIPPED {
            let bytes = crate::icons::Assets
                .load(spec.icon)
                .unwrap_or_else(|err| panic!("{:?} glyph {} errored: {err}", spec.tool, spec.icon));
            assert!(
                bytes.is_some(),
                "{:?} glyph {} is not a registered asset, so its button renders blank",
                spec.tool,
                spec.icon
            );
        }
    }

    /// Two tools sharing a glyph would be indistinguishable in the strip.
    #[test]
    fn glyphs_are_unique() {
        let mut icons: Vec<&str> = SHIPPED.iter().map(|spec| spec.icon).collect();
        let count = icons.len();
        icons.sort_unstable();
        icons.dedup();
        assert_eq!(icons.len(), count, "two tools draw the same glyph");
    }

    /// The gesture columns are mutually exclusive by construction: a press is
    /// dispatched by asking these in order, so two set on one row would make the
    /// later one dead.
    #[test]
    fn one_gesture_per_tool() {
        for spec in SHIPPED {
            let claimed = u8::from(spec.selects)
                + u8::from(spec.pans)
                + u8::from(spec.zooms)
                + u8::from(spec.cuts)
                + u8::from(spec.body.is_some())
                + u8::from(spec.edge.is_some());
            assert_eq!(
                claimed, 1,
                "{:?} claims {claimed} gestures; exactly one wins a press",
                spec.tool
            );
        }
    }

    /// Exactly one tool starts a marquee, and it is the default, because Escape
    /// returns to whichever tool this column names.
    #[test]
    fn exactly_one_tool_selects() {
        let selecting: Vec<EditorTool> = SHIPPED
            .iter()
            .filter(|spec| spec.selects)
            .map(|spec| spec.tool)
            .collect();
        assert_eq!(selecting, vec![EditorTool::default()]);
    }

    /// Each sub-axis member is claimed by exactly one tool. A member no tool
    /// claims is a gesture the UI cannot reach; two tools claiming one member
    /// would make the second unreachable.
    #[test]
    fn every_gesture_variant_has_one_tool() {
        let body: Vec<BodyDrag> = SHIPPED.iter().filter_map(|spec| spec.body).collect();
        assert_eq!(body, vec![BodyDrag::Slip, BodyDrag::Slide]);

        let edge: Vec<EdgeDrag> = SHIPPED.iter().filter_map(|spec| spec.edge).collect();
        assert_eq!(edge, vec![EdgeDrag::Roll, EdgeDrag::Rate, EdgeDrag::Ripple]);
    }

    #[test]
    fn a_control_id_finds_its_row() {
        assert_eq!(
            tool_spec_by_id("tool-roll").map(|spec| spec.tool),
            Some(EditorTool::Roll)
        );
        assert!(tool_spec_by_id("tool-nonesuch").is_none());
    }
}
