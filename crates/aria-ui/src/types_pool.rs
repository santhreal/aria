//! How the pool draws itself.
//!
//! A view is data, not three copies of the bin: every row is built by one
//! builder, and the view only says whether that row carries a picture, whether
//! it carries the facts line, and how the rows are laid out. A fourth view is a
//! variant plus its answers below, and the chips draw it without an edit in the
//! panel.
//!
//! The picture is the expensive part: drawing it asks the preview store for a
//! decode, so a view that answers `false` to [`PoolView::picture`] costs nothing
//! per item beyond its text. That is what makes the compact view worth having on
//! a pool of hundreds.

use serde::{Deserialize, Serialize};

/// The width of one cell in the wrapped view, in pixels.
///
/// Fixed rather than derived: the rail is resizable, and a cell that changed
/// width with it would reflow the whole pool on every drag of the divider.
pub const POOL_CELL_WIDTH: f32 = 96.0;

/// How a [`PoolView`] arranges its items geometrically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolLayout {
    /// Items stacked vertically one per row.
    Stack,
    /// Items flowed horizontally and wrapped into columns.
    Wrap,
    /// Items positioned freely on a 2D canvas.
    Freeform,
}

/// How the pool arranges its items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PoolView {
    /// One item per row, its picture at the left and its name and facts
    /// beside it.
    #[default]
    Tiles,
    /// One line per item: name on the left, facts on the right, no picture.
    List,
    /// Pictures in a wrapped grid, name underneath, no facts.
    Icons,
    /// 2D canvas where items can be positioned freely.
    Freeform,
}

impl PoolView {
    /// Every view, in the order the chips show them. The chips are generated
    /// from this, so a new view appears in the bin instead of existing as a
    /// variant nothing can reach.
    pub const ALL: &'static [Self] = &[Self::Tiles, Self::List, Self::Icons, Self::Freeform];

    pub fn label(self) -> &'static str {
        match self {
            Self::Tiles => "Tiles",
            Self::List => "List",
            Self::Icons => "Icons",
            Self::Freeform => "Freeform",
        }
    }

    /// Whether an item draws the source's picture, which is what costs a decode.
    pub fn picture(self) -> bool {
        match self {
            Self::Tiles | Self::Icons | Self::Freeform => true,
            Self::List => false,
        }
    }

    /// Whether an item draws its picture beside its text rather than above it.
    /// An item that fills the rail has room beside a thumbnail for its name
    /// and facts; a fixed-width cell has not, so it stacks them under the
    /// picture.
    pub fn picture_beside(self) -> bool {
        self.picture() && self.width().is_none()
    }

    /// Whether an item draws the facts line: duration, size, codec, proxy state.
    pub fn facts(self) -> bool {
        match self {
            Self::Tiles | Self::List => true,
            Self::Icons | Self::Freeform => false,
        }
    }

    /// Whether an item reads left to right instead of top to bottom.
    pub fn inline(self) -> bool {
        match self {
            Self::List => true,
            Self::Tiles | Self::Icons | Self::Freeform => false,
        }
    }

    /// Whether the pool wraps its items into columns instead of stacking them.
    pub fn wraps(self) -> bool {
        match self {
            Self::Icons => true,
            Self::Tiles | Self::List | Self::Freeform => false,
        }
    }

    /// Whether the pool allows free 2D positioning of items on a canvas.
    pub fn freeform(self) -> bool {
        match self {
            Self::Freeform => true,
            Self::Tiles | Self::List | Self::Icons => false,
        }
    }

    /// The fixed width of one item, or nothing when it fills the rail.
    pub fn width(self) -> Option<f32> {
        match self {
            Self::Icons | Self::Freeform => Some(POOL_CELL_WIDTH),
            Self::Tiles | Self::List => None,
        }
    }

    /// The geometric layout mode used to render items in this view.
    pub fn layout(self) -> PoolLayout {
        match self {
            Self::Tiles | Self::List => PoolLayout::Stack,
            Self::Icons => PoolLayout::Wrap,
            Self::Freeform => PoolLayout::Freeform,
        }
    }
}

/// Map a drop point in window coordinates to a pool item position relative to the canvas origin.
///
/// Ensures coordinates are non-negative and correctly offset against the canvas bounds.
pub fn freeform_drop_to_position(window_pos: (f32, f32), canvas_origin: (f32, f32)) -> (f32, f32) {
    (
        (window_pos.0 - canvas_origin.0).max(0.0),
        (window_pos.1 - canvas_origin.1).max(0.0),
    )
}

/// Deterministic placement for pool items in freeform view that do not have a stored position.
///
/// Unarranged items are placed in a 3-column grid ordered by their index in the visible pool,
/// with a 12px margin and 16px cell gap (cell width 96px, cell height 80px). This guarantees:
/// 1. Stable, predictable layout across renders without random jitter or overlap.
/// 2. Deterministic coordinates: item `i` lands at `(12 + (i % 3) * 112, 12 + (i / 3) * 96)`.
/// 3. Placed items remain in their dropped positions while unarranged items fill the grid.
pub fn freeform_default_position(index: usize) -> (f32, f32) {
    const MARGIN: f32 = 12.0;
    const CELL_W: f32 = POOL_CELL_WIDTH;
    const CELL_H: f32 = 80.0;
    const GAP_X: f32 = 16.0;
    const GAP_Y: f32 = 16.0;
    const COLS: usize = 3;

    let col = (index % COLS) as f32;
    let row = (index / COLS) as f32;
    let x = MARGIN + col * (CELL_W + GAP_X);
    let y = MARGIN + row * (CELL_H + GAP_Y);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_view_is_named_once() {
        let mut labels: Vec<&str> = PoolView::ALL.iter().map(|view| view.label()).collect();
        assert_eq!(labels.len(), 4);
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), PoolView::ALL.len(), "two views share a label");
        assert_eq!(PoolView::default(), PoolView::Tiles);
        assert!(PoolView::ALL.contains(&PoolView::default()));
    }

    /// A view that drew neither a picture nor the facts would be a column of
    /// names with no way to tell two takes apart.
    #[test]
    fn every_view_draws_something_about_the_item() {
        for view in PoolView::ALL {
            assert!(
                view.picture() || view.facts(),
                "{} draws nothing",
                view.label()
            );
        }
    }

    /// Every view variant enumerated from ALL at run time must have an explicit layout mode.
    #[test]
    fn every_view_has_a_layout() {
        for view in PoolView::ALL {
            let layout = view.layout();
            match layout {
                PoolLayout::Stack => {
                    assert!(
                        !view.wraps(),
                        "{} cannot wrap in stack layout",
                        view.label()
                    );
                    assert!(
                        !view.freeform(),
                        "{} cannot freeform in stack layout",
                        view.label()
                    );
                    assert!(
                        view.width().is_none(),
                        "{} has width in stack layout",
                        view.label()
                    );
                }
                PoolLayout::Wrap => {
                    assert!(view.wraps(), "{} must wrap in wrap layout", view.label());
                    assert!(
                        !view.freeform(),
                        "{} cannot freeform in wrap layout",
                        view.label()
                    );
                    assert!(
                        view.width().is_some(),
                        "{} must have width in wrap layout",
                        view.label()
                    );
                }
                PoolLayout::Freeform => {
                    assert!(
                        !view.wraps(),
                        "{} cannot wrap in freeform layout",
                        view.label()
                    );
                    assert!(
                        view.freeform(),
                        "{} must be freeform in freeform layout",
                        view.label()
                    );
                    assert!(
                        view.width().is_some(),
                        "{} must have width in freeform layout",
                        view.label()
                    );
                }
            }
        }
    }

    /// Wrapping needs a width: a cell that filled the rail would wrap to one
    /// column, which is the stacked view with extra steps.
    #[test]
    fn a_wrapping_view_has_a_width_and_a_stacking_one_does_not() {
        for view in PoolView::ALL {
            if view.wraps() {
                assert!(
                    view.width().is_some(),
                    "{} wraps but has no width",
                    view.label()
                );
                assert!(!view.inline(), "{} cannot wrap and inline", view.label());
            }
            if view.inline() {
                assert!(!view.wraps(), "{} cannot inline and wrap", view.label());
            }
        }
        assert!(PoolView::ALL.iter().any(|view| view.wraps()));
        assert!(PoolView::ALL.iter().any(|view| !view.picture()));
        assert!(PoolView::ALL.iter().any(|view| view.freeform()));
    }

    /// The saved spelling is the variant's own name, so a settings file written
    /// by this build reads the same way in the next one.
    #[test]
    fn the_saved_spelling_is_stable() {
        for view in PoolView::ALL {
            let text = serde_json::to_string(view).expect("serialize");
            assert_eq!(text, format!("\"{}\"", view.label().to_lowercase()));
            let back: PoolView = serde_json::from_str(&text).expect("deserialize");
            assert_eq!(back, *view);
        }
    }

    /// Window coordinates of a drop point map to relative, non-negative canvas coordinates.
    #[test]
    fn drop_point_maps_to_stored_position_window_coordinates_in_item_position_out() {
        let canvas_origin = (100.0, 50.0);

        // Inside canvas
        let pos = freeform_drop_to_position((250.0, 180.0), canvas_origin);
        assert_eq!(pos, (150.0, 130.0));

        // At origin
        let pos_origin = freeform_drop_to_position((100.0, 50.0), canvas_origin);
        assert_eq!(pos_origin, (0.0, 0.0));

        // Out of bounds top/left clamps to (0, 0)
        let pos_clamped = freeform_drop_to_position((40.0, 20.0), canvas_origin);
        assert_eq!(pos_clamped, (0.0, 0.0));
    }

    /// Items without stored positions receive deterministic, non-overlapping grid positions.
    #[test]
    fn unarranged_items_get_deterministic_placement() {
        let p0 = freeform_default_position(0);
        let p1 = freeform_default_position(1);
        let p2 = freeform_default_position(2);
        let p3 = freeform_default_position(3);

        assert_eq!(p0, (12.0, 12.0));
        assert_eq!(p1, (124.0, 12.0));
        assert_eq!(p2, (236.0, 12.0));
        assert_eq!(p3, (12.0, 108.0));

        // Deterministic repeat check
        assert_eq!(freeform_default_position(0), p0);
        assert_eq!(freeform_default_position(1), p1);

        // Ensure all first 9 slots are distinct
        let mut seen = std::collections::HashSet::new();
        for i in 0..9 {
            let (x, y) = freeform_default_position(i);
            let key = (x as i32, y as i32);
            assert!(seen.insert(key), "slot {i} collided with another slot");
        }
    }
}
