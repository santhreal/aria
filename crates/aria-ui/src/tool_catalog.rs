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
    /// Whether the tool button shows a 3 px corner triangle and opens a
    /// variants popover on a 300 ms press-and-hold or triangle click.
    pub has_variants: bool,
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
        track_select: None,
        pens: false,
        types: false,
        has_variants: false,
    },
    ToolSpec {
        tool: EditorTool::TrackSelectForward,
        id: "tool-track-forward",
        label: "Track Select Forward",
        key: "a",
        icon: icons::CHEVRON_RIGHT,
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
        has_variants: false,
    },
    ToolSpec {
        tool: EditorTool::TrackSelectBackward,
        id: "tool-track-backward",
        label: "Track Select Backward",
        key: "Shift+a",
        icon: icons::CHEVRON_LEFT,
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
        has_variants: false,
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
        has_variants: false,
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
        track_select: None,
        pens: false,
        types: false,
        has_variants: false,
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
        has_variants: false,
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
        has_variants: true,
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
        has_variants: true,
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
        track_select: None,
        pens: false,
        types: false,
        has_variants: true,
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
        track_select: None,
        pens: false,
        types: false,
        has_variants: true,
    },
    ToolSpec {
        tool: EditorTool::Ripple,
        id: "tool-ripple",
        label: "Ripple trim",
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
        has_variants: true,
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
        has_variants: true,
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
        has_variants: false,
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
        EditorTool::Razor => 3,
        EditorTool::Hand => 4,
        EditorTool::Zoom => 5,
        EditorTool::Slip => 6,
        EditorTool::Slide => 7,
        EditorTool::Roll => 8,
        EditorTool::Rate => 9,
        EditorTool::Ripple => 10,
        EditorTool::Pen => 11,
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

    /// Whether the tool button shows a 3 px corner triangle for variants.
    pub const fn has_variants(self) -> bool {
        self.spec().has_variants
    }

    /// Sibling tools offered in the variants popover for a trim tool.
    pub const fn trim_variants(self) -> &'static [Self] {
        match self {
            Self::Slip | Self::Slide | Self::Roll | Self::Rate | Self::Ripple => &[
                Self::Ripple,
                Self::Roll,
                Self::Rate,
                Self::Slip,
                Self::Slide,
            ],
            _ => &[],
        }
    }
}

#[cfg(test)]
#[path = "tool_catalog_tests.rs"]
mod tests;
