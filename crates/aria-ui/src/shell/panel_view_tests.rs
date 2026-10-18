//! A cached panel body draws what a full redraw draws.
//!
//! The class closed here is a docked panel that replays a stale drawing: a
//! change the panel draws reached the window, and the panel's cached body was
//! not notified, so the frame shows the panel as it was. Every panel the
//! registry holds is docked in turn, over a fresh project, and every change in
//! [`CHANGES`] is made through the route the app makes it: a narrow
//! [`Shell::redraw`] for a moved view or playhead, the pool tile route for a
//! tile hover, the filmstrip route for a landed cell, a state notification
//! for the rest. The frame drawn then
//! must equal the frame drawn after a refresh, which builds every body anew.
//! Every panel must also draw differently after at least one change, so a panel
//! added to the registry whose drawing no change here reaches fails until a
//! change it draws is added to [`CHANGES`].
//!
//! A frame is compared by its quads (bounds, clip, colours, corners, borders)
//! and its text runs (text, bounds, size). Not caught: a stale image or path,
//! and a panel that draws a change only through an image.

use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext};

use crate::app::AppState;
use crate::dock::{DockTree, FrameId, PANEL_PROJECT, PANEL_TIMELINE, Zone};
use crate::shell::{Moved, Shell};
use crate::test_support::{drawn_shell, wait_out_splash};

use super::{builds, unmoved};

/// The window the launch layout is built for.
const WINDOW_W: f32 = 1920.0;

/// A drawn frame, in an order that does not depend on how it was painted.
#[derive(Debug, PartialEq)]
struct Drawing {
    quads: Vec<(Bounds<Pixels>, String)>,
    runs: Vec<(Bounds<Pixels>, String)>,
}

impl Drawing {
    /// The part of the frame whose origin lies in `rect`.
    fn inside(&self, rect: Bounds<Pixels>) -> (Vec<&String>, Vec<&String>) {
        let within = |bounds: &Bounds<Pixels>| rect.contains(&bounds.origin);
        (
            self.quads
                .iter()
                .filter(|(b, _)| within(b))
                .map(|(_, q)| q)
                .collect(),
            self.runs
                .iter()
                .filter(|(b, _)| within(b))
                .map(|(_, r)| r)
                .collect(),
        )
    }

    /// Up to eight quads or runs drawn in one frame and not the other.
    fn differences(&self, full: &Drawing) -> String {
        let only = |a: &[(Bounds<Pixels>, String)], b: &[(Bounds<Pixels>, String)], tag: &str| {
            a.iter()
                .filter(|entry| !b.contains(entry))
                .map(|(_, what)| format!("  {tag} {what}"))
                .collect::<Vec<_>>()
        };
        let mut lines = only(&self.quads, &full.quads, "cached quad");
        lines.extend(only(&full.quads, &self.quads, "full quad"));
        lines.extend(only(&self.runs, &full.runs, "cached text"));
        lines.extend(only(&full.runs, &self.runs, "full text"));
        lines.truncate(8);
        lines.join("\n")
    }
}

fn drawing(visual: &mut VisualTestContext) -> Drawing {
    visual.update(|window, _| {
        let scale = window.scale_factor();
        let mut quads: Vec<(Bounds<Pixels>, String)> = window
            .painted_quads()
            .iter()
            .map(|quad| {
                let b = quad.bounds;
                let logical = Bounds::new(
                    gpui::point(
                        gpui::px(b.origin.x.0 / scale),
                        gpui::px(b.origin.y.0 / scale),
                    ),
                    gpui::size(
                        gpui::px(b.size.width.0 / scale),
                        gpui::px(b.size.height.0 / scale),
                    ),
                );
                let text = format!(
                    "{:?} {:?} {:?} {:?} {:?} {:?}",
                    quad.bounds,
                    quad.content_mask,
                    quad.background,
                    quad.border_color,
                    quad.corner_radii,
                    quad.border_widths
                );
                (logical, text)
            })
            .collect();
        quads.sort_by(|a, b| a.1.cmp(&b.1));
        let mut runs: Vec<(Bounds<Pixels>, String)> = window
            .rendered_text_runs()
            .iter()
            .map(|run| {
                (
                    run.bounds,
                    format!("{} {:?} {:?}", run.text, run.bounds, run.font_size),
                )
            })
            .collect();
        runs.sort_by(|a, b| a.1.cmp(&b.1));
        Drawing { quads, runs }
    })
}

/// A change to what a panel draws, made the way the app makes it.
#[derive(Clone, Copy, Debug)]
enum Change {
    /// A wheel notch: the timeline view pans and its lanes scroll.
    View,
    /// A scrub step on the ruler.
    Playhead,
    /// A click that selects the clip, or clears the selection.
    Select,
    /// A property of the selected clip nudged.
    Param,
    /// A bin created in the project.
    Bin,
    /// Snapping toggled.
    Setting,
    /// A marker dropped at a new playhead.
    Marker,
    /// An audio lane added.
    Lane,
    /// A block of sound reaching the output meter, as the meter ticker reads it.
    Meter,
    /// A job run to its end.
    Job,
    /// A title placed and selected, or the selection cleared.
    Title,
    /// A scope reading of the frame on screen arriving from the preview worker.
    Scopes,
    /// A letter typed in the effects search, or the search cleared.
    Search,
    /// The pointer scrubbing the still's pool tile, with no monitor open on it.
    PoolHover,
    /// The pointer scrubbing the still's pool tile with the monitor open on it,
    /// which moves the monitor too.
    MonitorScrub,
    /// A filmstrip cell landing for the timeline, as a finished render lands.
    Filmstrip,
}

const CHANGES: [Change; 16] = [
    Change::View,
    Change::Playhead,
    Change::Select,
    Change::Param,
    Change::Bin,
    Change::Setting,
    Change::Marker,
    Change::Lane,
    Change::Meter,
    Change::Job,
    Change::Title,
    Change::Scopes,
    Change::Search,
    Change::PoolHover,
    Change::MonitorScrub,
    Change::Filmstrip,
];

fn shell_of(visual: &mut VisualTestContext) -> gpui::Entity<Shell> {
    visual.update(|window, _| {
        window
            .root::<Shell>()
            .flatten()
            .expect("the window's root is the shell")
    })
}

pub(super) fn first_clip(state: &AppState) -> aria_models::ClipId {
    clip_where(state, |_| true).expect("a clip")
}

fn clip_where(
    state: &AppState,
    pick: impl Fn(&crate::types::UiClip) -> bool,
) -> Option<aria_models::ClipId> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| pick(clip))
        .map(|clip| aria_models::ClipId::from_raw(clip.id))
}

/// The first pool entry, or 0 over an empty pool.
fn first_media(state: &AppState) -> u64 {
    state.store.media.first().map_or(0, |item| item.id)
}

/// Waits for `job` to end: a row drawn while it runs would move under the
/// comparison.
fn settle(state: &AppState, job: aria_jobs::JobId) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !state
        .jobs
        .get(job)
        .is_some_and(|snapshot| snapshot.state.is_terminal())
    {
        assert!(std::time::Instant::now() < deadline, "the job never ended");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Makes `change` for the `step`th time.
fn make(visual: &mut VisualTestContext, shell: &gpui::Entity<Shell>, change: Change, step: usize) {
    if let Change::MonitorScrub = change {
        // The monitor opens on the still first and is drawn in full, so the
        // comparison sees what the hover alone moved.
        visual.update(|_, app| {
            let state = shell.read(app).state.clone();
            state.update(app, |state, cx| {
                let media = first_media(state);
                if state.sources.focused().map(|view| view.media) != Some(media) {
                    assert!(state.open_source(media), "the monitor opens on the still");
                }
                cx.notify();
            });
        });
        visual.run_until_parked();
    }
    visual.update(|_, app| {
        shell.update(app, |shell, cx| {
            let notify =
                |shell: &mut Shell, cx: &mut gpui::Context<Shell>, f: &dyn Fn(&mut AppState)| {
                    shell.state.update(cx, |state, cx| {
                        f(state);
                        cx.notify();
                    });
                };
            match change {
                Change::View => {
                    shell.state.update(cx, |state, _| {
                        state.zoom_bar_pan(if step % 2 == 0 { 0.02 } else { -0.01 });
                        state.video_scroll_y = (step % 3) as f32 * 8.0;
                    });
                    shell.redraw(Moved::View, cx);
                }
                Change::Playhead => {
                    shell.state.update(cx, |state, _| {
                        let to = 3 + (step as i64 * 5) % 40;
                        state.set_playhead(to);
                    });
                    shell.redraw(Moved::Playhead, cx);
                }
                Change::Select => notify(shell, cx, &|state| {
                    if state.selected_clip().is_none() {
                        let clip = first_clip(state);
                        state.select_clip(clip);
                    } else {
                        state.clear_selection();
                    }
                }),
                Change::Param => notify(shell, cx, &|state| {
                    let clip = first_clip(state);
                    state.select_clip(clip);
                    let delta = if step % 2 == 0 { -0.1 } else { 0.05 };
                    state
                        .nudge_param(aria_models::ClipParam::Opacity, None, delta)
                        .expect("nudge the opacity");
                }),
                Change::Bin => notify(shell, cx, &|state| {
                    state.create_bin().expect("create a bin");
                    state.cancel_bin_rename();
                }),
                Change::Setting => notify(shell, cx, &|state| state.toggle_snapping()),
                Change::Marker => notify(shell, cx, &|state| {
                    let to = 2 + (step as i64 * 7) % 40;
                    state.set_playhead(to);
                    state.add_marker_at_playhead().expect("drop a marker");
                }),
                Change::Lane => notify(shell, cx, &|state| {
                    state
                        .add_track(aria_models::TrackKind::Audio, format!("A{step}"))
                        .expect("add an audio lane");
                }),
                Change::Meter => notify(shell, cx, &|state| {
                    let level = if step % 2 == 0 { 0.9 } else { 0.3 };
                    state.audio.publish_block(&[level, level], 2);
                    state.meter_tick(std::time::Instant::now());
                }),
                Change::Job => notify(shell, cx, &|state| {
                    let job = state
                        .jobs
                        .spawn(format!("Job {step}"), |_| Ok("done".into()));
                    settle(state, job);
                    state.refresh_jobs();
                }),
                Change::Title => notify(shell, cx, &|state| {
                    let title =
                        |state: &AppState| clip_where(state, |clip| clip.generator.is_some());
                    match title(state) {
                        Some(clip) if state.selected_clip() == Some(clip) => {
                            state.clear_selection()
                        }
                        Some(clip) => state.select_clip(clip),
                        None => {
                            state
                                .add_generated(aria_models::Generator::text("Title"))
                                .expect("place a title");
                            let clip = title(state).expect("the title is on the timeline");
                            state.select_clip(clip);
                        }
                    }
                }),
                Change::Scopes => notify(shell, cx, &|state| {
                    let shade = (step * 37 % 256) as u8;
                    let pixels = [shade, 255 - shade, 90, 255].repeat(16 * 16);
                    let frame = aria_core::RgbaImage::new(16, 16, pixels);
                    let tick = *state.shown_tick.get_or_insert(0);
                    let reading = crate::app::scope_view::measure(
                        &frame,
                        tick,
                        state.waveform_mode().trace(),
                        &mut crate::app::scope_view::ScopeBuffers::default(),
                    );
                    state.adopt_scopes(reading);
                }),
                Change::Search => notify(shell, cx, &|state| {
                    if state.effects_browser.query.is_empty() {
                        state.effects_query_push('b');
                    } else {
                        state.clear_effects_query();
                    }
                }),
                Change::PoolHover | Change::MonitorScrub => shell.state.update(cx, |state, cx| {
                    let media = first_media(state);
                    let tick = if state.pool_tile_tick(media) == 3 {
                        6
                    } else {
                        3
                    };
                    state.hover_pool_tile(Some((media, tick)), cx);
                }),
                Change::Filmstrip => shell.state.update(cx, |state, cx| {
                    let revision = state.revision;
                    let tick = step as i64 * 8;
                    crate::filmstrips::land(
                        state,
                        revision,
                        tick,
                        Some(crate::filmstrips::test_cell()),
                        cx,
                    );
                }),
            }
        })
    });
    visual.run_until_parked();
}

/// A shell over a project with a still on its timeline, the boot cover gone
/// and motion reduced, so a frame holds only what the state draws.
pub(super) fn opened<'a>(
    cx: &'a mut TestAppContext,
    tag: &str,
) -> (gpui::Entity<AppState>, &'a mut VisualTestContext) {
    let (state, visual) = drawn_shell(cx, tag);
    visual.update(|_, app| app.set_reduce_motion(true));
    let still = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/texture.png");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let media = state.import_media(still).expect("import the still");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the still");
            cx.notify();
        });
    });
    wait_out_splash(visual);
    (state, visual)
}

/// Docks `panel` alone in the right frame of the editing layout and draws it
/// twice, the first draw recording the frame's rect. The rect, or `None` when
/// the right frame was not drawn.
pub(super) fn dock_right(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    panel: crate::dock::PanelId,
) -> Option<Bounds<Pixels>> {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.dock_tree = DockTree::editing_for_width(WINDOW_W);
            state.dock_tree.dock(panel, FrameId::RIGHT, Zone::Center);
            cx.notify();
        })
    });
    visual.run_until_parked();
    visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
    visual.run_until_parked();
    let [x, y, w, h] = visual.read(|app| {
        state
            .read(app)
            .dock_frame_rects
            .lock()
            .get(&FrameId::RIGHT)
            .copied()
    })?;
    Some(Bounds::new(
        gpui::point(gpui::px(x), gpui::px(y)),
        gpui::size(gpui::px(w), gpui::px(h)),
    ))
}

#[gpui::test]
fn every_cached_panel_body_draws_what_a_full_redraw_draws(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let panels = {
        let (state, visual) = opened(cx, "panel-view");
        let panels = visual.read(|app| state.read(app).registered_panel_ids());
        visual.update(|window, _| window.remove_window());
        panels
    };
    assert!(!panels.is_empty(), "the ui plugin registers panels");

    let mut stale = Vec::new();
    let mut unchanged = Vec::new();
    let mut step = 0;
    for (index, &panel) in panels.iter().enumerate() {
        // Each panel starts from the same project, so a list that grew under
        // the panels before it (History's steps, the Jobs rows) does not push
        // a change below the frame's edge.
        let (state, visual) = opened(cx, &format!("panel-view-{index}"));
        let shell = shell_of(visual);
        let Some(rect) = dock_right(&state, visual, panel) else {
            stale.push(format!("{panel}: the right frame was not drawn"));
            continue;
        };
        let mut changed = false;
        let mut before = drawing(visual);
        for change in CHANGES {
            step += 1;
            make(visual, &shell, change, step);
            let cached = drawing(visual);
            visual.update(|window, _| window.refresh());
            visual.run_until_parked();
            let full = drawing(visual);
            if cached != full {
                stale.push(format!(
                    "{panel} after {change:?}: the cached drawing differs from a full redraw\n{}",
                    cached.differences(&full)
                ));
            }
            changed |= before.inside(rect) != full.inside(rect);
            before = full;
        }
        if !changed {
            unchanged.push(panel);
        }
        visual.update(|window, _| window.remove_window());
    }
    assert!(stale.is_empty(), "stale drawings:\n{}", stale.join("\n"));
    assert!(
        unchanged.is_empty(),
        "no change in CHANGES alters the drawing of {unchanged:?}: add one each draws"
    );
}

#[gpui::test]
fn a_view_change_builds_only_the_bodies_that_draw_the_view(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "panel-view-scope");
    visual.update(|_, app| app.set_reduce_motion(true));
    wait_out_splash(visual);
    let shell = shell_of(visual);
    let docked = visual.read(|app| {
        let state = state.read(app);
        state
            .dock_tree
            .frames()
            .into_iter()
            .map(|frame| frame.active_panel())
            .collect::<Vec<_>>()
    });
    assert!(
        docked.contains(&PANEL_TIMELINE) && docked.contains(&PANEL_PROJECT),
        "the launch layout docks the Timeline and the Project panel"
    );
    assert!(
        docked.iter().any(|&panel| unmoved(panel, Moved::View)),
        "the launch layout docks no panel a view change leaves alone"
    );
    for moved in Moved::ALL {
        builds::take();
        make(
            visual,
            &shell,
            match moved {
                Moved::View => Change::View,
                Moved::Playhead => Change::Playhead,
                Moved::PoolHover => Change::PoolHover,
                Moved::Filmstrip => Change::Filmstrip,
            },
            1,
        );
        let mut built = builds::take();
        built.sort_unstable();
        built.dedup();
        let mut expected: Vec<_> = docked
            .iter()
            .copied()
            .filter(|&panel| !unmoved(panel, moved))
            .collect();
        expected.sort_unstable();
        assert_eq!(built, expected, "the bodies built for a {moved:?} change");
        // The panel that draws what moved is rebuilt, whatever the table lists.
        let drawn_in = match moved {
            Moved::View | Moved::Playhead | Moved::Filmstrip => PANEL_TIMELINE,
            Moved::PoolHover => PANEL_PROJECT,
        };
        assert!(
            built.contains(&drawn_in),
            "a {moved:?} change left {drawn_in} replaying what it drew before"
        );
    }
}
