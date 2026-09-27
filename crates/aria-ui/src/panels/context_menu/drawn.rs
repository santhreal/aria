//! Drawn: the menu the tables in [`crate::surfaces::context_menu`] describe.
//!
//! The target picks a table, the facts gathered when the menu opened pick the
//! rows, and one builder turns a row into an element. Nothing here decides what
//! a row does, which is what lets `app::drawn_census` press every row.

use super::label::{label_groups, label_row};
use super::row::item;
use super::*;
use crate::surfaces::context_menu::{
    CLIP_ITEMS, ClipMenu, MenuItem, POOL_ITEMS, PREVIEW_ITEMS, RECENT_ITEMS, TRACK_ITEMS,
    TRANSITION_ITEMS, apply_blend,
};

/// The target to draw: the open one, or the one still fading out.
pub(super) fn drawn_target(
    open: Option<ContextTarget>,
    exiting: Option<f32>,
    cx: &gpui::App,
) -> Option<ContextTarget> {
    if let Some(target) = open {
        LAST_TARGET.with(|slot| *slot.borrow_mut() = Some(target.clone()));
        return Some(target);
    }
    // An exit needs frames to spend and reduced motion schedules none, so the
    // fade would hang on the screen at full opacity instead of finishing.
    if exiting.is_none() || cx.reduce_motion() {
        LAST_TARGET.with(|slot| slot.borrow_mut().take());
        return None;
    }
    LAST_TARGET.with(|slot| slot.borrow().clone())
}

/// What a [`MenuItem::Labels`] block draws: whose label it writes, the label
/// already on it, and the groups to offer beside it.
struct Swatches<'a> {
    target: LabelTarget,
    current: Option<aria_models::MarkerColor>,
    groups: &'a [(aria_models::MarkerColor, usize)],
}

/// What a [`MenuItem::Blends`] block draws: the junction, and which blend is
/// already on it so the catalog can mark that row.
struct Blends {
    junction: ClipMenu,
    applied: Option<String>,
}

/// Build one table into elements.
///
/// The facts are copied into each row's listener, so a click acts on the menu the
/// operator opened rather than on whatever the session holds by the time they
/// reach for it. An engine refusal is reported here, once: forty rows cannot
/// disagree about whether a refused edit is announced.
fn drawn<F: Clone + 'static>(
    items: &'static [MenuItem<F>],
    facts: F,
    swatches: Option<Swatches<'_>>,
    blends: Option<Blends>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let mut out = Vec::with_capacity(items.len());
    for entry in items {
        match entry {
            // A row that leaves the editor draws exactly like one that does not:
            // the distinction is the census's, not the operator's.
            MenuItem::Row(row) | MenuItem::Outside(row) => {
                if !(row.when)(&facts) {
                    continue;
                }
                let label = (row.label)(&facts);
                let run = row.run;
                let facts = facts.clone();
                out.push(item(row.id, label, theme, cx, move |state| {
                    if let Err(err) = run(state, &facts) {
                        state.report_error(&err);
                    }
                }));
            }
            MenuItem::Labels => {
                let Some(swatches) = swatches.as_ref() else {
                    continue;
                };
                out.push(label_row(swatches.target, swatches.current, theme, cx));
                if let Some(groups) = label_groups(swatches.groups, theme, cx) {
                    out.push(groups);
                }
            }
            MenuItem::Blends => {
                let Some(blends) = blends.as_ref() else {
                    continue;
                };
                // The catalog is the single owner of what is offerable, so a new
                // blend appears here without a second list. The applied one takes
                // the same `• ` prefix the inspector and the sidebar use.
                let applied = blends.applied.as_deref();
                for spec in crate::app::clip_transitions::transition_choices() {
                    let id = spec.id.clone();
                    let label = if applied == Some(spec.id.as_str()) {
                        format!("• {}", spec.label)
                    } else {
                        spec.label.clone()
                    };
                    let junction = blends.junction;
                    out.push(item(
                        crate::motion::MotionKey::new("ctx-blend", &spec.id),
                        label,
                        theme,
                        cx,
                        move |state: &mut AppState| {
                            if let Err(err) = apply_blend(state, &junction, &id) {
                                state.report_error(&err);
                            }
                        },
                    ));
                }
            }
        }
    }
    out
}

pub(super) fn items(
    target: ContextTarget,
    facts: MenuFacts,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    match target {
        ContextTarget::Clip { .. } => {
            let Some(clip) = facts.clip else {
                return Vec::new();
            };
            drawn(
                CLIP_ITEMS,
                clip,
                Some(Swatches {
                    target: LabelTarget::Selection,
                    current: facts.label,
                    groups: &facts.label_groups,
                }),
                None,
                theme,
                cx,
            )
        }
        ContextTarget::Track { .. } => {
            let Some(track) = facts.track else {
                return Vec::new();
            };
            drawn(TRACK_ITEMS, track, None, None, theme, cx)
        }
        ContextTarget::Transition { .. } => {
            let Some(junction) = facts.clip else {
                return Vec::new();
            };
            drawn(
                TRANSITION_ITEMS,
                junction,
                None,
                Some(Blends {
                    junction,
                    applied: facts.transition_id,
                }),
                theme,
                cx,
            )
        }
        ContextTarget::Preview => drawn(PREVIEW_ITEMS, (), None, None, theme, cx),
        ContextTarget::PoolItem { media } => {
            let Some(pool) = facts.pool else {
                return Vec::new();
            };
            drawn(
                POOL_ITEMS,
                pool,
                Some(Swatches {
                    target: LabelTarget::Media(media),
                    current: facts.pool_label,
                    groups: &[],
                }),
                None,
                theme,
                cx,
            )
        }
        ContextTarget::Recent { path } => drawn(RECENT_ITEMS, path, None, None, theme, cx),
    }
}
