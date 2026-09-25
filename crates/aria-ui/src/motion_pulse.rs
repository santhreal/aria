//! The pulse and spin math, inlined from `zeron_proto::motion` — the video
//! editor does not depend on that crate. The names are kept so `loaders.rs`
//! ports verbatim.
//!
//! Pure arithmetic: a phase in, a number out. No gpui, no clock. Time belongs
//! to `motion::pulse_delta`, the one caller that owns it.

pub const ZERON_PULSE_MS: u64 = 2_400;
pub const GRADIENT_SPIN_MS: u64 = 750;
pub const ZERON_CELLS: usize = 5;
pub const MATRIX_SIDE: usize = 3;
pub const PULSE_MIN_OPACITY: f32 = 0.08;
pub const PULSE_MIN_SCALE: f32 = 0.9;
pub const PULSE_STAGGER: f32 = 0.15 / 2.4;
pub const GSPIN_ROW_TINTS: [u32; MATRIX_SIDE] = [0xB6D3EF, 0xEDB185, 0xF888A0];
pub const GSPIN_DIM: f32 = 0.1;
pub const MINI_RING: [[usize; 2]; 3] = [[0, 1], [5, 2], [4, 3]];
pub const MINI_RING_LEN: f32 = (MINI_RING.len() * MINI_RING[0].len()) as f32;
pub const MARK_SPREAD: f32 = 0.55;

#[rustfmt::skip]
pub const MARK_CELLS: [(f32, f32); 34] = [
    (0., 600.), (0., 720.), (240., 840.), (240., 720.), (120., 840.), (120., 600.), (240., 600.),
    (0., 480.), (0., 360.), (480., 840.), (480., 720.), (120., 360.), (120., 240.), (240., 360.),
    (600., 720.), (480., 600.), (360., 360.), (240., 240.), (600., 600.), (720., 600.), (720., 480.),
    (240., 120.), (600., 380.), (720., 240.), (720., 0.), (480., 240.), (480., 0.), (120., 480.),
    (240., 480.), (360., 840.), (360., 720.), (360., 600.), (360., 480.), (120., 720.),
];

pub fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

pub fn staggered_phase(raw_delta: f32, index: usize, stagger: f32) -> f32 {
    (raw_delta - index as f32 * stagger).rem_euclid(1.0)
}

/// A phase offset in `[0, 1)` derived from a loader's key.
///
/// Two loaders on screen at once run off the same clock, so without this they
/// pulse in lockstep and read as one animation drawn twice. The key each loader
/// already carries is what tells them apart, and hashing it is what makes the
/// offset stable across frames: the same row must land on the same phase every
/// frame or the loader flickers instead of pulsing.
pub fn key_phase(key: &str) -> f32 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    // The low 16 bits are enough resolution for a phase and keep the divide
    // exact in f32.
    (hasher.finish() & 0xFFFF) as f32 / 65_536.0
}

pub fn pulse_wave(phase: f32) -> f32 {
    0.5 - 0.5 * (phase * std::f32::consts::TAU).cos()
}

pub fn pulse_opacity(phase: f32) -> f32 {
    PULSE_MIN_OPACITY + (1.0 - PULSE_MIN_OPACITY) * pulse_wave(phase)
}

pub fn pulse_scale(phase: f32) -> f32 {
    PULSE_MIN_SCALE + (1.0 - PULSE_MIN_SCALE) * pulse_wave(phase)
}

pub fn gspin_opacity(t: f32, dim: f32) -> f32 {
    let t = t.rem_euclid(1.0);
    if t < 0.45 {
        lerp(1.0, dim, t / 0.45)
    } else if t < 0.92 {
        dim
    } else {
        lerp(dim, 1.0, (t - 0.92) / 0.08)
    }
}

pub fn gspin_cell_phase(row: usize, col: usize) -> f32 {
    let centre = (MATRIX_SIDE as f32 - 1.0) / 2.0;
    let max = MATRIX_SIDE as f32 - 1.0 + centre;
    let d = MATRIX_SIDE as f32 - 1.0 - row as f32 + (col as f32 - centre).abs();
    if max == 0.0 { 0.0 } else { d / (max + 1.0) }
}

pub fn mark_cell_stagger(x: f32, y: f32) -> f32 {
    let t = (820.0 - x + y) / 1660.0;
    (1.0 - t) * MARK_SPREAD
}

pub fn mark_phase(delta: f32, x: f32, y: f32) -> f32 {
    (delta + mark_cell_stagger(x, y)).rem_euclid(1.0)
}

pub fn matrix_wave(raw_delta: f32, wave_index: usize, wave_count: usize) -> f32 {
    let count = wave_count.max(1) as f32;
    pulse_wave(staggered_phase(raw_delta, wave_index, 1.0 / count))
}
