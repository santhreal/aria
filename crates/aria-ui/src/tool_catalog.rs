//! Every timeline tool, as data.
//!
//! # Fail-by-default
//!
//! `index_of` is an exhaustive match over [`EditorTool`], so a new tool cannot
//! compile until it names a row, and [`ToolSpec`] has no `Default`, so the row
//! cannot be written without deciding every column.
//!

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

/// Direction a track-select tool reaches from the clicked position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackSelectDir {
    /// Select the clicked clip and every clip starting at or after it.
    Forward,
    /// Select the clicked clip and every clip ending at or before it.
    Backward,
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

/// The tool bar button a tool shares with its siblings.
///
/// The bar draws one button per group, in [`ToolGroup::ALL`] order. A group of
/// more than one tool lists the rest in the button's variants popover, and each
/// tool keeps its own key whichever member the button shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolGroup {
    Select,
    TrackSelect,
    Trim,
    Razor,
    Pen,
    Navigate,
    Type,
}

impl ToolGroup {
    /// Every group, in tool bar order.
    pub const ALL: [Self; 7] = [
        Self::Select,
        Self::TrackSelect,
        Self::Trim,
        Self::Razor,
        Self::Pen,
        Self::Navigate,
        Self::Type,
    ];

    /// The tools that share this group's button, in table order.
    pub fn tools(self) -> impl Iterator<Item = EditorTool> {
        tool_specs()
            .iter()
            .filter(move |spec| spec.group == self)
            .map(|spec| spec.tool)
    }

    /// The tool the button shows while `active` is armed: `active` when it is
    /// in the group, otherwise the group's first tool.
    pub fn shown(self, active: EditorTool) -> EditorTool {
        if active.group() == self {
            return active;
        }
        self.tools().next().unwrap_or(active)
    }
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
    /// The control id in the tool bar, which is also the tooltip key.
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
    /// What a click on a lane or clip selects along the timeline.
    pub track_select: Option<TrackSelectDir>,
    /// A click on a clip's rubber-band line adds a keyframe; in the Viewer, a
    /// click or drag draws or edits masks and motion-path keyframes.
    pub pens: bool,
    /// A click in the Viewer creates a text clip or places a caret inside one.
    pub types: bool,
    /// The tool bar button this tool shares with its siblings.
    pub group: ToolGroup,
}

/// The shipped tools, in tool bar order.
///
/// Rows are grouped in [`ToolGroup::ALL`] order, and a group's rows are in the
/// order its variants popover lists them; the first row of a group is the tool
/// its button shows while another group's tool is armed. `Pointer` is first
/// because it is the default and Escape returns to it.
const SHIPPED: &[ToolSpec] = &[
    ToolSpec {
        tool: EditorTool::Pointer,
        id: "tool-pointer",
        label: "Select",
        key: "v",
        icon: icons::POINTER,
        cursor: CursorStyle::Arrow,
        selects: true,
        pans: false,
        zooms: false,
        cuts: false,
        body: None,
        edge: None,
        track_select: None,
        pens: false,
        types: false,
        group: ToolGroup::Select,
    },
    ToolSpec {
        tool: EditorTool::TrackSelectForward,
        id: "tool-track-forward",
        label: "Track Select Forward",
        key: "a",
        icon: icons::CHEVRONS_RIGHT,
        cursor: CursorStyle::PointingHand,
        selects: false,
        pans: false,
        zooms: false,
        cuts: false,
        body: None,
        edge: None,
        track_select: Some(TrackSelectDir::Forward),
        pens: false,
        types: false,
        group: ToolGroup::TrackSelect,
    },
    ToolSpec {
        tool: EditorTool::TrackSelectBackward,
        id: "tool-track-backward",
        label: "Track Select Backward",
        key: "Shift+a",
        icon: icons::CHEVRONS_LEFT,
        cursor: CursorStyle::PointingHand,
        selects: false,
        pans: false,
        zooms: false,
        cuts: false,
        body: None,
        edge: None,
        track_select: Some(TrackSelectDir::Backward),
        pens: false,
        types: false,
        group: ToolGroup::TrackSelect,
    },
    ToolSpec {
        tool: EditorTool::Ripple,
        id: "tool-ripple",
        label: "Ripple Trim",
        key: "b",
        icon: icons::RIPPLE,
        cursor: CursorStyle::ResizeLeftRight,
        selects: false,
        pans: false,
        zooms: false,
        cuts: false,
        body: None,
        edge: Some(EdgeDrag::Ripple),
        track_select: None,
        pens: false,
        types: false,
        group: ToolGroup::Trim,
    },
    ToolSpec {
        tool: EditorTool::Roll,
        id: "tool-roll",
        label: "Rolling Edit",
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
        track_select: None,
        pens: false,
        types: false,
        group: ToolGroup::Trim,
    },
    ToolSpec {
        tool: EditorTool::Rate,
        id: "tool-rate",
        label: "Rate Stretch",
        key: "r",
        icon: icons::RATE,
        cursor: CursorStyle::ResizeLeftRight,
        selects: false,
        pans: false,
        zooms: false,
        cuts: false,
        body: None,
        edge: Some(EdgeDrag::Rate),
        track_select: None,
        pens: false,
        types: false,
        group: ToolGroup::Trim,
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
        track_select: None,
        pens: false,
        types: false,
        group: ToolGroup::Trim,
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
        track_select: None,
        pens: false,
        types: false,
        group: ToolGroup::Trim,
    },
    ToolSpec {
        tool: EditorTool::Razor,
        id: "tool-razor",
        label: "Razor",
        key: "c",
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
        track_select: None,
        pens: false,
        types: false,
        group: ToolGroup::Razor,
    },
    ToolSpec {
        tool: EditorTool::Pen,
        id: "tool-pen",
        label: "Pen",
        key: "p",
        icon: icons::PEN,
        cursor: CursorStyle::Crosshair,
        selects: false,
        pans: false,
        zooms: false,
        cuts: false,
        body: None,
        edge: None,
        track_select: None,
        pens: true,
        types: false,
        group: ToolGroup::Pen,
    },
    ToolSpec {
        tool: EditorTool::Hand,
        id: "tool-hand",
        label: "Hand",
        key: "h",
        icon: icons::HAND,
        cursor: CursorStyle::OpenHand,
        selects: false,
        pans: true,
        zooms: false,
        cuts: false,
        body: None,
        edge: None,
        track_select: None,
        pens: false,
        types: false,
        group: ToolGroup::Navigate,
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
        track_select: None,
        pens: false,
        types: false,
        group: ToolGroup::Navigate,
    },
    ToolSpec {
        tool: EditorTool::Type,
        id: "tool-type",
        label: "Type",
        key: "t",
        icon: icons::TYPE,
        cursor: CursorStyle::IBeam,
        selects: false,
        pans: false,
        zooms: false,
        cuts: false,
        body: None,
        edge: None,
        track_select: None,
        pens: false,
        types: true,
        group: ToolGroup::Type,
    },
];

/// Which row describes `tool`.
///
/// Exhaustive on purpose. This is the gate: a tool added to `types.rs` and
/// nowhere else does not compile.
const fn index_of(tool: EditorTool) -> usize {
    match tool {
        EditorTool::Pointer => 0,
        EditorTool::TrackSelectForward => 1,
        EditorTool::TrackSelectBackward => 2,
        EditorTool::Ripple => 3,
        EditorTool::Roll => 4,
        EditorTool::Rate => 5,
        EditorTool::Slip => 6,
        EditorTool::Slide => 7,
        EditorTool::Razor => 8,
        EditorTool::Pen => 9,
        EditorTool::Hand => 10,
        EditorTool::Zoom => 11,
        EditorTool::Type => 12,
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

    /// The control id in the tool bar, which is also the tooltip key.
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

    /// What a click on a lane or clip selects along the timeline.
    pub const fn track_select(self) -> Option<TrackSelectDir> {
        self.spec().track_select
    }

    /// A click on a clip's line adds a keyframe; in the Viewer, draws masks.
    pub const fn pens(self) -> bool {
        self.spec().pens
    }

    /// In the Viewer, creates or edits a text clip.
    pub const fn types(self) -> bool {
        self.spec().types
    }

    /// The tool bar button this tool shares with its siblings.
    pub const fn group(self) -> ToolGroup {
        self.spec().group
    }
}

#[cfg(test)]
#[path = "tool_catalog_tests.rs"]
mod tests;
