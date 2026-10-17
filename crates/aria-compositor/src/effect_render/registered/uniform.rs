//! The storage buffer words a registered pass reads as `aria`.

use crate::registered_passes::{EFFECT_HEADER_WORDS, TRANSITION_WORDS, param_vec4s};

/// The storage buffer payload of one effect pass, in 32-bit words.
pub(super) fn effect_uniform(
    width: u32,
    height: u32,
    time: f32,
    pass_index: usize,
    pass_count: usize,
    params: &[f32],
) -> Vec<u32> {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let used = EFFECT_HEADER_WORDS + 4 * param_vec4s(params.len());
    let mut words = vec![0u32; used];
    words[..EFFECT_HEADER_WORDS].copy_from_slice(&[
        w.to_bits(),
        h.to_bits(),
        (1.0 / w).to_bits(),
        (1.0 / h).to_bits(),
        time.to_bits(),
        pass_index as u32,
        pass_count as u32,
        0,
    ]);
    for (slot, word) in words[EFFECT_HEADER_WORDS..used].iter_mut().enumerate() {
        *word = params.get(slot).map_or(0, |value| value.to_bits());
    }
    words
}

pub(super) fn transition_uniform(
    width: u32,
    height: u32,
    progress: f32,
    params: &[f32],
) -> Vec<u32> {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let used = TRANSITION_WORDS + 4 * param_vec4s(params.len());
    let mut words = vec![0u32; used];
    words[..TRANSITION_WORDS].copy_from_slice(&[
        w.to_bits(),
        h.to_bits(),
        (1.0 / w).to_bits(),
        (1.0 / h).to_bits(),
        progress.to_bits(),
        0,
        0,
        0,
    ]);
    for (slot, word) in words[TRANSITION_WORDS..used].iter_mut().enumerate() {
        *word = params.get(slot).map_or(0, |value| value.to_bits());
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY: the WGSL `AriaPass` layout is size, texel, time, pass index, pass
    /// count, padding, then `params` at byte 32 in whole `vec4`s. A word in the
    /// wrong place feeds every registered effect a wrong value; this pins each
    /// word and the zero fill of a partial `vec4`. It does not check the WGSL
    /// side, which the GPU test reads back.
    #[test]
    fn effect_uniform_words_follow_the_prelude_layout() {
        let words = effect_uniform(200, 100, 1.5, 1, 3, &[0.25, 0.5, 0.75, 1.0, 2.0]);
        assert_eq!(
            words.len(),
            EFFECT_HEADER_WORDS + 8,
            "five floats take two vec4s"
        );
        let floats = |range: std::ops::Range<usize>| {
            words[range]
                .iter()
                .map(|word| f32::from_bits(*word))
                .collect::<Vec<_>>()
        };
        assert_eq!(floats(0..5), [200.0, 100.0, 0.005, 0.01, 1.5]);
        assert_eq!(&words[5..8], &[1, 3, 0]);
        assert_eq!(
            floats(8..16),
            [0.25, 0.5, 0.75, 1.0, 2.0, 0.0, 0.0, 0.0],
            "a partial vec4 is zero-filled, not left with stale words"
        );
    }

    #[test]
    fn an_effect_without_parameters_still_uploads_one_vec4() {
        let words = effect_uniform(0, 0, 0.0, 0, 1, &[]);
        assert_eq!(words.len(), EFFECT_HEADER_WORDS + 4);
        assert_eq!(&words[EFFECT_HEADER_WORDS..], &[0; 4]);
        // A zero-sized target reads as one pixel rather than dividing by zero.
        assert_eq!(f32::from_bits(words[2]), 1.0);
    }

    #[test]
    fn an_effect_past_the_old_uniform_cap_packs_all_parameters() {
        let params = vec![1.0; 300];
        let words = effect_uniform(1, 1, 0.0, 0, 1, &params);
        assert_eq!(words.len(), EFFECT_HEADER_WORDS + 4 * param_vec4s(300));
    }

    #[test]
    fn transition_uniform_carries_size_texel_progress_and_params() {
        let words = transition_uniform(50, 25, 0.4, &[0.75]);
        let floats: Vec<f32> = words[..5].iter().map(|w| f32::from_bits(*w)).collect();
        assert_eq!(floats, [50.0, 25.0, 0.02, 0.04, 0.4]);
        assert_eq!(&words[5..8], &[0, 0, 0]);
        assert_eq!(f32::from_bits(words[8]), 0.75);
    }
}
