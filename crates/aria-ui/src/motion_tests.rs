//! Tests for [`super`].
//!
//! Moved out of `motion.rs` to stay under the file ceiling. The
//! module is unchanged; only its location moved.
use super::*;

fn assert_close(actual: f32, expected: f32, tol: f32, ctx: &str) {
    assert!(
        (actual - expected).abs() <= tol,
        "{ctx}: got {actual}, expected {expected} ±{tol}"
    );
}

/// A curve that stays inside the unit square may never leave it; one whose
/// handle passes 1.0 may leave it only by the bound `eval` clamps to. Which
/// rule a curve is held to is read off its own control points, so a curve added
/// with an overshooting handle is checked as a spring without anyone saying so,
/// and one added without a handle past 1.0 that overshoots anyway fails here.
#[test]
fn eval_stays_inside_the_bound_its_own_handles_earn() {
    for (name, curve) in CURVES {
        let (floor, ceiling) = curve.bounds();
        let mut peak = 0.0f32;
        for i in 0..=100_000u32 {
            let x = i as f32 / 100_000.0;
            let y = curve.eval(x);
            assert!(
                (floor..=ceiling).contains(&y),
                "{name}: eval({x}) = {y} left [{floor}, {ceiling}]"
            );
            peak = peak.max(y);
        }
        for x in [0.999_999f32, 0.999_999_9, 1.0 - f32::EPSILON] {
            let y = curve.eval(x);
            assert!(
                (floor..=ceiling).contains(&y),
                "{name}: eval({x}) = {y} left [{floor}, {ceiling}]"
            );
        }
        assert_eq!(
            curve.overshoots(),
            peak > 1.0,
            "{name} peaks at {peak}, which is not what its handles claim"
        );
    }
}

#[test]
fn bezier_linear_is_identity() {
    let linear = CubicBezier::new(0.0, 0.0, 1.0, 1.0);
    for x in [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
        assert_close(linear.eval(x), x, 1e-4, "linear");
    }
}

#[test]
fn bezier_known_values() {
    let cases: [(&str, CubicBezier, [f32; 5]); 3] = [
        (
            "expo",
            EASE_OUT_EXPO,
            [0.494391, 0.825622, 0.971779, 0.997677, 0.999878],
        ),
        (
            "ease-out",
            EASE_OUT,
            [0.160572, 0.378138, 0.684643, 0.906535, 0.982973],
        ),
        (
            "ease",
            EASE,
            [0.094796, 0.408511, 0.802403, 0.960459, 0.994316],
        ),
    ];
    for (name, curve, expected) in cases {
        for (x, want) in [0.1, 0.25, 0.5, 0.75, 0.9].into_iter().zip(expected) {
            assert_close(curve.eval(x), want, 1e-3, name);
        }
    }
}

#[test]
fn bezier_endpoints_and_clamping() {
    for (_, curve) in CURVES {
        assert_eq!(curve.eval(0.0), 0.0);
        assert_eq!(curve.eval(1.0), 1.0);
        assert_eq!(curve.eval(-0.5), 0.0);
        assert_eq!(curve.eval(1.5), 1.0);
    }
}

/// Every curve climbs and only a spring comes back down. A curve that reverses
/// without an overshooting handle is a feel nobody chose; one that claims the
/// overshoot and never returns would leave whatever it moves past its target.
#[test]
fn only_a_spring_leaves_its_target_and_returns() {
    for (name, curve) in CURVES {
        let mut last = 0.0;
        let mut descended = false;
        for i in 0..=1000 {
            let y = curve.eval(i as f32 / 1000.0);
            descended |= y < last - 1e-4;
            last = y;
        }
        assert_eq!(
            curve.overshoots(),
            descended,
            "{name} descends: {descended}, which is not what its handles claim"
        );
        assert_eq!(curve.eval(1.0), 1.0, "{name} does not end on its target");
    }
}

#[test]
fn spec_delay_holds_then_runs() {
    assert_eq!(SPLASH_OUT.total(), Duration::from_millis(660));
    assert_eq!(SPLASH_OUT.progress(0.0), 0.0);
    assert_eq!(SPLASH_OUT.progress(0.2), 0.0);
    assert_eq!(SPLASH_OUT.progress(1.0), 1.0);
    assert_eq!(SPLASH_OUT.progress(2.0), 1.0);
    let mid = SPLASH_OUT.progress(0.65);
    assert!(mid > 0.0 && mid < 1.0);
    assert_close(
        FADE_IN.progress(0.5),
        EASE_OUT_EXPO.eval(0.5),
        1e-6,
        "no-delay",
    );
}

/// Every spec, and the step it is timed to. The pairing is the claim: a spec
/// retimed to a different step has to be retimed here too, and a spec timed to
/// something that is not a step cannot be written at all — [`MotionSpec::new`]
/// takes a [`Beat`], and only this crate's scale mints one.
#[test]
fn every_spec_is_timed_to_a_step_on_the_scale() {
    let specs: [(&str, MotionSpec, Beat); 16] = [
        ("FADE_IN", FADE_IN, GLIDE),
        ("FADE_QUICK", FADE_QUICK, QUICK),
        ("MENU_IN", MENU_IN, QUICK),
        ("MENU_OUT", MENU_OUT, FLICK),
        ("DIALOG_IN", DIALOG_IN, SNAP),
        ("SPLASH_OUT", SPLASH_OUT, GLIDE),
        ("OPEN", OPEN, SETTLE),
        ("RESIZE", RESIZE, SNAP),
        ("TAB_SLIDE", TAB_SLIDE, QUICK),
        ("COLLAPSE", COLLAPSE, SNAP),
        ("CHEVRON", CHEVRON, SNAP),
        ("SCROLL_GLIDE", SCROLL_GLIDE, GLIDE),
        ("HOVER_FADE", HOVER_FADE, QUICK),
        ("ZERON_PULSE", ZERON_PULSE, BREATH),
        ("GRADIENT_SPIN", GRADIENT_SPIN, CYCLE),
        ("CARET_BLINK", CARET_BLINK, BLINK),
    ];
    for (name, spec, beat) in specs {
        assert_eq!(
            spec.duration_ms(),
            beat.ms(),
            "{name} is no longer timed to {beat}"
        );
        assert!(
            on_scale(spec.duration_ms()),
            "{name} runs for {} ms, which is not a step",
            spec.duration_ms()
        );
    }
    assert_eq!(SPLASH_OUT.delay_ms(), QUICK.ms());
    assert_eq!(EASE_OUT_EXPO, CubicBezier::new(0.16, 1.0, 0.3, 1.0));
}

#[test]
fn pulse_wave_endpoints() {
    assert_close(pulse_wave(0.0), 0.0, 1e-6, "wave start");
    assert_close(pulse_wave(0.5), 1.0, 1e-6, "wave peak");
    assert_close(pulse_wave(1.0), 0.0, 1e-6, "wave end");
    assert_close(pulse_opacity(0.0), 0.08, 1e-6, "opacity floor");
    assert_close(pulse_opacity(0.5), 1.0, 1e-6, "opacity peak");
    assert_close(pulse_scale(0.0), 0.9, 1e-6, "scale floor");
    assert_close(pulse_scale(0.5), 1.0, 1e-6, "scale peak");
}

#[test]
fn stagger_wraps_and_orders_cells() {
    assert_close(staggered_phase(0.0, 0, PULSE_STAGGER), 0.0, 1e-6, "cell 0");
    assert_close(
        staggered_phase(0.0, 1, PULSE_STAGGER),
        1.0 - PULSE_STAGGER,
        1e-5,
        "cell 1 wraps",
    );
    assert_close(
        staggered_phase(0.3, 2, PULSE_STAGGER),
        staggered_phase(0.3 + 1.0, 2, PULSE_STAGGER),
        2e-6,
        "periodic",
    );
    let peak0 = matrix_wave(0.5, 0, 5);
    assert_close(peak0, 1.0, 1e-5, "diag 0 peak at half period");
}

#[test]
fn lerp_basics() {
    assert_eq!(lerp(208.0, 400.0, 0.0), 208.0);
    assert_eq!(lerp(208.0, 400.0, 1.0), 400.0);
    assert_eq!(lerp(0.0, 10.0, 0.5), 5.0);
}

/// A key for one test control. The tests care about a key's identity, not its
/// text, so every case names its own.
fn key(name: &str) -> MotionKey {
    MotionKey::new("test", name)
}

/// The fade's own span, which is what the store animates over: the spec's total
/// through the speed multiplier. Typed lengths went stale here once already —
/// `QUICK` is 160 ms and the assertions said 150 — so every instant below is a
/// fraction of this.
fn hover_span() -> Duration {
    HOVER_FADE.total().mul_f32(speed_scale())
}

#[test]
fn hover_fade_ramps_and_reverses_continuously() {
    let mut fades = HoverFades::default();
    let t0 = Instant::now();
    let span = hover_span();
    let half = span / 2;
    let pill = key("pill");

    fades.set_at(pill, true, false, t0);
    assert_eq!(fades.value_at(pill, t0), 0.0);
    let mid = fades.value_at(pill, t0 + half);
    assert!(mid > 0.0 && mid < 1.0, "mid-flight enter: {mid}");
    assert_eq!(fades.value_at(pill, t0 + span), 1.0);
    assert_eq!(
        fades.value_at(pill, t0 + span * 3),
        1.0,
        "clamps past the end"
    );

    fades.set_at(pill, true, false, t0);
    let at_flip = fades.value_at(pill, t0 + half);
    fades.set_at(pill, false, false, t0 + half);
    let after_flip = fades.value_at(pill, t0 + half);
    assert!(
        (after_flip - at_flip).abs() < 1e-4,
        "continuity: {at_flip} vs {after_flip}"
    );
    let falling = fades.value_at(pill, t0 + half + span / 4);
    assert!(falling < after_flip, "fades back down");
    assert_eq!(fades.value_at(pill, t0 + half + span), 0.0, "lands at rest");
}

#[test]
fn hover_fade_reduced_motion_snaps() {
    let mut fades = HoverFades::default();
    let t0 = Instant::now();
    let row = key("row");
    fades.set_at(row, true, true, t0);
    assert_eq!(fades.value_at(row, t0), 1.0, "enter snaps to 1");
    fades.set_at(row, false, true, t0);
    assert_eq!(fades.value_at(row, t0), 0.0, "leave snaps to 0");
}

#[test]
fn hover_tick_evicts_unread_entries() {
    let mut fades = HoverFades::default();
    let t0 = Instant::now();
    let ms = |m: u64| t0 + Duration::from_millis(m);
    let row = key("menu-row");
    fades.set_at(row, true, false, t0);
    fades.tick_at(ms(16));
    fades.value_at(row, ms(16));
    fades.tick_at(ms(32));
    fades.tick_at(ms(48));
    assert!(fades.entries.is_empty(), "unread entry evicted");
    assert_eq!(fades.value_at(row, ms(64)), 0.0);
}

/// The key is a hash, so the two things a hash must not do are worth pinning:
/// lose a control's identity between frames, and merge two controls into one.
///
/// The merge case is not hypothetical. Concatenating `kind` and `id` without a
/// separator makes `("row", "a1")` and `("row-a", "1")` the same control, and
/// the symptom — one row lighting up when a different one is hovered — reads as
/// a layout bug rather than a key bug.
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
/// state by element id, and the three motion stores key their fades and glides
/// by motion key, so an identity that answers the same for two rows shows up as
/// the wrong row lighting under the pointer — and one that answers the same for
/// a row and the button inside it shows up as a row that lights when its own
/// button is hovered.
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

#[test]
fn the_hover_fade_is_a_quick_step_on_the_platform_curve() {
    assert_eq!(HOVER_FADE.duration_ms(), QUICK.ms());
    assert_eq!(HOVER_FADE.delay_ms(), 0);
    assert_eq!(EASE_STANDARD, CubicBezier::new(0.4, 0.0, 0.2, 1.0));
}

#[test]
fn gspin_pulse_shape() {
    assert_close(gspin_opacity(0.0, 0.1), 1.0, 1e-6, "cycle start");
    assert_close(gspin_opacity(0.45, 0.1), 0.1, 1e-6, "fully dim");
    assert_close(gspin_opacity(0.9, 0.1), 0.1, 1e-6, "rest band");
    assert_close(gspin_opacity(1.0, 0.1), 1.0, 1e-6, "wraps to full");
    let mid_fall = gspin_opacity(0.2, 0.1);
    assert!(mid_fall > 0.1 && mid_fall < 1.0, "eases down");
    let mid_rise = gspin_opacity(0.96, 0.1);
    assert!(mid_rise > 0.1 && mid_rise < 1.0, "eases up");
}

/// The list is the whole setting, so a speed that is not offered, or two rows
/// that read the same, is a defect. The match is exhaustive on purpose: adding a
/// variant without deciding what it says here will not compile.
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
            (MIN_SCALE..=MAX_SCALE).contains(&pref.scale()),
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
/// asks about reduced motion and takes its end state, so nothing is drawn
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

/// The choice has to reach the durations that are drawn. Every spec in the
/// catalog is multiplied by `speed_scale`, so this is where the setting is
/// checked without a window.
#[test]
fn a_motion_preference_changes_what_a_transition_costs() {
    let base = FADE_IN.total();
    for pref in MotionPreference::ALL {
        set_speed_scale(pref.scale());
        assert_close(speed_scale(), pref.scale(), 1e-6, pref.label());
        assert_eq!(
            FADE_IN.total().mul_f32(speed_scale()),
            base.mul_f32(pref.scale()),
            "{} duration",
            pref.label()
        );
    }
    set_speed_scale(1.0);
    assert_eq!(speed_scale(), 1.0);
}

/// The multiplier reaches every duration in the app, so a value that would
/// break all of them is refused rather than stored: zero finishes a transition
/// before it draws, and a NaN leaves one running for good.
#[test]
fn the_speed_scale_refuses_values_that_would_break_every_duration() {
    set_speed_scale(0.0);
    assert_eq!(speed_scale(), MIN_SCALE);
    set_speed_scale(1.0e9);
    assert_eq!(speed_scale(), MAX_SCALE);
    set_speed_scale(f32::NAN);
    assert_eq!(speed_scale(), MAX_SCALE, "a NaN is refused, not stored");
    set_speed_scale(f32::INFINITY);
    assert_eq!(speed_scale(), MAX_SCALE, "infinity is refused, not stored");
    set_speed_scale(1.0);
    assert_eq!(speed_scale(), 1.0);
}
