//! Where a row's eased value fills its track, and how much of the accent its
//! readout leads with while the value is still arriving.
//!
//! Closes the class "a lead measured in the stored units rather than along the
//! travel". A row's readout is lit by how far its eased value still has to go,
//! counted in nudges, a twentieth of the track's travel: a gain, whose travel is
//! even in decibels, a narrow range and a wide one all light up over the same
//! share of their travel, and a row with no track counts in its own units.
//! Every case is settled, half a nudge out, and a nudge or more out.
//!
//! Not covered: the color the lead mixes, which `motion::mix` owns, and the
//! glide that eases the value, which `motion_glide` tests.

use aria_analysis::linear_from_db;
use aria_models::ParamRange;

use super::Travel;
use crate::types::Taper;

/// Floats from arithmetic on the travel.
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn assert_reads(travel: Travel, eased: f32, fill: Option<f32>, lead: f32) {
    let (drawn, lit) = travel.reads(eased);
    assert!(
        match (drawn, fill) {
            (Some(drawn), Some(fill)) => close(drawn, fill),
            (None, None) => true,
            _ => false,
        } && close(lit, lead),
        "{travel:?} eased to {eased} reads {drawn:?} lit {lit}, expected {fill:?} lit {lead}"
    );
}

#[test]
fn a_bounded_row_leads_by_the_nudges_left_along_its_travel() {
    // A nudge is a twentieth of the travel: one unit of 0..20, a tenth of 0..2.
    let wide = Travel::along(10.0, &ParamRange::between(0.0, 20.0));
    assert_reads(wide, 10.0, Some(0.5), 0.0);
    assert_reads(wide, 10.5, Some(0.525), 0.5);
    assert_reads(wide, 9.0, Some(0.45), 1.0);
    assert_reads(wide, 16.0, Some(0.8), 1.0);
    let narrow = Travel::along(1.0, &ParamRange::between(0.0, 2.0));
    assert_reads(narrow, 1.0, Some(0.5), 0.0);
    assert_reads(narrow, 1.05, Some(0.525), 0.5);
    assert_reads(narrow, 0.9, Some(0.45), 1.0);
}

#[test]
fn a_gain_row_leads_by_the_nudges_left_along_its_decibels() {
    // From -60 dB to +20 dB a nudge is 4 dB, and unity stands at 0.75.
    let unity = Travel {
        value: 1.0,
        taper: Some(Taper::Gain { max: 10.0 }),
    };
    assert_reads(unity, 1.0, Some(0.75), 0.0);
    assert_reads(unity, linear_from_db(-2.0), Some(0.725), 0.5);
    assert_reads(unity, linear_from_db(4.0), Some(0.8), 1.0);
    assert_reads(unity, 0.0, Some(0.0), 1.0);
}

#[test]
fn a_row_with_no_track_leads_by_its_own_units() {
    let open = Travel::along(3.0, &ParamRange::ANY);
    assert_reads(open, 3.0, None, 0.0);
    assert_reads(open, 3.5, None, 0.5);
    assert_reads(open, 1.0, None, 1.0);
    // Bounds that do not part leave no travel to draw.
    let pinched = Travel::along(3.0, &ParamRange::between(3.0, 3.0));
    assert_reads(pinched, 3.25, None, 0.25);
}
