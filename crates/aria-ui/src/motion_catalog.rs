//! The motion catalog as a table: every spec, what it is for, where it is spent.
//!
//! The specs themselves stay plain consts in [`super`] — a caller writes
//! `motion::FADE_IN`, not a lookup. This table is what makes the catalog
//! reviewable: a spec that nothing spends is dead timing, a spec whose duration
//! moved is the whole app retimed, and a spec that no document mentions is a
//! number nobody can look up. The tests below hold all three, and the row is the
//! one place a new spec has to be declared.

use super::*;

/// One spec, and the claims made about it.
pub struct SpecRow {
    /// The const in `motion.rs`, exactly as it is written there.
    pub name: &'static str,
    pub spec: MotionSpec,
    /// The timing this row promises. Kept beside the spec rather than read out
    /// of it: that is what makes a retime a failing test instead of a silent
    /// change to every transition in the app. Both are steps on the duration
    /// scale, which the tests below also hold.
    pub delay_ms: u64,
    pub duration_ms: u64,
    /// What the timing is for, in terms of what the operator sees.
    pub purpose: &'static str,
    /// The file that spends it, relative to the workspace root.
    pub file: &'static str,
    /// The item in that file which spends it.
    pub symbol: &'static str,
    /// The curve it rides, by the name it has in [`CURVES`]. Two specs with the
    /// same duration and different curves are not the same motion, so the feel
    /// is pinned here the way the timing is.
    pub curve: &'static str,
}

/// Every spec in the catalog. Order is the order the document lists them.
pub const CATALOG: &[SpecRow] = &[
    SpecRow {
        name: "FADE_IN",
        spec: FADE_IN,
        curve: "EASE_OUT_EXPO",
        delay_ms: 0,
        duration_ms: 500,
        purpose: "An element arriving for the first time: a launch card, a panel body.",
        file: "crates/aria-ui/src/motion.rs",
        symbol: "fn fade_in",
    },
    SpecRow {
        name: "FADE_QUICK",
        spec: FADE_QUICK,
        curve: "EASE",
        delay_ms: 0,
        duration_ms: 160,
        purpose: "A small change that must not read as a jump: a seek badge, a drop hint.",
        file: "crates/aria-ui/src/motion.rs",
        symbol: "fn fade_quick",
    },
    SpecRow {
        name: "MENU_IN",
        spec: MENU_IN,
        curve: "EASE",
        delay_ms: 0,
        duration_ms: 160,
        purpose: "A menu or popover opening under the pointer.",
        file: "crates/aria-ui/src/motion.rs",
        symbol: "fn menu_in",
    },
    SpecRow {
        name: "MENU_OUT",
        spec: MENU_OUT,
        curve: "EASE",
        delay_ms: 0,
        duration_ms: 120,
        purpose: "Anything leaving: a menu, a dialog, a context menu, the rename bar. \
                  Also the window an overlay keeps drawing after its condition went false.",
        file: "crates/aria-ui/src/plugins/overlay_host.rs",
        symbol: "fn exit_window",
    },
    SpecRow {
        name: "DIALOG_IN",
        spec: DIALOG_IN,
        curve: "EASE",
        delay_ms: 0,
        duration_ms: 200,
        purpose: "A dialog surface arriving: slower than a menu because it takes the \
                  keyboard. Also a whole route arriving, which is the same size of \
                  change to the window.",
        file: "crates/aria-ui/src/motion.rs",
        symbol: "fn dialog_in",
    },
    SpecRow {
        name: "SPLASH_OUT",
        spec: SPLASH_OUT,
        curve: "EASE",
        delay_ms: 160,
        duration_ms: 500,
        purpose: "The boot mark leaving. The delay is the beat that keeps a fast start \
                  from flashing.",
        file: "crates/aria-ui/src/loaders.rs",
        symbol: "SPLASH_OUT",
    },
    SpecRow {
        name: "OPEN",
        spec: OPEN,
        curve: "EASE_OUT_EXPO",
        delay_ms: 0,
        duration_ms: 320,
        purpose: "One stage of the editor opening: the bar, then the rails, then the timeline. \
                  Staged by OPEN_STAGE so the layout resolves in reading order \
                  instead of all at once.",
        file: "crates/aria-ui/src/shell/mod.rs",
        symbol: "fn opening",
    },
    SpecRow {
        name: "RESIZE",
        spec: RESIZE,
        curve: "EASE_OUT",
        delay_ms: 0,
        duration_ms: 200,
        purpose: "Geometry settling after the hand let go: a pane width, a released \
                  splitter, a clip that was rippled, and the horizontal scale when the \
                  editor rather than the pointer chose it.",
        file: "crates/aria-ui/src/motion_glide.rs",
        symbol: "PANE",
    },
    SpecRow {
        name: "TAB_SLIDE",
        spec: TAB_SLIDE,
        curve: "EASE_OUT",
        delay_ms: 0,
        duration_ms: 160,
        purpose: "The inspector's tab indicator travelling to the tab that was clicked.",
        file: "crates/aria-ui/src/panels/inspector/tab_strip.rs",
        symbol: "TAB_SLIDE",
    },
    SpecRow {
        name: "COLLAPSE",
        spec: COLLAPSE,
        curve: "EASE_OUT",
        delay_ms: 0,
        duration_ms: 200,
        purpose: "A lane height opening or closing.",
        file: "crates/aria-ui/src/motion_glide.rs",
        symbol: "LANE",
    },
    SpecRow {
        name: "CHEVRON",
        spec: CHEVRON,
        curve: "EASE",
        delay_ms: 0,
        duration_ms: 200,
        purpose: "A disclosure arrow rotating, so the arrow and the section agree.",
        file: "crates/aria-ui/src/panels/inspector/blocks/widgets.rs",
        symbol: "CHEVRON",
    },
    SpecRow {
        name: "SCROLL_GLIDE",
        spec: SCROLL_GLIDE,
        curve: "EASE_IN_OUT",
        delay_ms: 0,
        duration_ms: 500,
        purpose: "A view the editor moved: a fit, or playback the timeline had to follow. \
                  Long, because the whole picture travels and a jump that size reads as a cut.",
        file: "crates/aria-ui/src/motion_glide.rs",
        symbol: "SCROLL",
    },
    SpecRow {
        name: "FLING",
        spec: FLING,
        curve: "SPRING",
        delay_ms: 0,
        duration_ms: 500,
        purpose: "The timeline offset after a pan is released, carrying the momentum of \
                  the release. The one motion in the app that passes its target and \
                  comes back, which is what makes a flick feel like weight.",
        file: "crates/aria-ui/src/motion_glide.rs",
        symbol: "FLING",
    },
    SpecRow {
        name: "HOVER_FADE",
        spec: HOVER_FADE,
        curve: "EASE_STANDARD",
        delay_ms: 0,
        duration_ms: 160,
        purpose: "Every hover blend in the app, over one store so a strip fades at one \
                  rate; and, at the same beat, the binary states a clip card can leave \
                  again — selected and locked.",
        file: "crates/aria-ui/src/motion.rs",
        symbol: "fn duration",
    },
    SpecRow {
        name: "ZERON_PULSE",
        spec: ZERON_PULSE,
        curve: "EASE",
        delay_ms: 0,
        duration_ms: 2400,
        purpose: "The loader's breathing cycle while work is in flight.",
        file: "crates/aria-ui/src/loaders.rs",
        symbol: "ZERON_PULSE",
    },
    SpecRow {
        name: "GRADIENT_SPIN",
        spec: GRADIENT_SPIN,
        curve: "EASE",
        delay_ms: 0,
        duration_ms: 750,
        purpose: "The loader's gradient sweep, faster than the pulse it runs inside.",
        file: "crates/aria-ui/src/loaders.rs",
        symbol: "GRADIENT_SPIN",
    },
    SpecRow {
        name: "CARET_BLINK",
        spec: CARET_BLINK,
        curve: "EASE_STANDARD",
        delay_ms: 0,
        duration_ms: 1000,
        purpose: "The caret in a rename field, marking time for whoever is typing.",
        file: "crates/aria-ui/src/panels/rename_bar.rs",
        symbol: "CARET_BLINK",
    },
];

/// The document that has to name every row.
pub const DOC: &str = "docs/animation-catalog.md";

#[cfg(test)]
mod tests {
    use super::*;

    /// The workspace root, from this crate's manifest. Two levels up:
    /// `<root>/crates/aria-ui`.
    fn workspace_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
    }

    /// Every `pub const` of type `MotionSpec` declared in `motion.rs`, read from
    /// the source at build time so a spec added without a row here fails rather
    /// than going undocumented.
    fn declared_specs() -> Vec<&'static str> {
        include_str!("motion.rs")
            .lines()
            .filter_map(|line| {
                let rest = line.trim().strip_prefix("pub const ")?;
                let (name, tail) = rest.split_once(':')?;
                tail.trim_start()
                    .starts_with("MotionSpec")
                    .then_some(name.trim())
            })
            .collect()
    }

    #[test]
    fn every_declared_spec_has_a_catalog_row_and_every_row_is_declared() {
        let declared = declared_specs();
        assert!(
            declared.len() >= 14,
            "only {} specs parsed out of motion.rs; the scan is broken, not the catalog",
            declared.len()
        );
        for name in &declared {
            assert!(
                CATALOG.iter().any(|row| row.name == *name),
                "{name} is a spec with no catalog row: undocumented timing"
            );
        }
        for row in CATALOG {
            assert!(
                declared.contains(&row.name),
                "{} is a catalog row for a spec that no longer exists",
                row.name
            );
        }
    }

    /// A catalog edit that retimes a spec has to say so here. Without this a
    /// one-character change to a duration retimes the whole app silently.
    #[test]
    fn every_row_pins_the_timing_it_promises() {
        for row in CATALOG {
            assert_eq!(
                row.spec.delay_ms, row.delay_ms,
                "{} delay moved; update the row or revert the spec",
                row.name
            );
            assert_eq!(
                row.spec.duration_ms, row.duration_ms,
                "{} duration moved; update the row or revert the spec",
                row.name
            );
            assert_eq!(
                row.spec.total(),
                std::time::Duration::from_millis(row.delay_ms + row.duration_ms),
                "{} total does not add up",
                row.name
            );
            assert!(
                on_scale(row.duration_ms),
                "{} promises {} ms, which is not a step on the scale",
                row.name,
                row.duration_ms
            );
            assert!(
                row.delay_ms == 0 || on_scale(row.delay_ms),
                "{} delays by {} ms, which is not a step on the scale",
                row.name,
                row.delay_ms
            );
        }
    }

    /// The feel, pinned the way the timing is. A curve swapped under a spec is a
    /// different animation at the same length, which no duration test can see.
    #[test]
    fn every_row_names_the_curve_its_spec_actually_rides() {
        for row in CATALOG {
            let (_, curve) = CURVES
                .iter()
                .find(|(name, _)| *name == row.curve)
                .unwrap_or_else(|| {
                    panic!("{} names {}, which is not in CURVES", row.name, row.curve)
                });
            assert_eq!(
                row.spec.curve(),
                *curve,
                "{} rides a different curve than the {} it claims",
                row.name,
                row.curve
            );
        }
    }

    /// Every spec, sampled across its own span.
    ///
    /// The class closed here is a transition that never finishes: a spec whose
    /// progress does not reach 1.0 by the end of its own total leaves whatever
    /// it animates part-way, forever, and no duration assertion can see it.
    /// Boundaries, mid-flight, monotonicity, and the delay are all held, so a
    /// curve or a delay that inverts the ramp fails here.
    #[test]
    fn every_spec_starts_at_rest_moves_once_and_terminates() {
        for row in CATALOG {
            let spec = row.spec;
            let name = row.name;
            assert_eq!(spec.progress(0.0), 0.0, "{name} starts at rest");
            assert_eq!(spec.progress(1.0), 1.0, "{name} finishes at its end");
            assert_eq!(spec.progress(2.0), 1.0, "{name} stays finished past it");

            // A spec on an overshooting curve is allowed past its target and
            // back, and only by the bound `eval` clamps to. Which rule applies
            // is read off the curve, so the one spring spec is held to the
            // spring rules and every other spec stays monotone.
            let springy = spec.curve().overshoots();
            let (_, ceiling) = spec.curve().bounds();
            let mut last = 0.0;
            for i in 0..=64 {
                let t = i as f32 / 64.0;
                let p = spec.progress(t);
                assert!(
                    (0.0..=ceiling).contains(&p),
                    "{name} left [0, {ceiling}] at {t}: {p}"
                );
                assert!(springy || p >= last - 1e-4, "{name} went backwards at {t}");
                last = p;
            }

            let span = (spec.delay_ms + spec.duration_ms) as f32;
            let halfway = (spec.delay_ms as f32 + spec.duration_ms as f32 / 2.0) / span;
            let mid = spec.progress(halfway);
            // Mid-flight means moving and not yet at rest. A springy spec is
            // past its target at halfway — `FLING` reads 1.087 there — so the
            // upper bound is the one its own curve earns, not 1.0.
            assert!(
                mid > 0.0 && mid < ceiling,
                "{name} is not mid-flight halfway through its own run: {mid}"
            );

            if spec.delay_ms > 0 {
                let inside = spec.delay_ms as f32 / 2.0 / span;
                assert_eq!(
                    spec.progress(inside),
                    0.0,
                    "{name} moves during its own delay"
                );
            }

            assert!(
                spec.total() > std::time::Duration::ZERO
                    && spec.total() <= std::time::Duration::from_secs(3),
                "{name} runs for {:?}, which is not a transition",
                spec.total()
            );
        }
    }

    /// A spec nothing spends is dead timing the catalog would otherwise
    /// advertise. The named file has to exist and to mention both the spec and
    /// the item that spends it.
    #[test]
    fn every_row_names_a_call_site_that_exists() {
        let root = workspace_root();
        for row in CATALOG {
            let path = root.join(row.file);
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("{} names {}: {err}", row.name, row.file));
            assert!(
                source.contains(row.symbol),
                "{} names {} in {}, which does not contain it",
                row.name,
                row.symbol,
                row.file
            );
            assert!(
                source.contains(row.name),
                "{} is not mentioned in {}, so the row points at the wrong place",
                row.name,
                row.file
            );
            assert!(
                !row.purpose.is_empty(),
                "{} has a call site and no purpose",
                row.name
            );
        }
    }

    /// The document is the operator-facing half of the catalog. A spec missing
    /// from it, or a spec whose curve or duration is not written down, is a
    /// timing nobody outside this file can look up.
    #[test]
    fn the_document_names_every_spec_its_timing_and_its_curve() {
        let doc = std::fs::read_to_string(workspace_root().join(DOC))
            .unwrap_or_else(|err| panic!("{DOC}: {err}"));
        for row in CATALOG {
            assert!(doc.contains(row.name), "{DOC} does not name {}", row.name);
            let ms = format!("{} ms", row.duration_ms);
            assert!(
                doc.contains(&ms),
                "{DOC} does not give {} its duration ({ms})",
                row.name
            );
            assert!(
                doc.contains(row.curve),
                "{DOC} does not name {}'s curve ({})",
                row.name,
                row.curve
            );
        }
    }

    /// The scale is the other half of a duration: a step the document does not
    /// name is a length a reviewer cannot look up, and the gallery and this
    /// table both read the same list.
    #[test]
    fn the_document_names_every_step_on_the_scale() {
        let doc = std::fs::read_to_string(workspace_root().join(DOC))
            .unwrap_or_else(|err| panic!("{DOC}: {err}"));
        for (name, beat) in SCALE {
            assert!(doc.contains(name), "{DOC} does not name the step {name}");
            assert!(
                doc.contains(&beat.to_string()),
                "{DOC} does not give {name} its length ({beat})"
            );
        }
        for (name, _) in CURVES {
            assert!(doc.contains(name), "{DOC} does not name the curve {name}");
        }
    }
}
