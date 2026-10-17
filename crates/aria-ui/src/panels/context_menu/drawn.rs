//! Drawn: the menu the tables in [`crate::surfaces::context_menu`] describe.
//!
//! The target picks a table, the facts gathered when the menu opened pick the
//! rows, and one builder turns a row into an element. Nothing here decides what
//! a row does, which is what lets `app::drawn_census` press every row.

use super::label::{label_groups, label_row};
use super::row::{item, item_with_icon};
use super::*;
use crate::surfaces::context_menu::{
    BADGE_ITEMS, BadgeMenu, CLIP_ITEMS, ClipMenu, EFFECT_ITEMS, MenuItem, POOL_ITEMS,
    PREVIEW_ITEMS, RECENT_ITEMS, TRACK_ITEMS, TRANSITION_ITEMS, apply_blend, draw_property,
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
    current: Option<aria_models::LabelColor>,
    groups: &'a [(aria_models::LabelColor, usize)],
}

/// What a [`MenuItem::Blends`] block draws: the junction, which blend is
/// already on it so the catalog can mark that row, and which kind it takes.
struct Blends {
    junction: ClipMenu,
    applied: Option<String>,
    media: Option<aria_models::TransitionMedia>,
}

/// The blocks a table may hold besides its rows, each drawn only when the menu
/// gathered the facts it reads.
#[derive(Default)]
struct Blocks<'a> {
    swatches: Option<Swatches<'a>>,
    blends: Option<Blends>,
    /// What a [`MenuItem::Properties`] block lists: the clip's properties, the
    /// drawn one marked.
    properties: Option<BadgeMenu>,
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
    blocks: Blocks<'_>,
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
                let Some(swatches) = blocks.swatches.as_ref() else {
                    continue;
                };
                out.push(label_row(swatches.target, swatches.current, theme, cx));
                if let Some(groups) = label_groups(swatches.groups, theme, cx) {
                    out.push(groups);
                }
            }
            MenuItem::Blends => {
                let Some(blends) = blocks.blends.as_ref() else {
                    continue;
                };
                // The catalog is the single owner of what is offerable, so a new
                // blend appears here without a second list; a cut between sounds
                // lists the crossfades instead. The applied one takes the same
                // `• ` prefix the inspector and the sidebar use.
                let applied = blends.applied.as_deref();
                let junction = blends.junction;
                let mut blend = |catalog_id: &str, name: &str| {
                    let label = if applied == Some(catalog_id) {
                        format!("• {name}")
                    } else {
                        name.to_owned()
                    };
                    let picked = catalog_id.to_owned();
                    out.push(item(
                        crate::motion::MotionKey::new("ctx-blend", catalog_id),
                        label,
                        theme,
                        cx,
                        move |state: &mut AppState| {
                            if let Err(err) = apply_blend(state, &junction, &picked) {
                                state.report_error(&err);
                            }
                        },
                    ));
                };
                match blends.media {
                    Some(aria_models::TransitionMedia::Audio) => {
                        for choice in crate::catalog_text::audio_transitions() {
                            blend(&choice.id, &choice.label);
                        }
                    }
                    _ => {
                        for spec in crate::app::clip_transitions::transition_choices() {
                            blend(&spec.id, &spec.label);
                        }
                    }
                }
            }
            MenuItem::Properties => {
                let Some(menu) = blocks.properties.as_ref() else {
                    continue;
                };
                // The drawn property takes the `• ` prefix the blend rows use.
                for property in menu.properties.iter() {
                    let label = if menu.drawn == Some(property.param) {
                        format!("• {}", property.label).into()
                    } else {
                        property.label.clone()
                    };
                    let param = property.param;
                    let menu = menu.clone();
                    out.push(item(
                        crate::motion::MotionKey::new(
                            "ctx-badge-property",
                            format_args!("{param:?}"),
                        ),
                        label,
                        theme,
                        cx,
                        move |state: &mut AppState| {
                            if let Err(err) = draw_property(state, &menu, param) {
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
                Blocks {
                    swatches: Some(Swatches {
                        target: LabelTarget::Selection,
                        current: facts.label,
                        groups: &facts.label_groups,
                    }),
                    ..Blocks::default()
                },
                theme,
                cx,
            )
        }
        ContextTarget::Track { .. } => {
            let Some(track) = facts.track else {
                return Vec::new();
            };
            drawn(TRACK_ITEMS, track, Blocks::default(), theme, cx)
        }
        ContextTarget::Transition { .. } => {
            let Some(junction) = facts.clip else {
                return Vec::new();
            };
            drawn(
                TRANSITION_ITEMS,
                junction,
                Blocks {
                    blends: Some(Blends {
                        junction,
                        applied: facts.transition_id,
                        media: facts.transition_media,
                    }),
                    ..Blocks::default()
                },
                theme,
                cx,
            )
        }
        ContextTarget::Preview => drawn(PREVIEW_ITEMS, (), Blocks::default(), theme, cx),
        ContextTarget::PoolItem { media } => {
            let Some(pool) = facts.pool else {
                return Vec::new();
            };
            drawn(
                POOL_ITEMS,
                pool,
                Blocks {
                    swatches: Some(Swatches {
                        target: LabelTarget::Media(media),
                        current: facts.pool_label,
                        groups: &[],
                    }),
                    ..Blocks::default()
                },
                theme,
                cx,
            )
        }
        ContextTarget::Recent { path } => drawn(RECENT_ITEMS, path, Blocks::default(), theme, cx),
        ContextTarget::Effect { .. } => {
            let Some(effect) = facts.effect else {
                return Vec::new();
            };
            drawn(EFFECT_ITEMS, effect, Blocks::default(), theme, cx)
        }
        ContextTarget::ClipBadge { .. } => {
            let Some(badge) = facts.badge else {
                return Vec::new();
            };
            drawn(
                BADGE_ITEMS,
                badge.clone(),
                Blocks {
                    properties: Some(badge),
                    ..Blocks::default()
                },
                theme,
                cx,
            )
        }
    }
}

/// The rows plugins added, below a rule, in mount order. A click runs a placed
/// command on the window's thread and a handler's row on a thread of its own.
pub(super) fn plugin_items(
    entries: &[crate::app::ContextPluginRow],
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    if entries.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(entries.len() + 1);
    out.push(
        div()
            .h(px(1.))
            .my(px(Theme::SPACE_XS))
            .bg(theme.border)
            .into_any_element(),
    );
    for (index, entry) in entries.iter().enumerate() {
        let row = entry.clone();
        out.push(item_with_icon(
            crate::motion::MotionKey::new("ctx-plugin", index),
            entry.label().to_owned(),
            entry.icon(),
            theme,
            cx,
            move |state: &mut AppState| state.run_context_plugin_row(&row),
        ));
    }
    out
}
