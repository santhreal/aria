//! What a shader reads out of its `params` uniform.
//!
//! One owner for the WGSL side of the parameter contract. The effect registry
//! declares no slots of its own — they come from the model catalog, which a user
//! can replace on disk — so the only way to know whether a declared slot is real
//! is to ask the shader that would read it. That parse happens here, once per
//! process, and [`super::passes`] refuses an effect whose derived slots and
//! shader reads disagree rather than feeding a pass whatever the previous one
//! left in the uniform.
//!
//! Nothing here knows an effect id or a file name: it takes shader source and
//! returns what that source reads.

use std::collections::BTreeSet;

/// Slot and component of one `params` read: `params[1].z` is `(1, 2)`.
pub(crate) type Read = (usize, usize);

/// Every `(slot, component)` of the `params` uniform that `source` reads.
///
/// Comments are stripped first: most of these shaders document their slots in
/// prose ("radius in params[0].x"), and prose is not a read.
pub(crate) fn param_reads(source: &str) -> BTreeSet<Read> {
    let mut reads = BTreeSet::new();
    for line in source.lines() {
        let code = line.split_once("//").map_or(line, |(before, _)| before);
        let mut offset = 0;
        while let Some(at) = code[offset..].find("params") {
            let start = offset + at;
            offset = start + "params".len();
            // `chroma_params` is a different uniform, not slot 0 of this one.
            if code[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                continue;
            }
            let after = &code[offset..];
            match after.chars().next() {
                // `uniforms.params[N]`: an array of vec4 slots.
                Some('[') => {
                    let Some(close) = after.find(']') else {
                        continue;
                    };
                    let Ok(slot) = after[1..close].trim().parse::<usize>() else {
                        continue;
                    };
                    insert_components(&mut reads, slot, &after[close + 1..]);
                }
                // `u.params.x`: a single vec4, so slot 0.
                Some('.') => insert_components(&mut reads, 0, after),
                // A declaration (`params: array<...>`) reads nothing.
                _ => {}
            }
        }
    }
    reads
}

/// Record what a read at `slot` takes. A swizzle names its components; a read
/// with no swizzle takes the whole vec4.
fn insert_components(reads: &mut BTreeSet<Read>, slot: usize, after: &str) {
    let swizzle: String = after
        .strip_prefix('.')
        .map(|rest| {
            rest.chars()
                .take_while(|c| "xyzwrgba".contains(*c))
                .collect()
        })
        .unwrap_or_default();
    if swizzle.is_empty() {
        reads.extend((0..4).map(|component| (slot, component)));
        return;
    }
    for component in swizzle.chars() {
        reads.insert((
            slot,
            match component {
                'x' | 'r' => 0,
                'y' | 'g' => 1,
                'z' | 'b' => 2,
                _ => 3,
            },
        ));
    }
}

/// The `N` of the shader's `params: array<vec4<f32>, N>` uniform.
pub(crate) fn declared_param_slots(source: &str) -> Option<usize> {
    let after = source.split_once("params: array<vec4<f32>,")?.1;
    let digits: String = after
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The flat floats `shaders` between them read, as `(slot, component)` pairs.
pub(crate) fn reads_of(shaders: &[&'static str]) -> BTreeSet<Read> {
    shaders
        .iter()
        .flat_map(|source| param_reads(source))
        .collect()
}

/// The `(slot, component)` pairs a run of `floats` flattened parameters covers.
pub(crate) fn slots_for(floats: usize) -> BTreeSet<Read> {
    (0..floats).map(|flat| (flat / 4, flat % 4)).collect()
}
