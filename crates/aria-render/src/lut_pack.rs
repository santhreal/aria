//! The first-party LUT starter pack: every filter-preset recipe from
//! [`crate::grade`], baked into a `.cube` table via the shared CPU grade
//! (`ColorGrade::apply` — the same math the shader runs).
//!
//! The pack is generated, not authored: `cargo run -p aria-render --bin
//! lut-pack -- <out-dir>` writes the `.cube` files plus a `luts.json` catalog
//! document for `aria-backend`'s `CATALOG_DIR`. A drift test pins each
//! table's content hash so a recipe change is a conscious catalog re-bake,
//! not silent drift between shipped LUTs and in-app filters.

use aria_compositor::{ColorGrade, CubeLut};
use aria_models::filter_catalog;

use crate::grade::preset_recipe;

/// Grid resolution of baked starter LUTs. 33 is the industry-standard size
/// (Resolve/Adobe default): fine enough that trilinear error on these smooth
/// recipes is far below 8-bit quantization.
pub const STARTER_LUT_SIZE: u32 = 33;

/// The prefix every starter-pack id carries, so a pack id and a filter-catalog
/// id can never be mistaken for each other.
const STARTER_PREFIX: &str = "aria-";

/// The reference colours a look swatch is graded from: three neutrals across the
/// range, then skin, sky, and foliage. Chosen so one strip shows what a recipe
/// does to exposure, contrast, temperature, and saturation at once — a grey ramp
/// alone hides every chroma move a look makes.
pub const SWATCH_REFERENCE: [[u8; 3]; 6] = [
    [26, 26, 28],
    [128, 128, 130],
    [230, 230, 232],
    [217, 166, 133],
    [82, 140, 217],
    [89, 158, 82],
];

/// Rows the swatch strip is graded as. Three rather than one because
/// [`ColorGrade::apply_image`] skips its unsharp pass on a single-row image, and
/// sharpness is part of what a look does.
const SWATCH_ROWS: u32 = 3;

/// One starter-pack row, without its baked table.
///
/// Listing the pack costs a catalog walk; baking it costs a `33³` grade per
/// look. A picker needs the first and one of the second, so the two are separate
/// calls rather than one that always pays for both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterLutEntry {
    /// Catalog id (`aria-<filter id>`), stable once shipped.
    pub id: String,
    /// Display label (the filter preset's label).
    pub label: String,
    /// The filter-catalog id whose recipe this bakes.
    pub filter_id: String,
}

/// One baked starter LUT.
#[derive(Debug)]
pub struct StarterLut {
    /// Catalog id (`aria-<filter id>`), stable once shipped.
    pub id: String,
    /// Display label (the filter preset's label). Owned: the label comes from a
    /// catalog file, not from a compiled table.
    pub label: String,
    /// The baked table (unit domain).
    pub cube: CubeLut,
}

/// Every row the starter pack ships, in catalog order and without baking.
///
/// The list is the filter catalog: a preset added to `filters.toml` is a starter
/// LUT with no edit here, and nothing downstream may hold its own copy of the
/// list.
pub fn starter_lut_entries() -> Vec<StarterLutEntry> {
    filter_catalog()
        .iter()
        .map(|spec| StarterLutEntry {
            id: format!("{STARTER_PREFIX}{}", spec.id),
            label: spec.label.clone(),
            filter_id: spec.id.clone(),
        })
        .collect()
}

/// The row with this pack id, or `None` for an id the catalog does not name.
pub fn starter_lut_entry(id: &str) -> Option<StarterLutEntry> {
    starter_lut_entries().into_iter().find(|row| row.id == id)
}

/// Bake one starter LUT by pack id — what a picker needs when the operator
/// chooses a look, instead of baking the whole pack to use one table.
pub fn bake_starter_lut(id: &str) -> Option<StarterLut> {
    starter_lut_entry(id).map(bake)
}

/// Bake the full starter pack: one LUT per filter-catalog recipe at full
/// intensity (in-app intensity blending happens in the LUT pass itself).
pub fn starter_lut_pack() -> Vec<StarterLut> {
    starter_lut_entries().into_iter().map(bake).collect()
}

/// What one look does to [`SWATCH_REFERENCE`], as one colour per reference
/// entry — the picker's preview, without a GPU pass or a texture upload.
///
/// This is the CPU mirror of the grade shader ([`ColorGrade::apply_image`]), so
/// the chip shows the same exposure, chroma, sharpen, and vignette the clip will
/// get, rather than a hand-picked swatch colour that drifts from the recipe.
pub fn starter_lut_swatch(id: &str) -> Option<Vec<[u8; 3]>> {
    let grade = preset_recipe(&starter_lut_entry(id)?.filter_id)?;
    Some(graded_reference(&grade))
}

fn bake(entry: StarterLutEntry) -> StarterLut {
    let grade = preset_recipe(&entry.filter_id).expect("every catalog id has a recipe");
    StarterLut {
        id: entry.id,
        label: entry.label,
        cube: CubeLut::from_fn(STARTER_LUT_SIZE, |rgb| grade.apply(rgb)),
    }
}

/// Grade the reference strip and read back its middle row.
fn graded_reference(grade: &ColorGrade) -> Vec<[u8; 3]> {
    let width = SWATCH_REFERENCE.len() as u32;
    let stride = (width * 4) as usize;
    let mut rgba = Vec::with_capacity(stride * SWATCH_ROWS as usize);
    for _ in 0..SWATCH_ROWS {
        for colour in SWATCH_REFERENCE {
            rgba.extend_from_slice(&[colour[0], colour[1], colour[2], 255]);
        }
    }
    grade.apply_image(width, SWATCH_ROWS, &mut rgba);
    rgba[stride..stride * 2]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| [px[0], px[1], px[2]])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FNV-1a over the serialized `.cube` text exactly as the `lut-pack`
    /// baker writes it — stable across platforms (the text is
    /// decimal-formatted, not raw float bits).
    fn content_hash(lut: &StarterLut) -> u64 {
        let text = lut.cube.to_cube_string(&lut.label);
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in text.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    #[test]
    fn pack_covers_every_filter_recipe() {
        let pack = starter_lut_pack();
        assert_eq!(pack.len(), filter_catalog().len());
        for lut in &pack {
            assert_eq!(lut.cube.size(), STARTER_LUT_SIZE);
        }
    }

    /// The cheap listing and the expensive bake have to describe the same pack,
    /// or a picker built on the listing offers a look that cannot be baked (or
    /// hides one that ships). Both sides are enumerated; neither is written down.
    #[test]
    fn the_listing_and_the_baked_pack_name_the_same_looks() {
        let listed: std::collections::BTreeSet<String> = starter_lut_entries()
            .into_iter()
            .map(|row| row.id)
            .collect();
        let baked: std::collections::BTreeSet<String> =
            starter_lut_pack().into_iter().map(|lut| lut.id).collect();
        assert_eq!(listed, baked);
        for row in starter_lut_entries() {
            let one = bake_starter_lut(&row.id).expect("a listed look bakes");
            assert_eq!(one.id, row.id);
            assert_eq!(one.label, row.label);
            assert_eq!(one.cube.size(), STARTER_LUT_SIZE);
        }
        assert!(bake_starter_lut("aria-not-a-look").is_none());
    }

    /// A swatch is what the picker shows instead of the look's name alone, so it
    /// has to answer for every shipped look, and wherever the recipe does
    /// something the strip has to show it. A recipe that is neutral is allowed to
    /// grade the reference to itself, so that case is skipped rather than pinned.
    #[test]
    fn every_look_has_a_swatch_that_shows_its_recipe() {
        for row in starter_lut_entries() {
            let swatch = starter_lut_swatch(&row.id).expect("a listed look swatches");
            let neutral = preset_recipe(&row.filter_id)
                .expect("a listed look has a recipe")
                .is_identity();
            assert_eq!(
                swatch.len(),
                SWATCH_REFERENCE.len(),
                "{} swatched {} colours",
                row.id,
                swatch.len()
            );
            if neutral {
                continue;
            }
            assert_ne!(
                swatch.as_slice(),
                SWATCH_REFERENCE.as_slice(),
                "{} graded the reference to itself",
                row.id
            );
        }
        assert!(starter_lut_swatch("aria-not-a-look").is_none());
    }

    #[test]
    fn baked_tables_match_their_recipes() {
        for lut in starter_lut_pack() {
            let filter_id = lut.id.strip_prefix("aria-").unwrap();
            let grade = preset_recipe(filter_id).unwrap();
            for probe in [
                [0.0, 0.0, 0.0],
                [1.0, 1.0, 1.0],
                [0.5, 0.5, 0.5],
                [0.8, 0.2, 0.4],
            ] {
                let want = grade.apply(probe);
                let got = lut.cube.sample(probe);
                for ch in 0..3 {
                    assert!(
                        (want[ch] - got[ch]).abs() < 0.02,
                        "{}: probe {probe:?} channel {ch}: recipe {want:?} vs LUT {got:?}",
                        lut.id
                    );
                }
            }
        }
    }

    /// Drift pin: these hashes change iff a recipe (or the bake) changes.
    /// That's allowed — but re-bake and re-upload the served pack in the
    /// same change, then update the pins here.
    #[test]
    fn starter_pack_is_pinned() {
        let pack = starter_lut_pack();
        let got: Vec<(String, u64)> = pack
            .iter()
            .map(|lut| (lut.id.clone(), content_hash(lut)))
            .collect();
        let pinned: &[(&str, u64)] = &[
            ("aria-vivid", 0x8cfbba8c2c31e91a),
            ("aria-warm", 0x101c937634e3ebf3),
            ("aria-cool", 0x7a678f3ca49392e5),
            ("aria-mono", 0xa92d463fcb1e6d1c),
            ("aria-fade", 0xcb3aab359c00a3a6),
            ("aria-chrome", 0x6e0c0434425fef21),
            ("aria-noir", 0x33f4a8c26c46eaec),
            ("aria-sunset", 0x2818785eecec0848),
            ("aria-forest", 0x3027f87cdf11a137),
            ("aria-berry", 0xd08d9de1d63e4854),
        ];
        assert_eq!(got.len(), pinned.len());
        let dump: Vec<String> = got
            .iter()
            .map(|(id, hash)| format!("(\"{id}\", 0x{hash:016x}),"))
            .collect();
        for ((id, hash), (want_id, want_hash)) in got.iter().zip(pinned) {
            assert_eq!(id, want_id);
            assert_eq!(
                *hash,
                *want_hash,
                "{id} drifted: re-bake the served pack and update the pins to:\n{}",
                dump.join("\n")
            );
        }
    }
}
