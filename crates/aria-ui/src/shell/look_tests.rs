//! Look invariants of the drawn editor, measured on every painted quad of the
//! shell in every workspace the switcher offers, at the three window sizes the
//! spacing walk draws.
//!
//! The classes closed here:
//!
//! - A soft control: a button, tab, field, bar, knob or toggle whose corners
//!   round past [`CONTROL_CAP`], or a surface's past [`SURFACE_CAP`].
//! - A plated icon control: an armed tool, a toggle that is on, Play or any
//!   other icon control that draws a filled plate at rest instead of its
//!   glyph alone.
//!
//! Exempt, as in the spacing walk: what the canvases that draw the edit paint.

use std::collections::BTreeMap;

use gpui::{TestAppContext, VisualTestContext};

use crate::theme::Theme;

use super::{Edges, Painted, content, inside, intersect, is_empty, painted, walk};

/// The shorter side from which a quad is a surface, a frame or a card, and may
/// round its corners by [`SURFACE_CAP`].
const SURFACE: f32 = 48.0;

/// The most a control rounds its corners by, as `docs/spec/look.md` Density
/// states. A literal rather than [`Theme::CONTROL_RADIUS`], so a token set
/// soft fails here as a site that rounds past it does.
const CONTROL_CAP: f32 = 2.0;

/// The most a menu, a popover, a dialog or a card rounds its corners by, as
/// `docs/spec/look.md` Density states.
const SURFACE_CAP: f32 = 4.0;

/// The least alpha a fill or a border is seen at.
const SEEN: f32 = 0.03;

/// Whether `quad` paints a fill or a border that is seen, and stands outside
/// the `exempt` canvases, with the part of it its mask shows.
fn shown(quad: &Painted, exempt: &[Edges]) -> Option<Edges> {
    let seen = quad.fill.is_some_and(|fill| fill.a > SEEN)
        || (quad.widths.iter().any(|width| *width > 0.0) && quad.border.a > SEEN);
    let shown = intersect(quad.at, quad.mask);
    (seen && !is_empty(shown) && !exempt.iter().any(|region| inside(shown, *region, 0.5)))
        .then_some(shown)
}

/// Every quad of `quads` rounded past its cap, into `failures` keyed by its
/// size and radius with the first case that drew it. Returns how many quads it
/// measured and how many of them were surfaces.
fn soft(
    quads: Vec<Painted>,
    exempt: &[Edges],
    case: &str,
    failures: &mut BTreeMap<String, String>,
) -> (usize, usize) {
    let (mut checked, mut surfaces) = (0, 0);
    for quad in quads {
        if shown(&quad, exempt).is_none() {
            continue;
        }
        checked += 1;
        let (w, h) = (quad.at[2] - quad.at[0], quad.at[3] - quad.at[1]);
        let radius = quad.radii.iter().copied().fold(0.0, f32::max);
        let circle =
            (w - h).abs() <= 0.01 && quad.radii.iter().all(|corner| *corner >= w / 2.0 - 0.01);
        let cap = if w.min(h) >= SURFACE {
            surfaces += 1;
            SURFACE_CAP
        } else {
            CONTROL_CAP
        };
        if !circle && radius > cap + 0.01 {
            failures
                .entry(format!(
                    "a {w:.0}x{h:.0} quad rounded by {radius} px, past {cap}"
                ))
                .or_insert_with(|| format!("{case}, at {:?}", quad.at));
        }
    }
    (checked, surfaces)
}

/// The panic message for `failures`: one line per failure and the first case
/// that drew it.
fn report(failures: &BTreeMap<String, String>) -> String {
    failures
        .iter()
        .map(|(what, first)| format!("{what}: first {first}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// No control is drawn soft. Every quad painted outside the canvases that draw
/// the edit rounds its corners by at most [`CONTROL_CAP`], or by at most
/// [`SURFACE_CAP`] when both its sides are [`SURFACE`] or longer. A circle, a
/// dot or a handle, is round. Each case is measured at rest, and the first
/// also with the File menu open, so a surface is measured.
///
/// The class closed here is a pill or a bubble: a button, a tab, a field, a
/// bar, a knob, a toggle or a menu whose corners round past the cap, whether
/// a radius token is set too soft, a spacing token stands in for a radius, a
/// literal does, or the radius is half the control's height.
///
/// Not caught: a control drawn as a full circle, a quad drawn transparent at
/// rest, and what the walk does not draw: a dialog, a popover other than the
/// File menu, Home.
#[gpui::test]
fn no_control_is_drawn_soft(cx: &mut TestAppContext) {
    let mut failures = BTreeMap::new();
    let (mut checked, mut surfaces) = (0, 0);
    walk(cx, &[false], |case, state, visual| {
        let exempt = content(visual);
        let (quads, _) = soft(painted(visual), &exempt, case, &mut failures);
        checked += quads;
        if surfaces > 0 {
            return;
        }
        let toggle = |visual: &mut VisualTestContext| {
            visual.update(|_, app| {
                state.update(app, |state, cx| {
                    state.toggle_menu(crate::types::AppMenu::File);
                    cx.notify();
                })
            });
            visual.run_until_parked();
        };
        toggle(visual);
        let case = format!("{case}, File menu open");
        let (_, menu) = soft(painted(visual), &exempt, &case, &mut failures);
        surfaces += menu;
        toggle(visual);
    });
    assert!(checked > 0, "the walk drew no quad to measure");
    assert!(
        surfaces > 0,
        "the open File menu drew no surface to measure"
    );
    assert!(failures.is_empty(), "{}", report(&failures));
}

/// The hit boxes of an icon control: on a bar or a header row, on the tool bar
/// and the transport, and inside a list or property row.
const ICON_HITS: [f32; 3] = [Theme::ICON_HIT, Theme::ICON_HIT_LARGE, Theme::ICON_HIT_ROW];

/// No icon control draws a plate at rest. A quad the size of an icon
/// control's hit box, filled where it is seen, is a plate behind a glyph; an
/// icon control at rest is its glyph alone, lit or not, and its wash fades in
/// only under the pointer. The walk draws with snapping and linked selection
/// on and the selection tool armed, so a lit toggle and an armed tool are
/// measured with the rest.
///
/// The class closed here is a contrasted box behind a glyph: the armed tool's
/// selected plate, a lit toggle's, Play's solid plate, or a plate any new icon
/// control draws.
///
/// Not caught: a plate of another size than an icon hit box, a plate drawn
/// only under the pointer or while pressed, and what the walk does not draw.
#[gpui::test]
fn no_icon_control_draws_a_plate_at_rest(cx: &mut TestAppContext) {
    let mut failures = BTreeMap::new();
    let mut lit = 0;
    walk(cx, &[false], |case, state, visual| {
        let (snap, linked) = visual.update(|_, app| {
            let state = state.read(app);
            (state.snap_enabled, state.linked_selection)
        });
        if snap && linked {
            lit += 1;
        }
        let exempt = content(visual);
        for quad in painted(visual) {
            if shown(&quad, &exempt).is_none() || quad.fill.is_none_or(|fill| fill.a <= SEEN) {
                continue;
            }
            let (w, h) = (quad.at[2] - quad.at[0], quad.at[3] - quad.at[1]);
            if ICON_HITS
                .iter()
                .any(|hit| (w - hit).abs() <= 0.01 && (h - hit).abs() <= 0.01)
            {
                failures
                    .entry(format!("a {w:.0}x{h:.0} plate filled {:?}", quad.fill))
                    .or_insert_with(|| format!("{case}, at {:?}", quad.at));
            }
        }
    });
    assert!(lit > 0, "the walk drew no case with its toggles lit");
    assert!(failures.is_empty(), "{}", report(&failures));
}
