//! Tests for [`super`]: the hover store, the identities every animated control
//! is keyed by, the colour blend, and the motion choice in Settings.
//!
//! Curves, specs, loader waveforms and release velocity are `gpui::motion` and
//! are tested there.

use std::time::Duration;

use gpui::motion::MotionPolicy;
use gpui::{AppContext as _, Rgba};

use super::*;

/// A key for one test control. The tests care about a key's identity, not its
/// text, so every case names its own.
fn key(name: &str) -> MotionKey {
    MotionKey::new("test", name)
}

/// Sets the hover toggle of `key` in the frame `frames` has open, as the hover
/// listener does.
fn hover(frames: &TestFrames, key: MotionKey, hovered: bool) {
    frames.update(|cx| set_hover(key, hovered, cx.motion_policy(), cx.frame_instant()));
}

#[test]
fn hover_fade_ramps_and_reverses_continuously() {
    let frames = TestFrames::new();
    let span = presets::HOVER_FADE.total();
    let half = span / 2;
    let pill = key("pill");

    hover(&frames, pill, true);
    assert_eq!(hover_t(pill), 0.0);
    frames.advance(half);
    let mid = hover_t(pill);
    assert!(mid > 0.0 && mid < 1.0, "mid-flight enter: {mid}");
    frames.advance(span - half);
    assert_eq!(hover_t(pill), 1.0);
    frames.advance(span * 2);
    assert_eq!(hover_t(pill), 1.0, "clamps past the end");

    hover(&frames, pill, false);
    frames.advance(half);
    let at_flip = hover_t(pill);
    assert!(
        at_flip > 0.0 && at_flip < 1.0,
        "mid-flight leave: {at_flip}"
    );
    hover(&frames, pill, true);
    let after_flip = hover_t(pill);
    assert!(
        (after_flip - at_flip).abs() < 1e-4,
        "continuity: {at_flip} vs {after_flip}"
    );
    frames.advance(span / 4);
    assert!(hover_t(pill) > after_flip, "climbs back up");
    frames.advance(span);
    assert_eq!(hover_t(pill), 1.0, "lands hovered");
}

#[test]
fn hover_fade_reduced_motion_snaps() {
    let frames = TestFrames::new();
    frames.update(|cx| cx.set_reduce_motion(true));
    let row = key("row");
    hover(&frames, row, true);
    assert_eq!(hover_t(row), 1.0, "enter snaps to 1");
    hover(&frames, row, false);
    assert_eq!(hover_t(row), 0.0, "leave snaps to 0");
    assert!(!frames.end(), "and no frame was requested for either");
}

/// A hover no element reads is dropped, so a menu row that was closed mid-fade
/// does not keep the window drawing frames for it.
#[test]
fn an_unread_hover_is_dropped() {
    let frames = TestFrames::new();
    let frame = Duration::from_millis(16);
    let row = key("menu-row");
    hover(&frames, row, true);
    frames.advance(frame);
    frames.end();
    hover_t(row);
    frames.advance(frame);
    frames.end();
    frames.advance(frame);
    frames.end();
    frames.advance(frame);
    assert_eq!(hover_t(row), 0.0, "unread entry dropped");
}

/// A hover in flight keeps the window drawing until it lands, then stops.
#[test]
fn a_hover_in_flight_requests_frames_until_it_lands() {
    let frames = TestFrames::new();
    let pill = key("frames");
    hover(&frames, pill, true);
    hover_t(pill);
    assert!(frames.end(), "a fade in flight requests the next frame");
    frames.advance(presets::HOVER_FADE.total());
    hover_t(pill);
    assert!(!frames.end(), "a landed fade requests none");
}

/// The key is a hash, so the two things a hash must not do are worth pinning:
/// lose a control's identity between frames, and merge two controls into one.
///
/// The merge case is not hypothetical. Concatenating `kind` and `id` without a
/// separator makes `("row", "a1")` and `("row-a", "1")` the same control, and
/// the symptom, one row lighting up when a different one is hovered, reads as a
/// layout bug rather than a key bug.
#[test]
fn a_motion_key_is_stable_per_control_and_separates_its_parts() {
    assert_eq!(
        MotionKey::new("clip-card", 7),
        MotionKey::new("clip-card", 7),
        "the same control must find its own fade on the next frame"
    );
    assert_ne!(
        MotionKey::new("clip-card", 7),
        MotionKey::new("clip-card", 8)
    );
    assert_ne!(
        MotionKey::new("clip-card", 7),
        MotionKey::new("clip-span", 7)
    );
    assert_ne!(
        MotionKey::new("row", "a1"),
        MotionKey::new("row-a", "1"),
        "kind and id must not run together"
    );
    assert_ne!(
        MotionKey::new("row", ""),
        MotionKey::new("", "row"),
        "an empty part still separates"
    );
}

/// A control identity separates the controls that share it, and separates two
/// rows of the same control from each other.
///
/// The class this closes: an identity that collapses. GPUI keys press and hover
/// state by element id, and the motion stores key their fades and glides by
/// motion key, so an identity that answers the same for two rows shows up as the
/// wrong row lighting under the pointer, and one that answers the same for a row
/// and the button inside it shows up as a row that lights when its own button is
/// hovered.
#[test]
fn a_control_identity_separates_its_rows_and_its_parts() {
    let first = ControlId::Key(MotionKey::new("mixer-strip", 1));
    let second = ControlId::Key(MotionKey::new("mixer-strip", 2));
    assert_ne!(
        first.element("strip"),
        second.element("strip"),
        "two rows of one control must not share an element id"
    );
    assert_eq!(
        first.element("strip"),
        ControlId::Key(MotionKey::new("mixer-strip", 1)).element("strip"),
        "and a row must find its own state on the next frame"
    );
    assert_ne!(
        first.motion("row"),
        first.motion("button"),
        "a row and the button inside it are two animations"
    );
    assert_ne!(
        first.element("row"),
        first.element("button"),
        "and two elements"
    );
}

/// A fixed name keeps the name the source wrote, so an element id stays
/// readable, and it is still separated from every other control's.
#[test]
fn a_fixed_identity_draws_under_its_own_name() {
    assert_eq!(
        ControlId::from("bin-new").element("ghost"),
        gpui::ElementId::Name("bin-new".into()),
        "a name written in the source is the id it draws under"
    );
    assert_eq!(
        ControlId::from("bin-new").fixed_name(),
        Some("bin-new"),
        "a fixed control can use its source name for a tooltip lookup"
    );
    assert_eq!(
        ControlId::Key(MotionKey::new("bin", 7)).fixed_name(),
        None,
        "a row key must not pretend its hash is tooltip text"
    );
    assert_ne!(
        ControlId::from("bin-new").motion("ghost"),
        ControlId::from("bin-remove").motion("ghost")
    );
    assert_ne!(
        ControlId::from("bin-new").motion("ghost"),
        ControlId::from("bin-new").motion("chip"),
        "one name used for two kinds of control is two animations"
    );
}

/// Every loader takes its phase offset from its identity, so a column of them
/// does not pulse as one block. The offset is inside a cycle either way.
#[test]
fn two_identities_pulse_out_of_step_and_stay_in_one_cycle() {
    let cases = [
        ControlId::from("ai-probe-busy"),
        ControlId::from("export-inflight"),
        ControlId::Key(MotionKey::new("job-bar", 1)),
        ControlId::Key(MotionKey::new("job-bar", 2)),
    ];
    for case in cases {
        let phase = case.phase();
        assert!(
            (0.0..1.0).contains(&phase),
            "{case:?} offsets the clock by {phase}, which is outside one cycle"
        );
    }
    for (index, left) in cases.iter().enumerate() {
        for right in &cases[index + 1..] {
            assert_ne!(
                left.phase(),
                right.phase(),
                "{left:?} and {right:?} pulse in lockstep"
            );
        }
    }
}

#[test]
fn mix_endpoints_and_transparent_blend() {
    let rest = crate::theme::neutral(0.235);
    let hover = crate::theme::neutral(0.29);
    assert_eq!(mix(rest, hover, 0.0), rest);
    assert_eq!(mix(rest, hover, 1.0), hover);
    assert_eq!(mix(rest, hover, -1.0), rest, "t clamps low");
    assert_eq!(mix(rest, hover, 2.0), hover, "t clamps high");

    let mid = mix(rest, hover, 0.5);
    assert!(mid.l > rest.l && mid.l < hover.l, "mid lightness {}", mid.l);

    let _guard = crate::theme::lock_appearance();
    crate::theme::set_current_appearance(crate::theme::Appearance::Dark);
    let wash = crate::theme::ink(0.06);
    let half = mix(gpui::transparent_black(), wash, 0.5);
    assert!((half.a - 0.03).abs() < 1e-4, "alpha midpoint {}", half.a);
    let half_rgba = Rgba::from(half);
    assert!(
        half_rgba.r > 0.99 && half_rgba.g > 0.99 && half_rgba.b > 0.99,
        "white wash keeps its hue: {half_rgba:?}"
    );
}

/// The list is the whole setting, so a speed that is not offered, or two rows
/// that read the same, is a defect. The match is exhaustive: adding a variant
/// without deciding what it says here does not compile.
#[test]
fn every_motion_preference_is_offered_once_and_reads_differently() {
    for pref in MotionPreference::ALL {
        let named = match pref {
            MotionPreference::Full => MotionPreference::Full,
            MotionPreference::Brisk => MotionPreference::Brisk,
            MotionPreference::Relaxed => MotionPreference::Relaxed,
            MotionPreference::Off => MotionPreference::Off,
        };
        assert!(
            MotionPreference::ALL.contains(&named),
            "{named:?} is not offered in Settings"
        );
        assert!(
            (MotionPolicy::MIN_DURATION_SCALE..=MotionPolicy::MAX_DURATION_SCALE)
                .contains(&pref.scale()),
            "{pref:?} scales outside the bounds every duration is clamped to"
        );
    }
    let count = MotionPreference::ALL.len();
    let mut labels: Vec<&str> = MotionPreference::ALL.iter().map(|p| p.label()).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), count, "two motion rows share a label");
    let mut details: Vec<&str> = MotionPreference::ALL.iter().map(|p| p.detail()).collect();
    details.sort_unstable();
    details.dedup();
    assert_eq!(details.len(), count, "two motion rows say the same thing");
}

/// Off is reduced motion rather than a very short duration: every primitive
/// checks reduced motion and takes its end state, so nothing is drawn
/// mid-flight and no animation frames are requested.
#[test]
fn no_motion_is_reduced_motion_rather_than_a_short_one() {
    assert!(MotionPreference::Off.reduced());
    assert_eq!(MotionPreference::Off.scale(), 1.0);
    for pref in [
        MotionPreference::Full,
        MotionPreference::Brisk,
        MotionPreference::Relaxed,
    ] {
        assert!(!pref.reduced(), "{pref:?} should still animate");
    }
    assert!(MotionPreference::Brisk.scale() < MotionPreference::Full.scale());
    assert!(MotionPreference::Relaxed.scale() > MotionPreference::Full.scale());
}

/// The choice has to reach the durations that are drawn. Applying a choice
/// installs its policy on the app, reduced flag and scale together, and the
/// policy scales every spec by the choice's multiplier.
#[test]
fn a_motion_preference_changes_what_a_transition_costs() {
    let cx = gpui::TestAppContext::single();
    let base = presets::FADE_IN.total();
    for pref in MotionPreference::ALL {
        let installed = cx.update(|cx| {
            pref.apply(cx);
            cx.motion_policy()
        });
        assert_eq!(
            installed,
            pref.policy(),
            "{} was not installed",
            pref.label()
        );
        assert_eq!(installed.reduced(), pref.reduced(), "{}", pref.label());
        assert_eq!(
            installed.scale(base),
            base.mul_f32(pref.scale()),
            "{} duration",
            pref.label()
        );
    }
    // A scale left over from one choice never applies to the next.
    let after = cx.update(|cx| {
        MotionPreference::Relaxed.apply(cx);
        MotionPreference::Full.apply(cx);
        cx.motion_policy()
    });
    assert_eq!(after, MotionPolicy::DEFAULT);
}

/// A pulse is a phase in one cycle of its spec, shared by every view that
/// draws one, so two loaders on screen stay phase-locked.
#[test]
fn two_views_read_one_pulse_phase() {
    let cx = gpui::TestAppContext::single();
    cx.executor().advance_clock(Duration::from_millis(1));
    let (first, second) = cx.update(|cx| {
        let first = cx.new(|_| gpui::Empty);
        let second = cx.new(|_| gpui::Empty);
        let a = pulse_delta(&presets::ZERON_PULSE, first.entity_id(), cx);
        let b = pulse_delta(&presets::ZERON_PULSE, second.entity_id(), cx);
        (a, b)
    });
    assert_eq!(first, second, "two views on one clock read one phase");
    cx.executor()
        .advance_clock(presets::ZERON_PULSE.total() / 4);
    let later = cx.update(|cx| {
        let view = cx.new(|_| gpui::Empty);
        pulse_delta(&presets::ZERON_PULSE, view.entity_id(), cx)
    });
    assert!(
        (later - (first + 0.25)).abs() < 1e-3,
        "a quarter period later the phase is a quarter further: {first} then {later}"
    );
}
