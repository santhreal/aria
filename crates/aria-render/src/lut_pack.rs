//! The first-party LUT starter pack: every filter-preset recipe from
//! [`crate::grade`], baked into a `.cube` table via the shared CPU grade
//! (`ColorGrade::apply` — the same math the shader runs).
//!
//! The pack is generated, not authored: `cargo run -p aria-render --bin
//! lut-pack -- <out-dir>` writes the `.cube` files plus a `luts.json` catalog
//! document for `aria-backend`'s `CATALOG_DIR`. A drift test pins each
//! table's content hash so a recipe change is a conscious catalog re-bake,
//! not silent drift between shipped LUTs and in-app filters.

use aria_compositor::CubeLut;
use aria_models::filter_catalog;

use crate::grade::preset_recipe;

/// Grid resolution of baked starter LUTs. 33 is the industry-standard size
/// (Resolve/Adobe default): fine enough that trilinear error on these smooth
/// recipes is far below 8-bit quantization.
pub const STARTER_LUT_SIZE: u32 = 33;

/// The prefix every starter-pack id carries, so a pack id and a filter-catalog
/// id can never be mistaken for each other.
const STARTER_PREFIX: &str = "aria-";

/// One starter-pack row, without its baked table.
///
/// Listing the pack costs a catalog walk; baking it costs a `33³` grade per
/// look, so a caller that needs the ids and labels lists without baking.
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

/// Bake the full starter pack: one LUT per filter-catalog recipe at full
/// intensity (in-app intensity blending happens in the LUT pass itself).
pub fn starter_lut_pack() -> Vec<StarterLut> {
    starter_lut_entries().into_iter().map(bake).collect()
}

fn bake(entry: StarterLutEntry) -> StarterLut {
    let grade = preset_recipe(&entry.filter_id).expect("every catalog id has a recipe");
    StarterLut {
        id: entry.id,
        label: entry.label,
        cube: CubeLut::from_fn(STARTER_LUT_SIZE, |rgb| grade.apply(rgb)),
    }
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
    /// or a reader of the listing names a look the pack does not write (or
    /// misses one it writes). Both sides are enumerated; neither is written down.
    #[test]
    fn the_listing_and_the_baked_pack_name_the_same_looks() {
        let listed: Vec<(String, String)> = starter_lut_entries()
            .into_iter()
            .map(|row| (row.id, row.label))
            .collect();
        let baked: Vec<(String, String)> = starter_lut_pack()
            .into_iter()
            .map(|lut| (lut.id, lut.label))
            .collect();
        assert_eq!(listed, baked);
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
