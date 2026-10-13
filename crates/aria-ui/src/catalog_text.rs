//! The catalogs' text, shared once instead of copied per frame.
//!
//! A catalog's rows hold `String`, because the model crate has no idea a UI
//! exists. A panel draws those ids and labels as chips every frame, so reading
//! them straight out of the catalog means one heap copy per chip per frame: the
//! filter list alone is a dozen, and gpui takes a `SharedString` to draw text.
//!
//! These are the same rows with their text shared. A catalog read once from a
//! data file is shared for the life of the process. The transition and
//! generator catalogs also list what plugins register, so their shares are
//! rebuilt when the catalog's revision moves and handed out as a refcount until
//! it moves again.

use std::sync::{Arc, LazyLock};

use gpui::SharedString;
use parking_lot::Mutex;

/// One catalog row as a panel draws it: what to send, and what to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// The catalog id, which is what a pick sends back to the engine.
    pub id: SharedString,
    /// The display label.
    pub label: SharedString,
}

/// Share every row of a catalog, in the catalog's own order.
fn shared<'a, T: 'a, C: FromIterator<Choice>>(
    rows: impl IntoIterator<Item = &'a T>,
    id: fn(&T) -> &str,
    label: fn(&T) -> &str,
) -> C {
    rows.into_iter()
        .map(|row| Choice {
            id: SharedString::from(id(row)),
            label: SharedString::from(label(row)),
        })
        .collect()
}

/// The share of a catalog plugins can extend, kept with the revision it was
/// built from.
struct Revisioned(Mutex<Option<(u64, Arc<[Choice]>)>>);

impl Revisioned {
    const fn new() -> Self {
        Self(parking_lot::const_mutex(None))
    }

    /// The share built at `revision`, building it with `build` when the held one
    /// is from another revision or there is none.
    fn at(&self, revision: u64, build: impl FnOnce() -> Arc<[Choice]>) -> Arc<[Choice]> {
        let mut held = self.0.lock();
        if let Some((at, rows)) = held.as_ref()
            && *at == revision
        {
            return Arc::clone(rows);
        }
        let rows = build();
        *held = Some((revision, Arc::clone(&rows)));
        rows
    }
}

/// The filter catalog, for the Look tab's chips.
pub fn filters() -> &'static [Choice] {
    static ROWS: LazyLock<Vec<Choice>> = LazyLock::new(|| {
        shared(
            aria_models::filter_catalog(),
            |spec| &spec.id,
            |spec| &spec.label,
        )
    });
    &ROWS
}

/// The transition catalog, shipped and registered, for the cut's blend chips,
/// the sidebar, and the clip menu.
pub fn transitions() -> Arc<[Choice]> {
    static ROWS: Revisioned = Revisioned::new();
    let catalog = aria_models::transition_catalog();
    ROWS.at(catalog.revision(), || {
        shared(&catalog, |spec| &spec.id, |spec| &spec.label)
    })
}

/// The effects that draw a picture of their own, for the sidebar's Generators
/// grid. Plugins register generators, so the share follows the effect
/// catalog's revision.
pub fn generators() -> Arc<[Choice]> {
    static ROWS: Revisioned = Revisioned::new();
    let catalog = aria_models::effect_catalog();
    ROWS.at(catalog.revision(), || {
        shared(
            catalog.iter().filter(|spec| spec.generator),
            |spec| &spec.id,
            |spec| &spec.label,
        )
    })
}

/// The audio effect catalog, shipped and registered, for an insert chain's Add
/// list. Plugins register audio effects, so the share follows the catalog's
/// revision.
pub fn audio_effects() -> Arc<[Choice]> {
    static ROWS: Revisioned = Revisioned::new();
    let catalog = aria_models::audio_effect_catalog();
    ROWS.at(catalog.revision(), || {
        shared(&catalog, |spec| &spec.id, |spec| &spec.label)
    })
}

/// The text-effect catalog, for the Text tab's chips.
pub fn text_effects() -> &'static [Choice] {
    static ROWS: LazyLock<Vec<Choice>> = LazyLock::new(|| {
        shared(
            aria_models::text_effect_catalog(),
            |spec| &spec.id,
            |spec| &spec.label,
        )
    });
    &ROWS
}

/// The sticker catalog, for the sidebar's Stickers grid.
pub fn stickers() -> &'static [Choice] {
    static ROWS: LazyLock<Vec<Choice>> = LazyLock::new(|| {
        shared(
            aria_models::sticker_catalog(),
            |spec| &spec.id,
            |spec| &spec.label,
        )
    });
    &ROWS
}

/// The animation catalog, for the sidebar's Animations grid.
pub fn animations() -> &'static [Choice] {
    static ROWS: LazyLock<Vec<Choice>> = LazyLock::new(|| {
        shared(
            aria_models::animation_catalog(),
            |spec| &spec.id,
            |spec| &spec.label,
        )
    });
    &ROWS
}

/// The mask catalog, for the sidebar's Masks grid.
///
/// A mask's id belongs to its shape rather than to the row, so the shape hands
/// it over; everything downstream reads the same two fields as every other
/// catalog.
pub fn masks() -> &'static [Choice] {
    static ROWS: LazyLock<Vec<Choice>> = LazyLock::new(|| {
        shared(
            aria_models::mask_catalog(),
            |spec| spec.kind.id(),
            |spec| &spec.label,
        )
    });
    &ROWS
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;
    use std::sync::Arc;

    use super::{Choice, Revisioned};

    /// One accessor's rows, whether shared for the process or per revision.
    type Rows = Box<dyn Deref<Target = [Choice]>>;

    fn rows(rows: impl Deref<Target = [Choice]> + 'static) -> Rows {
        Box::new(rows)
    }

    fn ids<'a, T: 'a>(rows: impl IntoIterator<Item = &'a T>, id: fn(&T) -> &str) -> Vec<String> {
        rows.into_iter().map(|row| id(row).to_string()).collect()
    }

    /// Every accessor, paired with the ids of the model catalog behind it.
    ///
    /// The pairing is written out because each catalog has its own spec type and
    /// no two of them can be read through one generic; what is *not* written out
    /// is which accessors exist, which the next rule reads out of this file.
    fn covered() -> Vec<(&'static str, Rows, Vec<String>)> {
        vec![
            (
                "filters",
                rows(super::filters()),
                ids(aria_models::filter_catalog(), |spec| &spec.id),
            ),
            (
                "transitions",
                rows(super::transitions()),
                ids(&aria_models::transition_catalog(), |spec| &spec.id),
            ),
            (
                "generators",
                rows(super::generators()),
                ids(
                    aria_models::effect_catalog()
                        .iter()
                        .filter(|spec| spec.generator),
                    |spec| &spec.id,
                ),
            ),
            (
                "audio_effects",
                rows(super::audio_effects()),
                ids(&aria_models::audio_effect_catalog(), |spec| &spec.id),
            ),
            (
                "text_effects",
                rows(super::text_effects()),
                ids(aria_models::text_effect_catalog(), |spec| &spec.id),
            ),
            (
                "stickers",
                rows(super::stickers()),
                ids(aria_models::sticker_catalog(), |spec| &spec.id),
            ),
            (
                "animations",
                rows(super::animations()),
                ids(aria_models::animation_catalog(), |spec| &spec.id),
            ),
            (
                "masks",
                rows(super::masks()),
                ids(aria_models::mask_catalog(), |spec| spec.kind.id()),
            ),
        ]
    }

    /// **A new accessor is covered before it ships.**
    ///
    /// The accessor list is read out of this file rather than kept by hand, so
    /// sharing another catalog is red until it is paired with the catalog it
    /// shares. Without this, the rules below would silently keep testing the old set.
    #[test]
    fn every_accessor_this_module_offers_is_covered() {
        let source = include_str!("catalog_text.rs");
        let offered: Vec<&str> = source
            .lines()
            .filter_map(|line| {
                let name = line.trim().strip_prefix("pub fn ")?;
                let (name, rest) = name.split_once('(')?;
                (rest.contains("-> &'static [Choice]") || rest.contains("-> Arc<[Choice]>"))
                    .then_some(name)
            })
            .collect();
        let tested: Vec<&str> = covered().iter().map(|(name, _, _)| *name).collect();
        assert_eq!(
            offered, tested,
            "the accessors this module offers and the ones these rules cover have drifted"
        );
    }

    /// Accessors whose catalog holds only what plugins register. No shipped
    /// generator exists and these tests register none, so the share is empty
    /// here; `tests/registered_catalog.rs` checks it with a generator in it.
    const REGISTERED_ONLY: &[&str] = &["generators"];

    /// The cache is the catalog, row for row and in order. A cache that dropped
    /// or reordered a row would hide a shipped filter behind a chip that sends
    /// the wrong id.
    #[test]
    fn every_catalog_row_is_offered_in_the_catalogs_own_order() {
        for (name, offered, ids) in covered() {
            assert!(
                REGISTERED_ONLY.contains(&name) || !ids.is_empty(),
                "the catalog behind {name} is empty"
            );
            assert_eq!(
                offered
                    .iter()
                    .map(|choice| choice.id.as_ref())
                    .collect::<Vec<&str>>(),
                ids,
                "{name} does not offer its catalog row for row"
            );
        }
    }

    /// Asked twice, the same allocation: this is the whole reason the module
    /// exists, and a cache that rebuilt would be the defect it replaced.
    #[test]
    fn asking_twice_hands_back_the_same_text() {
        for (name, offered, _) in covered() {
            let again = covered()
                .into_iter()
                .find(|(again, _, _)| *again == name)
                .map(|(_, rows, _)| rows)
                .expect("the same accessor answers twice");
            assert!(
                std::ptr::eq(offered.as_ptr(), again.as_ptr()),
                "{name} was rebuilt, so every frame pays for it again"
            );
        }
    }

    /// Every row carries a label, because a chip with no text is a chip
    /// nobody can read: an empty label in the catalog file is a data defect
    /// this makes visible rather than drawing a blank.
    #[test]
    fn every_offered_row_carries_both_halves() {
        for (name, offered, _) in covered() {
            for choice in offered.iter() {
                assert!(!choice.id.is_empty(), "a {name} row has no id");
                assert!(
                    !choice.label.is_empty(),
                    "{name} row {} has no label to draw",
                    choice.id
                );
            }
        }
    }

    /// **A registered transition reaches the chips.**
    ///
    /// A plugin registering or dropping a transition moves the catalog's
    /// revision. The share follows the revision it was built from: at one
    /// revision it is built once, a new revision rebuilds it, and a revision
    /// coming back is rebuilt rather than served the rows of another.
    #[test]
    fn a_revisioned_share_is_built_once_per_revision() {
        let share = Revisioned::new();
        let row = |id: &'static str| -> Arc<[Choice]> {
            Arc::from([Choice {
                id: id.into(),
                label: id.into(),
            }])
        };
        let mut builds = 0;
        let first = share.at(1, || {
            builds += 1;
            row("first")
        });
        let again = share.at(1, || {
            builds += 1;
            row("again")
        });
        assert!(Arc::ptr_eq(&first, &again), "one revision built twice");
        assert_eq!(builds, 1);

        let moved = share.at(2, || {
            builds += 1;
            row("moved")
        });
        assert_eq!(moved[0].id, "moved", "a new revision served the old rows");
        let back = share.at(1, || {
            builds += 1;
            row("back")
        });
        assert_eq!(back[0].id, "back", "revision 1 served revision 2's rows");
        assert_eq!(builds, 3);
    }

    /// **A mask row names a shape.**
    ///
    /// The inspector recovers the shape from the row's id, so a row whose id no
    /// shape answers to would vanish from the panel instead of drawing. The
    /// catalog and the shapes are one contract in two files.
    #[test]
    fn every_mask_row_names_a_shape() {
        for choice in super::masks() {
            assert!(
                aria_models::MaskKind::from_id(choice.id.as_str()).is_some(),
                "the mask catalog offers {}, which names no shape, so the inspector \
                 would draw one button fewer than the catalog ships",
                choice.id
            );
        }
    }
}
