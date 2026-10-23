//! Every context menu row stands on the menu bar's row grid: a lead column,
//! then the caption.
//!
//! The class closed here: a context row that spelled its state in its caption,
//! as the lane heights did with a `• ` prefix, or drew a check mark anywhere but
//! the lead column, so the caption of a checked row started at another x than
//! its neighbours' and the state read differently than in the menu bar.
//!
//! A timeline with a blended cut is opened in a drawn shell, and the menu of
//! every target it holds is opened over it. Every row the tables draw for the
//! target, the blend and property rows among them, must start its caption one
//! gap past the lead column; a checked row must draw a check mark centred in
//! that column on its own line, an unchecked row none, and the card no other.
//!
//! Not covered: the label swatch block, which is not a text row; plugin rows,
//! which `chrome::grid_tests` places in the menu bar's lead column; and a cut
//! between sounds, whose crossfade rows are built by the blend rows' builder.

use gpui::{Bounds, Pixels, TestAppContext, px};

use crate::app::AppState;
use crate::app::fixtures::{asset, blend_right_junction};
use crate::app::premiere_tests::place_media;
use crate::surfaces::context_menu::{
    BADGE_ITEMS, BadgeMenu, CLIP_ITEMS, ClipMenu, EFFECT_ITEMS, EffectMenu, MenuItem, POOL_ITEMS,
    PREVIEW_ITEMS, PoolMenu, RECENT_ITEMS, TRACK_ITEMS, TRANSITION_ITEMS, TrackMenu,
};
use crate::test_support::drawn_shell;
use crate::theme::Theme;
use crate::types::ContextTarget;

const EPS: f32 = 0.51;

#[gpui::test]
fn every_context_row_draws_its_check_mark_in_the_lead_column(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "ctx-grid");
    visual.update(|_, app| app.set_reduce_motion(true));
    let targets = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let targets = dress(state);
            cx.notify();
            targets
        })
    });
    visual.run_until_parked();
    // Text the panels draw with no menu open, which the card may lie over.
    let underneath = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let mut faults = Vec::new();
    let mut checked_by_menu = Vec::new();
    for target in targets {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.close_context();
                state.open_context(target.clone(), 240.0, 40.0);
                cx.notify();
            })
        });
        visual.run_until_parked();
        let rows = visual.read(|app| rows_of(state.read(app), &target));
        assert!(!rows.is_empty(), "the menu on {target:?} offers no rows");
        let card = visual
            .debug_bounds("context-menu-card")
            .unwrap_or_else(|| panic!("the menu on {target:?} drew no card"));
        let mut runs: Vec<(String, Bounds<Pixels>)> = visual.update(|window, _| {
            window
                .rendered_text_runs()
                .iter()
                .filter(|run| {
                    card.contains(&run.bounds.center())
                        && !underneath
                            .iter()
                            .any(|under| under.text == run.text && under.bounds == run.bounds)
                })
                .map(|run| (run.text.to_string(), run.bounds))
                .collect()
        });
        // A frosted card may paint its content twice at one place; one row
        // is one caption however often it is painted.
        runs.sort_by(|a, b| {
            a.1.top()
                .partial_cmp(&b.1.top())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        runs.dedup();
        // The lead column starts inside the card's 1 px border, its padding
        // and the row's padding.
        let column = card.left() + px(1.0 + Theme::SPACE_XS + Theme::SPACE_SM);
        let caption_x = column + px(Theme::ICON_SIZE + Theme::SPACE_SM);
        let centre = column + px(Theme::ICON_SIZE / 2.0);
        let checks: Vec<Bounds<Pixels>> = runs
            .iter()
            .filter(|(text, _)| text == "✓")
            .map(|(_, at)| *at)
            .collect();
        let mut checked = 0;
        for (index, (caption, is_checked)) in rows.iter().enumerate() {
            let nth = rows[..index].iter().filter(|(c, _)| c == caption).count();
            let Some(at) = runs
                .iter()
                .filter(|(text, _)| text == caption)
                .map(|(_, at)| *at)
                .nth(nth)
            else {
                faults.push(format!("{target:?}: `{caption}` drew no caption"));
                continue;
            };
            if (at.left() - caption_x).abs() > px(EPS) {
                faults.push(format!(
                    "{target:?}: `{caption}` starts at {:?}, the caption column at {caption_x:?}",
                    at.left()
                ));
            }
            let on_line = checks.iter().find(|check| {
                (check.center().y - at.center().y).abs() < px(Theme::ROW_HEIGHT / 2.0)
            });
            match (is_checked, on_line) {
                (true, Some(check)) => {
                    checked += 1;
                    if (check.center().x - centre).abs() > px(EPS) {
                        faults.push(format!(
                            "{target:?}: `{caption}`'s check is centred at {:?}, the column at {centre:?}",
                            check.center().x
                        ));
                    }
                }
                (true, None) => faults.push(format!("{target:?}: `{caption}` drew no check mark")),
                (false, Some(_)) => {
                    faults.push(format!(
                        "{target:?}: unchecked `{caption}` drew a check mark"
                    ));
                }
                (false, None) => {}
            }
        }
        if checks.len() != rows.iter().filter(|(_, is_checked)| *is_checked).count() {
            faults.push(format!(
                "{target:?}: the card drew {} check marks for {:?}",
                checks.len(),
                rows.iter()
                    .filter(|(_, is_checked)| *is_checked)
                    .map(|(caption, _)| caption)
                    .collect::<Vec<_>>()
            ));
        }
        checked_by_menu.push((target, checked));
    }
    assert!(
        faults.is_empty(),
        "context menu rows off the lead column grid:\n{}",
        faults.join("\n")
    );
    // Enable on the clip, the medium height on the lane, and the alignment and
    // the blend at the junction hold in this timeline, so each menu must have
    // drawn a check mark the assertions above were run against.
    for (target, checked) in &checked_by_menu {
        let holds = matches!(
            target,
            ContextTarget::Clip { .. }
                | ContextTarget::Track { .. }
                | ContextTarget::Transition { .. }
        );
        assert!(
            !holds || *checked > 0,
            "the menu on {target:?} drew no check mark to measure"
        );
    }
}

/// Dress `state` with a blended cut, the left clip selected and carrying an
/// effect, and return every target the timeline then offers a menu on.
fn dress(state: &mut AppState) -> Vec<ContextTarget> {
    let left = place_media(state, asset("texture.png"));
    blend_right_junction(state, left);
    state.select_clip(left);
    state
        .add_effect_to_selected("vignette")
        .expect("the clip takes an effect");
    let track = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|lane| lane.clips.iter().any(|clip| clip.id == left.raw()))
        .map(|lane| lane.id)
        .expect("the clip is on a lane");
    let media = state
        .store
        .media
        .first()
        .expect("the fixture imports one pool item")
        .id;
    let clip = left.raw();
    vec![
        ContextTarget::Clip { clip, track },
        ContextTarget::Transition { clip },
        ContextTarget::Track { track },
        ContextTarget::ClipBadge { clip, track },
        ContextTarget::Effect { clip, index: 0 },
        ContextTarget::PoolItem { media },
        ContextTarget::Preview,
        ContextTarget::Recent {
            path: asset("ctx-grid.aria"),
        },
    ]
}

/// The rows the menu on `target` draws, top to bottom: each caption, and
/// whether it is checked.
fn rows_of(state: &AppState, target: &ContextTarget) -> Vec<(String, bool)> {
    match target {
        ContextTarget::Clip { clip, .. } => table(CLIP_ITEMS, &ClipMenu::of(state, *clip), &[]),
        ContextTarget::Transition { clip } => {
            let junction = aria_models::ClipId::from_raw(*clip);
            assert!(
                !matches!(
                    state.clip_transition_media(junction),
                    Some(aria_models::TransitionMedia::Audio)
                ),
                "the fixture's junction is between pictures"
            );
            let applied = state.clip_transition_id(junction);
            let mut blends = Vec::new();
            for spec in crate::app::clip_transitions::transition_choices() {
                blends.push((
                    spec.label.to_string(),
                    applied.as_deref() == Some(&*spec.id),
                ));
            }
            table(TRANSITION_ITEMS, &ClipMenu::of(state, *clip), &blends)
        }
        ContextTarget::Track { track } => table(
            TRACK_ITEMS,
            &TrackMenu::of(state, *track).expect("the lane is projected"),
            &[],
        ),
        ContextTarget::ClipBadge { clip, .. } => {
            let badge = BadgeMenu::of(state, *clip).expect("the clip is projected");
            let properties: Vec<(String, bool)> = badge
                .properties
                .iter()
                .map(|property| {
                    (
                        property.label.to_string(),
                        badge.drawn == Some(property.param),
                    )
                })
                .collect();
            table(BADGE_ITEMS, &badge, &properties)
        }
        ContextTarget::Effect { clip, index } => {
            table(EFFECT_ITEMS, &EffectMenu::of(state, *clip, *index), &[])
        }
        ContextTarget::PoolItem { media } => table(POOL_ITEMS, &PoolMenu::of(state, *media), &[]),
        ContextTarget::Preview => table(PREVIEW_ITEMS, &(), &[]),
        ContextTarget::Recent { path } => table(RECENT_ITEMS, path, &[]),
    }
}

/// The rows `items` draws against `facts`, with `block` standing where the
/// table places its generated blend or property rows.
fn table<F>(
    items: &'static [MenuItem<F>],
    facts: &F,
    block: &[(String, bool)],
) -> Vec<(String, bool)> {
    let mut rows = Vec::new();
    for entry in items {
        match entry {
            MenuItem::Row(row) | MenuItem::Outside(row) => {
                if (row.when)(facts) {
                    rows.push(((row.label)(facts).to_owned(), row.is_checked(facts)));
                }
            }
            MenuItem::Labels => {}
            MenuItem::Blends | MenuItem::Properties => rows.extend_from_slice(block),
        }
    }
    rows
}
