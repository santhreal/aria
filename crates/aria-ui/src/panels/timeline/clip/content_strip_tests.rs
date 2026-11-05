//! What a generated clip's content strip draws: a borderless dot of the color
//! the clip fills with, and the generator's name only when the name strip above
//! it reads something else.
//!
//! WHY: a title named after its text, a shape named `Shape`, a solid named
//! `Solid` and a sticker named after its asset each drew that name twice, once
//! in the name strip and again beside the swatch under it, and the fill drew as
//! a 12 px rounded square with a dark border that read as an unchecked
//! checkbox. Closes, for each kind the Graphics and Effects lists place: a
//! content line that repeats the name strip, a content line dropped once the
//! clip is renamed, where it is the only line that states the kind, and a fill
//! drawn square, bordered or at another size. Not covered: the dot's color,
//! which the bar's dim and disabled inks scale, and generator kinds no list
//! places (adjustment, effect and filter clips).

use aria_commands::{Command, EditCommand};
use aria_models::{Generator, Shape};
use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext, point, px, size};

use super::SWATCH;
use crate::test_support::drawn_shell;

fn canvas(visual: &mut VisualTestContext) -> Bounds<Pixels> {
    visual
        .debug_bounds("timeline-video-canvas")
        .expect("the timeline draws its picture lanes")
}

/// How many text runs reading `text` the timeline's picture lanes draw.
fn drawn(visual: &mut VisualTestContext, text: &str) -> usize {
    let canvas = canvas(visual);
    visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .filter(|run| run.text.as_ref() == text && canvas.contains(&run.bounds.center()))
            .count()
    })
}

/// Every square quad from 6 to 14 px a side the picture lanes paint, with
/// whether it is round and whether it has a border. Keyframe dots are smaller.
fn swatches(visual: &mut VisualTestContext) -> Vec<(Bounds<Pixels>, bool, bool)> {
    let canvas = canvas(visual);
    visual.update(|window, _| {
        let scale = window.scale_factor();
        window
            .painted_quads()
            .iter()
            .filter_map(|quad| {
                let b = quad.bounds;
                let bounds = Bounds::new(
                    point(px(b.origin.x.0 / scale), px(b.origin.y.0 / scale)),
                    size(px(b.size.width.0 / scale), px(b.size.height.0 / scale)),
                );
                let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                let square = (w - h).abs() < 0.5 && (6.0..=14.0).contains(&w);
                (square && canvas.contains(&bounds.center())).then(|| {
                    let round = quad.corner_radii.top_left.0 / scale >= w * 0.5 - 0.5;
                    let border = quad.border_widths.left.0 > 0.0;
                    (bounds, round, border)
                })
            })
            .collect()
    })
}

#[gpui::test]
fn a_generated_clip_prints_its_kind_only_under_another_name(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "generator-content-strip");
    let sticker = crate::catalog_text::stickers()
        .first()
        .map(|choice| choice.id.to_string())
        .expect("a sticker ships");
    // Each kind, and whether it fills with a color.
    let kinds = [
        (Generator::text("Title"), true),
        (Generator::shape(Shape::Rectangle, [255, 64, 0, 255]), true),
        (
            Generator::SolidColor {
                rgba: [0, 128, 255, 255],
            },
            true,
        ),
        (Generator::sticker(sticker), false),
    ];
    for (generator, fills) in kinds {
        let (clip, name, label) = visual.update(|_, app| {
            state.update(app, |state, cx| {
                let (clip, _) = state
                    .place_generated(generator, None, 0)
                    .expect("the generator places");
                cx.notify();
                let bar = state.ui_clip_ref(clip).expect("the clip is projected");
                let label = bar
                    .generator
                    .as_ref()
                    .expect("a generated clip")
                    .label
                    .to_string();
                (clip, bar.name.to_string(), label)
            })
        });
        visual.run_until_parked();
        assert_eq!(name, label, "a new {label} clip is named after its kind");
        assert_eq!(
            drawn(visual, &label),
            1,
            "a {label} clip named {name:?} prints that name once, in its name strip"
        );
        let dots = swatches(visual);
        assert_eq!(
            dots.len(),
            usize::from(fills),
            "a {label} clip draws one fill dot if it fills with a color: {dots:?}"
        );
        for (bounds, round, border) in dots {
            assert!(
                (f32::from(bounds.size.width) - SWATCH).abs() < 0.5 && round && !border,
                "a {label} clip's fill draws {bounds:?} (round {round}, border {border}), \
                 not a borderless {SWATCH} px dot"
            );
        }

        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state
                    .apply_command(Command::Edit(EditCommand::SetClipName {
                        clip,
                        name: Some("Renamed".to_owned()),
                    }))
                    .expect("the clip renames");
                cx.notify();
            })
        });
        visual.run_until_parked();
        assert_eq!(
            (drawn(visual, "Renamed"), drawn(visual, &label)),
            (1, 1),
            "a renamed {label} clip prints its name, and its kind under it"
        );

        // The rename, then the placement and the lane it added.
        for _ in 0..2 {
            let undone = visual.update(|_, app| state.update(app, |state, _| state.undo()));
            assert!(undone, "the {label} clip's steps undo");
        }
        visual.run_until_parked();
    }
}
