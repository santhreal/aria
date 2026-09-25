//! The catalogs' text, shared once instead of copied per frame.
//!
//! A catalog is a data file `aria_models` reads on first use and never changes
//! afterwards, and its rows hold `String`, because the model crate has no idea a
//! UI exists. A panel draws those ids and labels as chips every frame, so reading
//! them straight out of the catalog means one heap copy per chip per frame: the
//! filter list alone is a dozen, and gpui takes a `SharedString` to draw text.
//!
//! These are the same rows with their text shared. Built on first draw, handed
//! out as refcounts after that. A catalog cannot change during a run, so a
//! process-lifetime cache is the whole story: nothing invalidates, and nothing
//! is rebuilt.

use std::sync::LazyLock;

use gpui::SharedString;

/// One catalog row as a panel draws it: what to send, and what to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// The catalog id, which is what a pick sends back to the engine.
    pub id: SharedString,
    /// The display label.
    pub label: SharedString,
}

/// Share every row of a catalog, in the catalog's own order.
fn shared<T>(rows: &[T], id: fn(&T) -> &str, label: fn(&T) -> &str) -> Vec<Choice> {
    rows.iter()
        .map(|row| Choice {
            id: id(row).to_string().into(),
            label: label(row).to_string().into(),
        })
        .collect()
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

/// The transition catalog, for the cut's blend chips and the clip menu.
pub fn transitions() -> &'static [Choice] {
    static ROWS: LazyLock<Vec<Choice>> = LazyLock::new(|| {
        shared(
            aria_models::transition_catalog(),
            |spec| &spec.id,
            |spec| &spec.label,
        )
    });
    &ROWS
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
    use super::Choice;

    /// Every accessor, paired with the ids the model catalog behind it ships.
    ///
    /// The pairing is written out because each catalog has its own spec type and
    /// no two of them can be read through one generic; what is *not* written out
    /// is which accessors exist, which the next rule reads out of this file.
    fn covered() -> Vec<(&'static str, &'static [Choice], Vec<&'static str>)> {
        vec![
            (
                "filters",
                super::filters(),
                aria_models::filter_catalog()
                    .iter()
                    .map(|spec| spec.id.as_str())
                    .collect(),
            ),
            (
                "transitions",
                super::transitions(),
                aria_models::transition_catalog()
                    .iter()
                    .map(|spec| spec.id.as_str())
                    .collect(),
            ),
            (
                "text_effects",
                super::text_effects(),
                aria_models::text_effect_catalog()
                    .iter()
                    .map(|spec| spec.id.as_str())
                    .collect(),
            ),
            (
                "stickers",
                super::stickers(),
                aria_models::sticker_catalog()
                    .iter()
                    .map(|spec| spec.id.as_str())
                    .collect(),
            ),
            (
                "animations",
                super::animations(),
                aria_models::animation_catalog()
                    .iter()
                    .map(|spec| spec.id.as_str())
                    .collect(),
            ),
            (
                "masks",
                super::masks(),
                aria_models::mask_catalog()
                    .iter()
                    .map(|spec| spec.kind.id())
                    .collect(),
            ),
        ]
    }

    /// **A new accessor is covered before it ships.**
    ///
    /// The accessor list is read out of this file rather than kept by hand, so
    /// sharing a seventh catalog is red until it is paired with the catalog it
    /// shares. Without this, the rules below would silently keep testing six.
    #[test]
    fn every_accessor_this_module_offers_is_covered() {
        let source = include_str!("catalog_text.rs");
        let offered: Vec<&str> = source
            .lines()
            .filter_map(|line| {
                let name = line.trim().strip_prefix("pub fn ")?;
                let (name, rest) = name.split_once('(')?;
                rest.contains("-> &'static [Choice]").then_some(name)
            })
            .collect();
        let tested: Vec<&str> = covered().iter().map(|(name, _, _)| *name).collect();
        assert_eq!(
            offered, tested,
            "the accessors this module offers and the ones these rules cover have drifted"
        );
    }

    /// The cache is the catalog, row for row and in order. A cache that dropped
    /// or reordered a row would hide a shipped filter behind a chip that sends
    /// the wrong id.
    #[test]
    fn every_catalog_row_is_offered_in_the_catalogs_own_order() {
        for (name, offered, ids) in covered() {
            assert!(!ids.is_empty(), "the catalog behind {name} is empty");
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
            for choice in offered {
                assert!(!choice.id.is_empty(), "a {name} row has no id");
                assert!(
                    !choice.label.is_empty(),
                    "{name} row {} has no label to draw",
                    choice.id
                );
            }
        }
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
