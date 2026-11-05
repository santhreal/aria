//! A mixer strip's head draws the app's controls, each starting in the
//! strip's column.
//!
//! Two classes are closed here, over the Audio workspace drawn with an audio
//! lane added and the drawn controls recorded:
//!
//! - A panel drawing a chip of its own beside [`crate::components::chip`], as
//!   the mixer did with a boxed chip filled in the selection or the warning
//!   color, so its rows of choices read as another app's. Every chip each
//!   strip's facts offer, on every lane strip the mixer draws and the master
//!   strip, must be recorded as a compact chip under the identity the strip
//!   draws it with: its mute, a lane's solo, its automation mode list chip, and
//!   each output a lane may point at. While a strip's automation list is open,
//!   every automation mode row is drawn below the chip. A picture lane with no
//!   sound has no strip.
//! - A control in a strip's head standing off the column the strip's labels
//!   start in, or the automation control wrapping onto a second line. The
//!   insert chain lays its rows out as the Effect Controls rows it shares, its
//!   controls a few pixels in from the row's edge, and drew its Add button that
//!   far right of the chips above and below it. On the lane strip and the
//!   master strip, the automation chip, the first output chip and the Add
//!   button must each start in the strip's column, and the automation chip must
//!   stand one compact chip tall with no second automation row.
//!
//! Not covered: the warning tone of a lit mute, a writing automation mode and a
//! pre-fader send, which the record does not hold; a send chip, whose label
//! states the send's level; and a bus strip, which draws its head through the
//! same rows as a lane strip.

use aria_models::{AutomationMode, TrackKind};
use gpui::{Bounds, Modifiers, Pixels, SharedString, TestAppContext, VisualTestContext, px};

use crate::app::mixer::MixerStrip;
use crate::components::noted::{
    Drawn, DrawnControl, NotingControls, control_selector, drawn_controls, record_controls,
};
use crate::components::{ChipSize, GHOST_INSET};
use crate::interned::intern;
use crate::motion::{ControlId, MotionKey};
use crate::test_support::drawn_shell;
use crate::theme::Theme;

const EPS: f32 = 0.51;

/// The compact chip labeled `label` drawn as `id`: the selector it draws under
/// and what it draws, its inset and its text size. A chip of another size
/// draws under the same selector, so the two are told apart by what they draw.
fn compact(id: MotionKey, label: &str) -> DrawnControl {
    let drawn = Drawn::Labelled {
        label: SharedString::from(label.to_string()),
        inset: ChipSize::Compact.pad().0,
        size: Theme::TEXT_SMALL,
    };
    DrawnControl {
        selector: control_selector("chip", ControlId::Key(id), &drawn),
        drawn,
    }
}

/// The selector of the Add button of the insert chain keyed `base`.
fn add_button(base: MotionKey) -> String {
    let drawn = Drawn::Labelled {
        label: SharedString::new_static("+ Audio effect"),
        inset: GHOST_INSET.0,
        size: Theme::TEXT_UI,
    };
    control_selector(
        "ghost",
        ControlId::Key(base.with("add").with("insert-add")),
        &drawn,
    )
}

/// The chips every strip keyed `raw` draws at rest whatever its routing: its
/// mute and its automation mode list chip.
fn every_strip(raw: u64) -> [DrawnControl; 2] {
    [
        compact(MotionKey::new("mixer-mute", raw), "M"),
        compact(
            MotionKey::new("mixer-auto", raw),
            AutomationMode::Off.label(),
        ),
    ]
}

/// The Audio workspace drawn with an audio lane added, while the controls it
/// builds are recorded.
struct Mixer<'a> {
    visual: &'a mut VisualTestContext,
    /// The lane and bus strips the mixer draws: every strip in the mix but a
    /// soundless picture lane's.
    strips: Vec<MixerStrip>,
    /// The added lane's key.
    lane: u64,
    /// The controls the frame built.
    built: Vec<DrawnControl>,
    /// Keeps the controls' debug selectors on every frame drawn after.
    _noting: NotingControls,
}

fn drawn_mixer<'a>(cx: &'a mut TestAppContext, tag: &str) -> Mixer<'a> {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, tag);
    visual.update(|_, app| app.set_reduce_motion(true));
    let lane = visual.update(|_, app| {
        state.update(app, |state, cx| {
            assert!(state.apply_workspace("win-workspace-audio", cx));
            state
                .add_lane_of_kind(TrackKind::Audio, None)
                .unwrap_or_else(|err| panic!("an audio lane: {err:?}"))
        })
    });
    visual.run_until_parked();
    let strips = visual.update(|_, app| {
        let state = state.read(app);
        let soundless = state.soundless_picture_lanes();
        state
            .mixer_strips()
            .unwrap_or_else(|err| panic!("the mix folds: {err}"))
            .iter()
            .filter(|strip| !soundless.contains(&strip.track))
            .cloned()
            .collect::<Vec<_>>()
    });
    assert!(
        strips.iter().any(|strip| strip.track == lane.raw()),
        "the added audio lane has a strip, so a lane strip's head is measured"
    );
    let noting = NotingControls::start();
    record_controls();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let (built, _) = drawn_controls();
    Mixer {
        visual,
        strips,
        lane: lane.raw(),
        built,
        _noting: noting,
    }
}

/// Where the last frame drew `selector`.
fn drawn(visual: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    visual
        .debug_bounds(intern(selector))
        .unwrap_or_else(|| panic!("the mixer draws no {selector}"))
}

#[gpui::test]
fn every_mixer_chip_is_the_apps_compact_chip(cx: &mut TestAppContext) {
    let mixer = drawn_mixer(cx, "mixer-chips");
    let mut expected: Vec<DrawnControl> = every_strip(0).into_iter().collect();
    for strip in mixer.strips.iter() {
        expected.extend(every_strip(strip.track));
        expected.push(compact(MotionKey::new("mixer-solo", strip.track), "S"));
        expected.extend(strip.output_targets.iter().map(|target| {
            compact(
                MotionKey::new("mixer-out", strip.track).with(&target.name),
                &target.name,
            )
        }));
    }
    let missing: Vec<&String> = expected
        .iter()
        .filter(|chip| !mixer.built.contains(chip))
        .map(|chip| &chip.selector)
        .collect();
    assert!(
        missing.is_empty(),
        "chips the mixer offers that were not drawn as the app's compact chip: {missing:?}"
    );
    for raw in std::iter::once(0).chain(mixer.strips.iter().map(|strip| strip.track)) {
        for mode in AutomationMode::ALL {
            let old = compact(
                MotionKey::new("mixer-auto", raw).with(mode.label()),
                mode.label(),
            );
            assert!(
                !mixer.built.contains(&old),
                "strip {raw} drew a separate chip for automation mode {}: {}",
                mode.label(),
                old.selector
            );
            let row = crate::pick_list::row_selector(
                MotionKey::new("mixer-auto", raw).with(mode.label()),
                mode.label(),
            );
            assert!(
                mixer.visual.debug_bounds(intern(&row)).is_none(),
                "strip {raw} drew automation row {row} while its list was closed"
            );
        }
    }
    for raw in [0, mixer.lane] {
        let chip_sel = compact(
            MotionKey::new("mixer-auto", raw),
            AutomationMode::Off.label(),
        );
        let at = drawn(mixer.visual, &chip_sel.selector).center();
        record_controls();
        mixer.visual.simulate_click(at, Modifiers::none());
        mixer.visual.run_until_parked();
        let (built, _) = drawn_controls();
        assert!(
            built.contains(&chip_sel),
            "strip {raw} did not draw its automation chip while its list was open"
        );
        for mode in AutomationMode::ALL {
            let row = crate::pick_list::row_selector(
                MotionKey::new("mixer-auto", raw).with(mode.label()),
                mode.label(),
            );
            assert!(
                mixer.visual.debug_bounds(intern(&row)).is_some(),
                "strip {raw} did not draw automation row {row} while its list was open"
            );
        }
        mixer.visual.simulate_click(at, Modifiers::none());
        mixer.visual.run_until_parked();
    }
}

#[gpui::test]
fn every_head_control_starts_in_the_strips_column(cx: &mut TestAppContext) {
    let mixer = drawn_mixer(cx, "mixer-column");
    let (visual, lane) = (mixer.visual, mixer.lane);
    let lane_strip = mixer
        .strips
        .iter()
        .find(|strip| strip.track == lane)
        .unwrap_or_else(|| panic!("no strip for lane {lane}"));
    let first_output = lane_strip
        .output_targets
        .first()
        .unwrap_or_else(|| panic!("lane {lane} offers no output"));
    let off = AutomationMode::Off.label();
    let heads = [
        (
            0,
            vec![
                compact(MotionKey::new("mixer-auto", 0), off).selector,
                add_button(MotionKey::new("insert-master", 0)),
            ],
        ),
        (
            lane,
            vec![
                compact(MotionKey::new("mixer-auto", lane), off).selector,
                add_button(MotionKey::new("insert-lane", lane)),
                compact(
                    MotionKey::new("mixer-out", lane).with(&first_output.name),
                    &first_output.name,
                )
                .selector,
            ],
        ),
    ];
    let mut wrong = Vec::new();
    for (raw, controls) in heads {
        let head = drawn(visual, &format!("mixer-head-{raw}"));
        // The head's one-pixel side border, then its side padding.
        let column = f32::from(head.left()) + 1.0 + Theme::SPACE_SM;
        for selector in controls {
            let left = f32::from(drawn(visual, &selector).left());
            if (left - column).abs() > EPS {
                wrong.push(format!(
                    "{selector} starts at {left} px, and strip {raw}'s column at {column} px"
                ));
            }
        }
    }
    for raw in std::iter::once(0).chain(mixer.strips.iter().map(|strip| strip.track)) {
        let mute = drawn(
            visual,
            &compact(MotionKey::new("mixer-mute", raw), "M").selector,
        );
        let auto = drawn(
            visual,
            &compact(MotionKey::new("mixer-auto", raw), off).selector,
        );
        if (auto.size.height - mute.size.height).abs() > px(EPS) {
            wrong.push(format!(
                "strip {raw}'s automation chip is {:?} tall, and its mute chip {:?}",
                auto.size.height, mute.size.height
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
