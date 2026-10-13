//! Second-order filters: the RBJ cookbook designs, run in transposed direct
//! form II at double precision so a 20 Hz band stays accurate at 48 kHz.

use std::f64::consts::PI;

/// The quality of a second-order Butterworth response: maximally flat.
pub(super) const BUTTERWORTH: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Normalized biquad coefficients.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Coefs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

/// `cos(w0)` and `alpha` for a design at `freq` Hz with quality `q`. The
/// frequency is held inside (1 Hz, 0.49 of the rate) and the quality above
/// 0.05, so an out-of-range value bends the curve instead of breaking it.
fn prepare(rate: u32, freq: f32, q: f32) -> (f64, f64) {
    let rate = f64::from(rate.max(1));
    let freq = f64::from(freq).clamp(1.0, rate * 0.49);
    let q = f64::from(q).max(0.05);
    let w0 = 2.0 * PI * freq / rate;
    (w0.cos(), w0.sin() / (2.0 * q))
}

impl Coefs {
    fn normalized(b: [f64; 3], a: [f64; 3]) -> Self {
        Self {
            b0: b[0] / a[0],
            b1: b[1] / a[0],
            b2: b[2] / a[0],
            a1: a[1] / a[0],
            a2: a[2] / a[0],
        }
    }

    pub(super) fn peaking(rate: u32, freq: f32, q: f32, gain_db: f32) -> Self {
        let (cos, alpha) = prepare(rate, freq, q);
        let a = 10f64.powf(f64::from(gain_db) / 40.0);
        Self::normalized(
            [1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a],
            [1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a],
        )
    }

    pub(super) fn low_shelf(rate: u32, freq: f32, q: f32, gain_db: f32) -> Self {
        let (cos, alpha) = prepare(rate, freq, q);
        let a = 10f64.powf(f64::from(gain_db) / 40.0);
        let root = 2.0 * a.sqrt() * alpha;
        Self::normalized(
            [
                a * ((a + 1.0) - (a - 1.0) * cos + root),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - root),
            ],
            [
                (a + 1.0) + (a - 1.0) * cos + root,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - root,
            ],
        )
    }

    pub(super) fn high_shelf(rate: u32, freq: f32, q: f32, gain_db: f32) -> Self {
        let (cos, alpha) = prepare(rate, freq, q);
        let a = 10f64.powf(f64::from(gain_db) / 40.0);
        let root = 2.0 * a.sqrt() * alpha;
        Self::normalized(
            [
                a * ((a + 1.0) + (a - 1.0) * cos + root),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - root),
            ],
            [
                (a + 1.0) - (a - 1.0) * cos + root,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - root,
            ],
        )
    }

    pub(super) fn high_pass(rate: u32, freq: f32, q: f32) -> Self {
        let (cos, alpha) = prepare(rate, freq, q);
        Self::normalized(
            [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0],
            [1.0 + alpha, -2.0 * cos, 1.0 - alpha],
        )
    }

    pub(super) fn low_pass(rate: u32, freq: f32, q: f32) -> Self {
        let (cos, alpha) = prepare(rate, freq, q);
        Self::normalized(
            [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0],
            [1.0 + alpha, -2.0 * cos, 1.0 - alpha],
        )
    }
}

/// One filter's memory.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Biquad {
    z1: f64,
    z2: f64,
}

impl Biquad {
    #[inline]
    pub(super) fn run(&mut self, c: &Coefs, x: f64) -> f64 {
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        if self.z1.abs() < 1e-30 {
            self.z1 = 0.0;
        }
        if self.z2.abs() < 1e-30 {
            self.z2 = 0.0;
        }
        y
    }
}
