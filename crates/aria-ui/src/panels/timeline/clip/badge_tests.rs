//! The fx badge with nothing applied reads on its clip alike in both
//! appearances.
//!
//! The class closed here: a badge on a clip filled from a chrome token. A clip's
//! label fill is the same in both appearances, so a badge in `text_muted` drew
//! a light chip under dark ink on a tan clip in the dark look and a dark chip
//! under light ink in the light look, and on several labels in the dark look
//! the chip all but vanished into the fill. For every label, selected and not,
//! in both appearances: the resting badge is the same color, its glyph keeps a
//! contrast of 3:1 against the badge laid over the fill, and the badge stands
//! off the fill at 1.3:1. The labels are read from `LabelColor::ALL` at run
//! time, so a label added later is checked on the run that adds it.
//!
//! Not covered: an offline clip, whose red fill is the appearance's `danger`
//! and so differs between the looks by design; the yellow and purple badges,
//! which `bar_tests` pins.

use aria_models::{ClipId, LabelColor};
use gpui::{Background, Entity, VisualTestContext};

use super::fx_colors;
use crate::app::AppState;
use crate::theme::{Theme, contrast_ratio, flatten};
use crate::types::UiFxBadge;

/// The resting badge `clip` draws, as the frame paints its fill.
pub(super) fn rest(
    visual: &mut VisualTestContext,
    state: &Entity<AppState>,
    clip: ClipId,
) -> Background {
    let theme = visual.read(Theme::of);
    let drawn = visual
        .read(|app| state.read(app).ui_clip(clip))
        .expect("the placed clip draws a bar");
    let ink = super::super::ink(&theme, &drawn);
    Background::from(fx_colors(&theme, UiFxBadge::default(), ink).0)
}

#[test]
fn the_resting_fx_badge_reads_on_every_label_alike_in_both_looks() {
    let (dark, light) = (Theme::dark(), Theme::light());
    let mut offenders = Vec::new();
    for label in LabelColor::ALL {
        for selected in [false, true] {
            let looks = [&dark, &light].map(|theme| {
                let fill = theme.label(label, selected);
                let (badge, glyph) = fx_colors(
                    theme,
                    UiFxBadge::default(),
                    theme.label_ink(label, selected),
                );
                (fill, badge, glyph)
            });
            let what = format!("{label:?}{}", if selected { " selected" } else { "" });
            if looks[0].1 != looks[1].1 || looks[0].2 != looks[1].2 {
                offenders.push(format!("{what}: the badge differs between the looks"));
            }
            for (look, (fill, badge, glyph)) in ["dark", "light"].into_iter().zip(looks) {
                let laid = flatten(badge, fill);
                let glyph_contrast = contrast_ratio(glyph, laid);
                let edge = contrast_ratio(laid, fill);
                if glyph_contrast < 3.0 {
                    offenders.push(format!("{what}, {look}: fx at {glyph_contrast:.2}:1"));
                }
                if edge < 1.3 {
                    offenders.push(format!("{what}, {look}: badge off its fill at {edge:.2}:1"));
                }
            }
        }
    }
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}
