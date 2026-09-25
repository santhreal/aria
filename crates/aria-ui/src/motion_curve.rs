//! Cubic beziers: the six curves the app eases with, the one that overshoots,
//! and the evaluator they share.
//!
//! Split out of `motion.rs` as a pure move — the specs that spend these curves
//! stay there, because the catalog reads that file to find them.

/// How far past its endpoints a curve may travel.
///
/// [`CubicBezier::eval`] clamps to this, so an overshoot is bounded by
/// construction rather than by whoever chose the control points. A curve whose
/// handles stay inside the unit square never reaches the bound at all; it
/// exists for [`SPRING`], which peaks a tenth of the way past its target.
pub const OVERSHOOT: f32 = 0.15;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubicBezier {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl CubicBezier {
    /// Reachable from `motion` and no further: see [`CURVES`]. Four numbers are
    /// not reviewable, so the only curves a panel can spend are the named ones
    /// below.
    pub(super) const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    fn coefficients(a: f32, b: f32) -> (f32, f32, f32) {
        let c = 3.0 * a;
        let bb = 3.0 * (b - a) - c;
        let aa = 1.0 - c - bb;
        (aa, bb, c)
    }

    fn sample_x(&self, t: f32) -> f32 {
        let (a, b, c) = Self::coefficients(self.x1, self.x2);
        ((a * t + b) * t + c) * t
    }

    fn sample_y(&self, t: f32) -> f32 {
        let (a, b, c) = Self::coefficients(self.y1, self.y2);
        ((a * t + b) * t + c) * t
    }

    fn sample_x_derivative(&self, t: f32) -> f32 {
        let (a, b, c) = Self::coefficients(self.x1, self.x2);
        (3.0 * a * t + 2.0 * b) * t + c
    }

    fn solve_t_for_x(&self, x: f32) -> f32 {
        let mut t = x;
        for _ in 0..8 {
            let err = self.sample_x(t) - x;
            if err.abs() < 1e-6 {
                return t;
            }
            let d = self.sample_x_derivative(t);
            if d.abs() < 1e-6 {
                break;
            }
            t -= err / d;
        }
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..32 {
            let mid = (lo + hi) / 2.0;
            if self.sample_x(mid) < x {
                lo = mid
            } else {
                hi = mid
            }
        }
        (lo + hi) / 2.0
    }

    /// The value at `x`, inside the bound this curve's own handles earn.
    ///
    /// Solving for `t` is iterative, so `sample_y` carries a few bits of noise:
    /// a curve that tops out at 1.0 mathematically can compute 1.0000001. The
    /// value goes straight into an opacity, a scale or a colour mix, so the
    /// bound is applied here rather than at every call site, and it is the
    /// bound [`Self::bounds`] reads off the control points — a curve that
    /// cannot overshoot cannot leave `0..=1`.
    pub fn eval(&self, x: f32) -> f32 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        let (floor, ceiling) = self.bounds();
        self.sample_y(self.solve_t_for_x(x)).clamp(floor, ceiling)
    }

    /// The range [`Self::eval`] can return: the unit interval, widened by
    /// [`OVERSHOOT`] only for a curve whose handles leave the unit square.
    ///
    /// One owner, so a test asserts the bound rather than re-deriving it.
    pub const fn bounds(&self) -> (f32, f32) {
        if self.overshoots() {
            (-OVERSHOOT, 1.0 + OVERSHOOT)
        } else {
            (0.0, 1.0)
        }
    }

    /// Whether this curve leaves the unit square on its way, read off the
    /// control points rather than declared beside them.
    ///
    /// A bezier stays inside the hull of its control points, so a curve whose
    /// handles are both inside `0..=1` cannot overshoot and one whose handle
    /// passes 1.0 must. Every test that asks whether a curve is monotone asks
    /// this first, which is why it is derived: a curve added with a handle past
    /// 1.0 is held to the overshoot rules without anyone marking it.
    pub const fn overshoots(&self) -> bool {
        self.y1 > 1.0 || self.y2 > 1.0 || self.y1 < 0.0 || self.y2 < 0.0
    }

    pub fn easing(self) -> impl Fn(f32) -> f32 + 'static {
        move |x| self.eval(x)
    }
}

/// Arrival. Nearly all the distance in the first third, so a thing that appears
/// has already stopped by the time the eye reaches it.
pub const EASE_OUT_EXPO: CubicBezier = CubicBezier::new(0.16, 1.0, 0.3, 1.0);
/// Deceleration without the overshoot: geometry coming to rest.
pub const EASE_OUT: CubicBezier = CubicBezier::new(0.0, 0.0, 0.58, 1.0);
/// The general-purpose curve: a slight ease in, a long ease out.
pub const EASE: CubicBezier = CubicBezier::new(0.25, 0.1, 0.25, 1.0);
/// A list reordering itself. Steeper than [`EASE`] at the start so the rows
/// commit to the new order immediately, then settle.
pub const EASE_RESORT: CubicBezier = CubicBezier::new(0.22, 1.0, 0.36, 1.0);
/// Symmetric, for a value that travels a long way and has to leave as smoothly
/// as it arrived: a scroll the editor performed.
pub const EASE_IN_OUT: CubicBezier = CubicBezier::new(0.42, 0.0, 0.58, 1.0);
/// The platform-standard curve, used where the app should feel like every other
/// application rather than like itself: a hover, a caret.
pub const EASE_STANDARD: CubicBezier = CubicBezier::new(0.4, 0.0, 0.2, 1.0);

/// Momentum coming to rest: the only curve that passes its target and comes
/// back. Peaks at 1.098 — under a tenth of the distance past the end — and is
/// within half a percent of resting by 94% of its span, so the overshoot reads
/// as weight rather than as a bounce.
pub const SPRING: CubicBezier = CubicBezier::new(0.34, 1.56, 0.64, 1.0);

/// Every easing the app is allowed to use.
///
/// `CubicBezier::new` is private to this module, so a panel that wants a
/// different feel adds a named curve here and says what it is for, rather than
/// typing four numbers into a call nobody will read again. The table is what
/// the gallery draws and what the tests enumerate, so a curve that is added
/// and not listed has no specimen and no coverage.
pub const CURVES: &[(&str, CubicBezier)] = &[
    ("EASE_OUT_EXPO", EASE_OUT_EXPO),
    ("EASE_OUT", EASE_OUT),
    ("EASE", EASE),
    ("EASE_RESORT", EASE_RESORT),
    ("EASE_IN_OUT", EASE_IN_OUT),
    ("EASE_STANDARD", EASE_STANDARD),
    ("SPRING", SPRING),
];
