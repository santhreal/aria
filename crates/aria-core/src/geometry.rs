//! Frame geometry: the visible region within a (possibly padded) decode buffer,
//! and the display rotation a container can attach.

/// An axis-aligned rectangle in pixels, used for the visible/crop region inside
/// a coded frame buffer.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// A rectangle at the origin covering `width × height`.
    pub const fn from_size(width: u32, height: u32) -> Self {
        Self::new(0, 0, width, height)
    }

    pub fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Saturating right coordinate `x + width`.
    pub const fn right(self) -> u32 {
        self.x.saturating_add(self.width)
    }

    /// Saturating bottom coordinate `y + height`.
    pub const fn bottom(self) -> u32 {
        self.y.saturating_add(self.height)
    }

    /// Checked right coordinate `x + width`, returning `None` on overflow.
    pub const fn right_checked(self) -> Option<u32> {
        self.x.checked_add(self.width)
    }

    /// Checked bottom coordinate `y + height`, returning `None` on overflow.
    pub const fn bottom_checked(self) -> Option<u32> {
        self.y.checked_add(self.height)
    }

    /// Returns true if the half-open rectangle `[x, x+w) × [y, y+h)` contains `(px, py)`.
    pub fn contains_point(self, px: u32, py: u32) -> bool {
        if self.is_empty() {
            return false;
        }
        let Some(r) = self.right_checked() else {
            return px >= self.x && py >= self.y && py < self.bottom();
        };
        let Some(b) = self.bottom_checked() else {
            return px >= self.x && px < r && py >= self.y;
        };
        px >= self.x && px < r && py >= self.y && py < b
    }

    /// Returns true if `self` and `other` overlap with non-zero area.
    pub fn intersects(self, other: Rect) -> bool {
        self.intersection(other).is_some()
    }

    /// Returns the intersection rectangle if `self` and `other` overlap with non-zero area.
    pub fn intersection(self, other: Rect) -> Option<Rect> {
        if self.is_empty() || other.is_empty() {
            return None;
        }
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());
        if x1 < x2 && y1 < y2 {
            Some(Rect::new(x1, y1, x2 - x1, y2 - y1))
        } else {
            None
        }
    }
}

/// Display rotation applied to a frame before it's shown, in degrees clockwise.
///
/// Phones record sensor-native and stash the orientation in container metadata;
/// honoring it is the difference between upright and sideways playback, so it
/// rides with the frame rather than being baked into pixels at decode time.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Rotation {
    #[default]
    None,
    Cw90,
    Cw180,
    Cw270,
}

impl Rotation {
    /// Every rotation a container can attach, in increasing degrees.
    ///
    /// A surface that offers an orientation walks this instead of writing
    /// `[0, 90, 180, 270]`, so the chips and [`Rotation::from_degrees`] cannot
    /// disagree about what the choices are.
    pub const ALL: [Rotation; 4] = [
        Rotation::None,
        Rotation::Cw90,
        Rotation::Cw180,
        Rotation::Cw270,
    ];

    /// True when the rotation swaps width and height (90° or 270°).
    pub const fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Cw90 | Rotation::Cw270)
    }

    /// Degrees clockwise (0/90/180/270).
    pub fn degrees(self) -> u32 {
        match self {
            Rotation::None => 0,
            Rotation::Cw90 => 90,
            Rotation::Cw180 => 180,
            Rotation::Cw270 => 270,
        }
    }

    /// Build from clockwise degrees, accepting any multiple of 90 (normalized
    /// into `0..360`). Returns `None` for non-multiples of 90.
    pub fn from_degrees(deg: i32) -> Option<Self> {
        match deg.rem_euclid(360) {
            0 => Some(Rotation::None),
            90 => Some(Rotation::Cw90),
            180 => Some(Rotation::Cw180),
            270 => Some(Rotation::Cw270),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_basics() {
        let r = Rect::from_size(1920, 1080);
        assert_eq!(r, Rect::new(0, 0, 1920, 1080));
        assert!(!r.is_empty());
        assert!(Rect::new(0, 0, 0, 100).is_empty());
    }

    #[test]
    fn rotation_axis_swap() {
        assert!(!Rotation::None.swaps_axes());
        assert!(Rotation::Cw90.swaps_axes());
        assert!(!Rotation::Cw180.swaps_axes());
        assert!(Rotation::Cw270.swaps_axes());
    }

    /// `ALL` is what a surface offers, `from_degrees` is what it sends. A
    /// rotation reachable through the degree space and missing from `ALL` would
    /// be an orientation no control could choose, so the two are compared as
    /// sets derived from the quarter turns themselves rather than from a list.
    #[test]
    fn every_quarter_turn_is_offered_and_round_trips() {
        let offered: std::collections::BTreeSet<u32> =
            Rotation::ALL.iter().map(|r| r.degrees()).collect();
        let reachable: std::collections::BTreeSet<u32> = (0..360)
            .step_by(90)
            .filter_map(Rotation::from_degrees)
            .map(|r| r.degrees())
            .collect();
        assert_eq!(offered, reachable);
        for rotation in Rotation::ALL {
            assert_eq!(
                Rotation::from_degrees(rotation.degrees() as i32),
                Some(rotation)
            );
        }
    }

    #[test]
    fn rotation_from_degrees_normalizes() {
        assert_eq!(Rotation::from_degrees(0), Some(Rotation::None));
        assert_eq!(Rotation::from_degrees(90), Some(Rotation::Cw90));
        assert_eq!(Rotation::from_degrees(-90), Some(Rotation::Cw270));
        assert_eq!(Rotation::from_degrees(450), Some(Rotation::Cw90));
        assert_eq!(Rotation::from_degrees(45), None);
    }

    #[test]
    fn rect_contains_point_and_bounds() {
        let r = Rect::new(10, 20, 100, 50);
        assert_eq!(r.right(), 110);
        assert_eq!(r.bottom(), 70);
        assert!(r.contains_point(10, 20));
        assert!(r.contains_point(109, 69));
        assert!(!r.contains_point(110, 69));
        assert!(!r.contains_point(10, 70));
        assert!(!r.contains_point(9, 20));
        assert!(!r.contains_point(10, 19));

        let empty = Rect::new(10, 20, 0, 50);
        assert!(!empty.contains_point(10, 20));
    }

    #[test]
    fn rect_intersection_and_intersects() {
        let r1 = Rect::new(0, 0, 100, 100);
        let r2 = Rect::new(50, 50, 100, 100);
        assert!(r1.intersects(r2));
        assert_eq!(r1.intersection(r2), Some(Rect::new(50, 50, 50, 50)));

        // Touching edges do not intersect
        let r3 = Rect::new(100, 0, 50, 50);
        assert!(!r1.intersects(r3));
        assert_eq!(r1.intersection(r3), None);

        // Disjoint
        let r4 = Rect::new(200, 200, 50, 50);
        assert!(!r1.intersects(r4));
        assert_eq!(r1.intersection(r4), None);

        // Overflow checked
        let r_overflow = Rect::new(u32::MAX - 10, u32::MAX - 10, 100, 100);
        assert_eq!(r_overflow.right(), u32::MAX);
        assert_eq!(r_overflow.right_checked(), None);
    }
}
