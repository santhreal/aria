//! The motion catalog as a table: every spec the editor spends, what it is for,
//! and where it is spent.
//!
//! The specs are plain consts: the reference presets in
//! [`gpui::motion::presets`], and the editor's own in [`super`]. A caller writes
//! `presets::FADE_IN` or `motion::LOADER_WAIT`, not a lookup. This table makes the
//! catalog reviewable: a spec that nothing spends is dead timing, a spec whose
//! duration moved retimes the whole app, and a spec that no document mentions
//! is a number nobody can look up. The tests below hold all three, and the row
//! is the one place a new spec has to be declared.

use gpui::motion::{Easing, MotionSpec, presets};

use super::{CARET_BLINK, FLING, LOADER_WAIT};

/// One spec, and the claims made about it.
#[derive(Debug)]
pub struct SpecRow {
    /// The const, as it is written in `presets` or in `motion.rs`.
    pub name: &'static str,
    pub spec: MotionSpec,
    /// The timing this row promises. Kept beside the spec rather than read out
    /// of it, so a retime is a failing test instead of a silent change to every
    /// transition in the app.
    pub delay_ms: u64,
    pub duration_ms: u64,
    /// The curve it rides. Two specs with the same duration and different
    /// curves are not the same motion, so the feel is pinned here the way the
    /// timing is.
    pub easing: Easing,
    /// What the timing is for, in terms of what the screen shows.
    pub purpose: &'static str,
    /// The file that spends it, relative to the workspace root.
    pub file: &'static str,
    /// The item in that file which spends it.
    pub symbol: &'static str,
}

/// Every spec in the catalog. Order is the order the document lists them.
pub const CATALOG: &[SpecRow] = &[
    SpecRow {
        name: "FADE_IN",
        spec: presets::FADE_IN,
        easing: Easing::EaseOutExpo,
        delay_ms: 0,
        duration_ms: 500,
        purpose: "An element arriving for the first time: a launch card, a panel body.",
        file: "crates/aria-ui/src/motion.rs",
        symbol: "fn fade_in",
    },
    SpecRow {
        name: "FADE_QUICK",
        spec: presets::FADE_QUICK,
        easing: Easing::Ease,
        delay_ms: 0,
        duration_ms: 150,
        purpose: "A small change that must not read as a jump: a seek badge, a drop hint.",
        file: "crates/aria-ui/src/motion.rs",
        symbol: "fn fade_quick",
    },
    SpecRow {
        name: "MENU_IN",
        spec: presets::MENU_IN,
        easing: Easing::Ease,
        delay_ms: 0,
        duration_ms: 140,
        purpose: "A menu or popover opening under the pointer.",
        file: "crates/aria-ui/src/motion.rs",
        symbol: "fn menu_in",
    },
    SpecRow {
        name: "MENU_OUT",
        spec: presets::MENU_OUT,
        easing: Easing::Ease,
        delay_ms: 0,
        duration_ms: 100,
        purpose: "Anything leaving: a menu, a dialog, a context menu, the rename bar. \
                  Also the window an overlay keeps drawing after its condition went false.",
        file: "crates/aria-ui/src/plugins/overlay_host.rs",
        symbol: "fn exit_window",
    },
    SpecRow {
        name: "DIALOG_IN",
        spec: presets::DIALOG_IN,
        easing: Easing::Ease,
        delay_ms: 0,
        duration_ms: 180,
        purpose: "A dialog surface arriving: slower than a menu because it takes the \
                  keyboard. Also a whole route arriving, which is the same size of \
                  change to the window.",
        file: "crates/aria-ui/src/motion.rs",
        symbol: "fn dialog_in",
    },
    SpecRow {
        name: "SPLASH_OUT",
        spec: presets::SPLASH_OUT,
        easing: Easing::Ease,
        delay_ms: 150,
        duration_ms: 500,
        purpose: "The boot mark leaving. The delay keeps a fast start from flashing.",
        file: "crates/aria-ui/src/motion.rs",
        symbol: "fn splash_out",
    },
    SpecRow {
        name: "LOADER_WAIT",
        spec: LOADER_WAIT,
        easing: Easing::EaseOutExpo,
        delay_ms: 0,
        duration_ms: 320,
        purpose: "The span a wait must outlast before a loader appears. Only the \
                  span is read: a wait shorter than this leaves no loader on screen.",
        file: "crates/aria-ui/src/motion_glide.rs",
        symbol: "pub(crate) const WAIT",
    },
    SpecRow {
        name: "RESIZE",
        spec: presets::RESIZE,
        easing: Easing::EaseOut,
        delay_ms: 0,
        duration_ms: 200,
        purpose: "Geometry settling after the hand let go: a clip that was rippled, and the \
                  horizontal scale and scroll offset when the editor rather than the pointer \
                  chose them.",
        file: "crates/aria-ui/src/motion_glide.rs",
        symbol: "EDIT",
    },
    SpecRow {
        name: "TAB_SLIDE",
        spec: presets::TAB_SLIDE,
        easing: Easing::EaseOut,
        delay_ms: 0,
        duration_ms: 150,
        purpose: "The inspector's tab indicator traveling to the tab that was clicked.",
        file: "crates/aria-ui/src/panels/inspector/tab_strip.rs",
        symbol: "TAB_SLIDE",
    },
    SpecRow {
        name: "COLLAPSE",
        spec: presets::COLLAPSE,
        easing: Easing::EaseOut,
        delay_ms: 0,
        duration_ms: 180,
        purpose: "A lane height opening or closing, and the property rows a twirl opens \
                  under it.",
        file: "crates/aria-ui/src/motion_glide.rs",
        symbol: "LANE",
    },
    SpecRow {
        name: "CHEVRON",
        spec: presets::CHEVRON,
        easing: Easing::Ease,
        delay_ms: 0,
        duration_ms: 200,
        purpose: "A disclosure arrow rotating, so the arrow and the section agree.",
        file: "crates/aria-ui/src/panels/inspector/blocks/widgets_keys.rs",
        symbol: "CHEVRON",
    },
    SpecRow {
        name: "SCROLL_GLIDE",
        spec: presets::SCROLL_GLIDE,
        easing: Easing::EaseInOut,
        delay_ms: 0,
        duration_ms: 500,
        purpose: "The assistant transcript following its newest reply. Long, because the \
                  rows travel through the line being read and a jump that size reads as a cut.",
        file: "crates/aria-ui/src/motion_glide.rs",
        symbol: "SCROLL",
    },
    SpecRow {
        name: "FLING",
        spec: FLING,
        easing: Easing::EaseOutBack,
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
        spec: presets::HOVER_FADE,
        easing: Easing::Standard,
        delay_ms: 0,
        duration_ms: 150,
        purpose: "Every hover blend in the app, over one store so a strip fades at one \
                  rate; and, at the same beat, the binary states a clip card can leave \
                  again: selected and locked.",
        file: "crates/aria-ui/src/motion_hover.rs",
        symbol: "fn set_hover",
    },
    SpecRow {
        name: "ZERON_PULSE",
        spec: presets::ZERON_PULSE,
        easing: Easing::Ease,
        delay_ms: 0,
        duration_ms: 2400,
        purpose: "The loader's breathing cycle while work is in flight.",
        file: "crates/aria-ui/src/loaders.rs",
        symbol: "ZERON_PULSE",
    },
    SpecRow {
        name: "GRADIENT_SPIN",
        spec: presets::GRADIENT_SPIN,
        easing: Easing::Ease,
        delay_ms: 0,
        duration_ms: 750,
        purpose: "The loader's gradient sweep, faster than the pulse it runs inside.",
        file: "crates/aria-ui/src/loaders.rs",
        symbol: "GRADIENT_SPIN",
    },
    SpecRow {
        name: "CARET_BLINK",
        spec: CARET_BLINK,
        easing: Easing::Standard,
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

    /// Every spec the editor can spend: each entry of [`presets::PRESETS`], and
    /// every `pub const` of type `MotionSpec` declared in `motion.rs`, read from
    /// the source at build time. A preset added to the framework, or a spec
    /// added to `motion.rs`, without a row here fails rather than going
    /// undocumented.
    fn declared_specs() -> Vec<&'static str> {
        let own = include_str!("motion.rs").lines().filter_map(|line| {
            let rest = line.trim().strip_prefix("pub const ")?;
            let (name, tail) = rest.split_once(':')?;
            tail.trim_start()
                .starts_with("MotionSpec")
                .then_some(name.trim())
        });
        presets::PRESETS
            .iter()
            .map(|(name, _)| *name)
            .chain(own)
            .collect()
    }

    #[test]
    fn every_declared_spec_has_a_catalog_row_and_every_row_is_declared() {
        let declared = declared_specs();
        assert!(
            declared.len() > presets::PRESETS.len(),
            "no spec parsed out of motion.rs; the scan is broken, not the catalog"
        );
        for name in &declared {
            assert_eq!(
                CATALOG.iter().filter(|row| row.name == *name).count(),
                1,
                "{name} needs exactly one catalog row"
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

    /// A row that names one preset and holds another compiles, and documents
    /// the wrong timing under the right name.
    #[test]
    fn every_preset_row_holds_the_preset_it_names() {
        for (name, spec) in presets::PRESETS {
            let row = CATALOG
                .iter()
                .find(|row| row.name == name)
                .unwrap_or_else(|| panic!("{name} has no catalog row"));
            assert_eq!(row.spec, spec, "the {name} row holds a different spec");
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
        }
    }

    /// The feel, pinned the way the timing is. A curve swapped under a spec is a
    /// different animation at the same length, which no duration test can see.
    #[test]
    fn every_row_names_the_curve_its_spec_rides() {
        for row in CATALOG {
            assert_eq!(
                row.spec.easing,
                row.easing,
                "{} rides {} rather than the {} it claims",
                row.name,
                row.spec.easing.name(),
                row.easing.name()
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
            // is read off the curve, so the one overshooting spec is held to
            // those rules and every other spec stays monotone.
            let curve = spec.easing.bezier();
            let springy = curve.is_some_and(|curve| curve.overshoots());
            let ceiling = curve.map_or(1.0, |curve| curve.bounds().1);
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
            // Mid-flight means moving and not yet at rest. An overshooting spec
            // is past its target at halfway, so the upper bound is the one its
            // own curve earns, not 1.0.
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

    /// The document is the published half of the catalog. A spec missing
    /// from it, or a spec whose curve, duration, or delay is not written down,
    /// is a timing nobody outside this file can look up.
    #[test]
    fn the_document_names_every_spec_its_timing_and_its_curve() {
        let doc = std::fs::read_to_string(workspace_root().join(DOC))
            .unwrap_or_else(|err| panic!("{DOC}: {err}"));
        for row in CATALOG {
            let line = doc
                .lines()
                .find(|line| line.starts_with(&format!("| `{}` |", row.name)))
                .unwrap_or_else(|| panic!("{DOC} has no table row for {}", row.name));
            let ms = format!("| {} ms |", row.duration_ms);
            assert!(
                line.contains(&ms),
                "{DOC} does not give {} its duration ({ms})",
                row.name
            );
            let delay = match row.delay_ms {
                0 => "| 0 |".to_owned(),
                ms => format!("| {ms} ms |"),
            };
            assert!(
                line.contains(&delay),
                "{DOC} does not give {} its delay ({delay})",
                row.name
            );
            let curve = format!("`{}`", row.easing.name());
            assert!(
                line.contains(&curve),
                "{DOC} does not name {}'s curve ({curve})",
                row.name
            );
        }
    }

    /// Every curve a row rides is listed with its handles, so a reviewer can
    /// read the feel of a spec without opening the framework.
    #[test]
    fn the_document_lists_every_curve_the_catalog_rides() {
        let doc = std::fs::read_to_string(workspace_root().join(DOC))
            .unwrap_or_else(|err| panic!("{DOC}: {err}"));
        for row in CATALOG {
            let name = row.easing.name();
            assert!(
                doc.lines()
                    .any(|line| line.starts_with(&format!("| `{name}` |"))),
                "{DOC} has no curve row for {name}, which {} rides",
                row.name
            );
        }
    }
}
