//! A cached panel body draws what a full redraw draws.
//!
//! The class closed here is a docked panel that replays a stale drawing: a
//! change the panel draws reached the window, and the panel's cached body was
//! not notified, so the frame shows the panel as it was. Every panel the
//! registry holds is docked in turn, over a fresh project, and every change in
//! [`CHANGES`] is made through the route the app makes it: a narrow
//! [`Shell::redraw`] for a moved view or playhead, the pool tile route for a
//! tile hover, the filmstrip route for a landed cell, the frame route for a
//! landed Viewer frame or scope reading, the timeline canvas's pointer
//! listener for a held clip drag, a state notification for the rest.
//! The frame drawn then must equal the frame drawn after a refresh, which
//! builds every body anew. Every panel must also draw differently after at
//! least one change, so a panel added to the registry whose drawing no change
//! here reaches fails until a change it draws is added to [`CHANGES`].
//!
//! A frame is compared by its quads (bounds, clip, colours, corners, borders),
//! its text runs (text, bounds, size) and its images (bounds, clip, the atlas
//! tile sampled, opacity). Not caught: a stale path.

use gpui::{Bounds, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext, point, px};

use crate::app::AppState;
use crate::dock::{DockTree, FrameId, PANEL_PROJECT, PANEL_TIMELINE, PANEL_VIEWER, PanelId, Zone};
use crate::shell::{Moved, Shell};
use crate::test_support::{drawn_shell, wait_out_splash};
use crate::transport::TIMELINE_BASE_PPS;

use super::{builds, unmoved};

/// The window the launch layout is built for.
const WINDOW_W: f32 = 1920.0;

/// Whether a change that moved `moved` leaves `panel` replaying its last
/// drawing, with the state as it stands.
pub(super) fn left_alone(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    panel: PanelId,
    moved: Moved,
) -> bool {
    visual.read(|app| unmoved(panel, moved, state.read(app)))
}

/// A drawn frame, in an order that does not depend on how it was painted.
#[derive(Debug, PartialEq)]
pub(super) struct Drawing {
    quads: Vec<(Bounds<Pixels>, String)>,
    runs: Vec<(Bounds<Pixels>, String)>,
    images: Vec<(Bounds<Pixels>, String)>,
}

impl Drawing {
    /// The part of the frame whose origin lies in `rect`: its quads, its text
    /// runs and its images.
    fn inside(&self, rect: Bounds<Pixels>) -> [Vec<&String>; 3] {
        fn within(drawn: &[(Bounds<Pixels>, String)], rect: Bounds<Pixels>) -> Vec<&String> {
            drawn
                .iter()
                .filter(|(bounds, _)| rect.contains(&bounds.origin))
                .map(|(_, what)| what)
                .collect()
        }
        [
            within(&self.quads, rect),
            within(&self.runs, rect),
            within(&self.images, rect),
        ]
    }

    /// Up to eight quads, runs or images drawn in one frame and not the other.
    pub(super) fn differences(&self, full: &Drawing) -> String {
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
        lines.extend(only(&self.images, &full.images, "cached image"));
        lines.extend(only(&full.images, &self.images, "full image"));
        lines.truncate(8);
        lines.join("\n")
    }
}

pub(super) fn drawing(visual: &mut VisualTestContext) -> Drawing {
    visual.update(|window, _| {
        let scale = window.scale_factor();
        let logical = |b: Bounds<gpui::ScaledPixels>| {
            Bounds::new(
                gpui::point(
                    gpui::px(b.origin.x.0 / scale),
                    gpui::px(b.origin.y.0 / scale),
                ),
                gpui::size(
                    gpui::px(b.size.width.0 / scale),
                    gpui::px(b.size.height.0 / scale),
                ),
            )
        };
        let mut quads: Vec<(Bounds<Pixels>, String)> = window
            .painted_quads()
            .iter()
            .map(|quad| {
                let text = format!(
                    "{:?} {:?} {:?} {:?} {:?} {:?}",
                    quad.bounds,
                    quad.content_mask,
                    quad.background,
                    quad.border_color,
                    quad.corner_radii,
                    quad.border_widths
                );
                (logical(quad.bounds), text)
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
        let mut images: Vec<(Bounds<Pixels>, String)> = window
            .painted_images()
            .iter()
            .map(|image| {
                let text = format!(
                    "{:?} {:?} {:?} {:?} {:?}",
                    image.bounds,
                    image.content_mask,
                    image.tile.texture_id,
                    image.tile.tile_id,
                    image.opacity
                );
                (logical(image.bounds), text)
            })
            .collect();
        images.sort_by(|a, b| a.1.cmp(&b.1));
        Drawing {
            quads,
            runs,
            images,
        }
    })
}

/// A change to what a panel draws, made the way the app makes it.
#[derive(Clone, Copy, Debug)]
pub(super) enum Change {
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
    /// An effect starred under Favorites, or unstarred.
    Favorite,
    /// The effects search box taking the keyboard, or giving it back.
    SearchFocus,
    /// A preset saved from the first clip's effect, or deleted.
    Preset,
    /// The pointer scrubbing the still's pool tile, with no monitor open on it.
    PoolHover,
    /// The pointer scrubbing the still's pool tile with the monitor open on it,
    /// which moves the monitor too.
    MonitorScrub,
    /// A filmstrip cell landing for the timeline, as a finished render lands.
    Filmstrip,
    /// The frame at the playhead rendered again and landing in the Viewer.
    Frame,
    /// The pointer, no button held, onto the still's card: onto its left edge
    /// handle on an odd step and onto its middle on an even one, so the cursor
    /// the lanes set changes at each step.
    Pointer,
    /// The still's card carried 7 px further by a held clip drag, the drag
    /// opened by a press on the card when none is held.
    Drag,
}

const CHANGES: [Change; 22] = [
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
    Change::Favorite,
    Change::SearchFocus,
    Change::Preset,
    Change::Search,
    Change::PoolHover,
    Change::MonitorScrub,
    Change::Filmstrip,
    Change::Frame,
    Change::Pointer,
    Change::Drag,
];

/// The change that moves only `moved`, after making what it needs first, so
/// that the change made next with `step` is all a count after it sees: a
/// held drag's press redraws in full, and a pointer arriving on the lanes
/// leaves what it was over.
pub(super) fn moving(
    visual: &mut VisualTestContext,
    shell: &gpui::Entity<Shell>,
    moved: Moved,
    step: usize,
) -> Change {
    let change = match moved {
        Moved::View => Change::View,
        Moved::Playhead => Change::Playhead,
        Moved::PoolHover => Change::PoolHover,
        Moved::Filmstrip => Change::Filmstrip,
        Moved::Frame => Change::Frame,
        Moved::Pointer => Change::Pointer,
        Moved::Drag => Change::Drag,
    };
    match moved {
        Moved::Pointer => make(visual, shell, change, step + 1),
        Moved::Drag => make(visual, shell, change, step),
        Moved::View | Moved::Playhead | Moved::PoolHover | Moved::Filmstrip | Moved::Frame => {}
    }
    change
}

pub(super) fn shell_of(visual: &mut VisualTestContext) -> gpui::Entity<Shell> {
    visual.update(|window, _| {
        window
            .root::<Shell>()
            .flatten()
            .expect("the window's root is the shell")
    })
}

/// What each of `panels` keeps of its inputs, read from the state as it
/// stands. A panel that keeps nothing is rebuilt by every notification of the
/// state.
pub(super) fn stamps(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    panels: &[PanelId],
) -> Vec<Option<super::PanelStamp>> {
    visual.read(|app| {
        let reduced = app.reduce_motion();
        panels
            .iter()
            .map(|&panel| super::panel_stamp(panel, state.read(app), reduced))
            .collect()
    })
}

/// The panels of `panels` a notification of the state rebuilds, given what
/// they kept of their inputs before it: those that keep nothing, and those
/// whose inputs it moved.
pub(super) fn rebuilt(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    panels: &[PanelId],
    before: &[Option<super::PanelStamp>],
) -> Vec<PanelId> {
    let after = stamps(state, visual, panels);
    panels
        .iter()
        .zip(before.iter().zip(&after))
        .filter(|(_, (was, now))| was.is_none() || was != now)
        .map(|(&panel, _)| panel)
        .collect()
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
pub(super) fn make(
    visual: &mut VisualTestContext,
    shell: &gpui::Entity<Shell>,
    change: Change,
    step: usize,
) {
    match change {
        Change::Pointer => return hover_still(visual, shell, step),
        Change::Drag => return carry_still(visual, shell),
        _ => {}
    }
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
                        let by = if step.is_multiple_of(2) { 24.0 } else { -12.0 };
                        state.scroll_timeline_to(state.scroll_x + by, false);
                        state.video_scroll_y = (step % 3) as f32 * 8.0;
                    });
                    shell.redraw(Moved::View, cx);
                }
                Change::Playhead => {
                    shell.state.update(cx, |state, _| {
                        let to = 3 + (step as i64 * 5) % 40;
                        state.scrub_playhead_on_ruler(to);
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
                    let delta = if step.is_multiple_of(2) { -0.1 } else { 0.05 };
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
                    let level = if step.is_multiple_of(2) { 0.9 } else { 0.3 };
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
                Change::Scopes => shell.state.update(cx, |state, cx| {
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
                    // As the preview worker's mail lands a reading.
                    let mail = if state.adopt_scopes(reading) {
                        crate::app::Mail::Reading
                    } else {
                        crate::app::Mail::Nothing
                    };
                    state.land_mail(mail, cx);
                }),
                Change::Search => notify(shell, cx, &|state| {
                    if state.effects_browser.query.is_empty() {
                        state.effects_query_push('b');
                    } else {
                        state.clear_effects_query();
                    }
                }),
                Change::Favorite => notify(shell, cx, &|state| {
                    state.toggle_effect_favorite(
                        crate::app::effects_browser::EntryKind::VideoEffect,
                        "vignette",
                    );
                }),
                Change::SearchFocus => notify(shell, cx, &|state| {
                    let focused = state.effects_browser.focused;
                    state.focus_effects_search(!focused);
                }),
                Change::Preset => notify(shell, cx, &|state| {
                    if state.presets.is_empty() {
                        let clip = first_clip(state);
                        state.select_clip(clip);
                        state
                            .add_effect_to_selected("vignette")
                            .expect("add a vignette");
                        state.save_selected_preset("Look").expect("save a preset");
                        state
                            .remove_all_effects_selected()
                            .expect("strip the vignette");
                    } else {
                        state.delete_preset("Look").expect("delete the preset");
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
                Change::Frame => crate::app::spawn_preview(shell.state.clone(), cx),
                Change::Pointer | Change::Drag => {
                    unreachable!("`make` moves the pointer before this update")
                }
            }
        })
    });
    visual.run_until_parked();
}

/// Carries the still's card 7 px right with the left button held, through the
/// timeline canvas's own pointer listener, holding a move first with
/// [`hold_still`].
fn carry_still(visual: &mut VisualTestContext, shell: &gpui::Entity<Shell>) {
    hold_still(visual, shell);
    carry_by(visual, 7.0, Modifiers::none());
}

/// Holds a move of the still's card. With no drag held it presses the middle
/// of the part of the card the lanes show, clear of its edge handles in a
/// Timeline docked narrow, and moves 12 px, past the click slop, which opens
/// the drag.
pub(super) fn hold_still(visual: &mut VisualTestContext, shell: &gpui::Entity<Shell>) {
    let held = |visual: &mut VisualTestContext| {
        visual.read(|app| shell.read(app).state.read(app).held_move.is_some())
    };
    if !held(visual) {
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        let (canvas, card) = still_card(visual, shell);
        let lane = visual
            .debug_bounds("track-header-V1")
            .expect("the timeline draws V1's header");
        let shown = (
            card.0.max(f32::from(canvas.left())),
            card.1.min(f32::from(canvas.right())),
        );
        let grab = point(px((shown.0 + shown.1) / 2.0), lane.center().y);
        visual.simulate_mouse_move(grab, None, Modifiers::none());
        visual.simulate_mouse_down(grab, MouseButton::Left, Modifiers::none());
        visual.run_until_parked();
        visual.simulate_mouse_move(
            point(grab.x + px(12.0), grab.y),
            MouseButton::Left,
            Modifiers::none(),
        );
        visual.run_until_parked();
        assert!(
            held(visual),
            "a press at {grab:?} on the still's card, drawn across {card:?} in the lanes at \
             {canvas:?}, under V1's header at {lane:?}, and a 12 px move hold a move; \
             selected {:?}",
            visual.read(|app| shell.read(app).state.read(app).selected_clip())
        );
    }
}

/// Moves the pointer `dx` px along the lanes with the left button and
/// `modifiers` held.
pub(super) fn carry_by(visual: &mut VisualTestContext, dx: f32, modifiers: Modifiers) {
    let at = visual.update(|window, _| window.mouse_position());
    visual.simulate_mouse_move(point(at.x + px(dx), at.y), MouseButton::Left, modifiers);
    visual.run_until_parked();
}

/// Moves the pointer, no button held, onto the still's card on V1: 3 px into
/// its left edge handle on an odd `step`, and onto the middle of the part of
/// the card the lanes show on an even one.
pub(super) fn hover_still(
    visual: &mut VisualTestContext,
    shell: &gpui::Entity<Shell>,
    step: usize,
) {
    let (canvas, card) = still_card(visual, shell);
    let lane = visual
        .debug_bounds("track-header-V1")
        .expect("the timeline draws V1's header");
    let x = if step.is_multiple_of(2) {
        (card.0.max(f32::from(canvas.left())) + card.1.min(f32::from(canvas.right()))) / 2.0
    } else {
        card.0 + 3.0
    };
    visual.simulate_mouse_move(point(px(x), lane.center().y), None, Modifiers::none());
    visual.run_until_parked();
}

/// The video lanes' window bounds, and the window x span of the still's card
/// on them as the state places it.
pub(super) fn still_card(
    visual: &mut VisualTestContext,
    shell: &gpui::Entity<Shell>,
) -> (Bounds<Pixels>, (f32, f32)) {
    let canvas = visual
        .debug_bounds("timeline-video-canvas")
        .expect("the timeline draws its video lanes");
    let card = visual.read(|app| {
        let state = shell.read(app).state.read(app);
        // The still is the first clip of the lane badged V1; a title placed
        // by an earlier change stands on a lane of its own.
        let clip = state
            .store
            .sequence
            .tracks
            .iter()
            .enumerate()
            .find(|&(row, _)| state.lane_badge_at(row) == "V1")
            .and_then(|(_, track)| track.clips.first())
            .expect("the still is on V1");
        let tick_px = TIMELINE_BASE_PPS * state.zoom / state.store.sequence.ticks_per_second();
        let left = f32::from(canvas.origin.x) - state.scroll_x;
        (
            left + clip.timeline_start_ticks as f32 * tick_px,
            left + clip.timeline_end_ticks as f32 * tick_px,
        )
    });
    (canvas, card)
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
    let (state, visual) = opened(cx, "panel-view-scope");
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
        [PANEL_TIMELINE, PANEL_PROJECT, PANEL_VIEWER]
            .iter()
            .all(|panel| docked.contains(panel)),
        "the launch layout docks the Timeline, the Project panel and the Viewer"
    );
    assert!(
        docked
            .iter()
            .any(|&panel| left_alone(&state, visual, panel, Moved::View)),
        "the launch layout docks no panel a view change leaves alone"
    );
    for moved in Moved::ALL {
        let change = moving(visual, &shell, moved, 1);
        builds::take();
        make(visual, &shell, change, 1);
        let mut built = builds::take();
        built.sort_unstable();
        built.dedup();
        let mut expected: Vec<_> = docked
            .iter()
            .copied()
            .filter(|&panel| !left_alone(&state, visual, panel, moved))
            .collect();
        expected.sort_unstable();
        assert_eq!(built, expected, "the bodies built for a {moved:?} change");
        // The panel that draws what moved is rebuilt, whatever the table
        // lists. A playhead move is drawn by the shell's playhead layer over
        // the timeline body it replays, since no state in `INLINE_WHEN` holds
        // over a still on the timeline.
        let drawn_in = match moved {
            Moved::View | Moved::Filmstrip | Moved::Pointer | Moved::Drag => Some(PANEL_TIMELINE),
            Moved::Playhead => None,
            Moved::PoolHover => Some(PANEL_PROJECT),
            Moved::Frame => Some(PANEL_VIEWER),
        };
        match drawn_in {
            Some(drawn_in) => assert!(
                built.contains(&drawn_in),
                "a {moved:?} change left {drawn_in} replaying what it drew before"
            ),
            None => assert!(
                !built.contains(&PANEL_TIMELINE),
                "a playhead move over a timeline the playhead layer draws built the timeline \
                 body"
            ),
        }
    }

    // A scope reading of the frame on screen lands through the frame route.
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.toggle_scopes();
            cx.notify();
        })
    });
    visual.run_until_parked();
    builds::take();
    make(visual, &shell, Change::Scopes, 1);
    let mut built = builds::take();
    built.sort_unstable();
    built.dedup();
    let mut expected: Vec<_> = docked
        .iter()
        .copied()
        .filter(|&panel| !left_alone(&state, visual, panel, Moved::Frame))
        .collect();
    expected.sort_unstable();
    assert_eq!(built, expected, "the bodies built for a scope reading");
}

/// A frame that lands after an error clears the error, and the window draws it
/// cleared through the frame route alone: the landing makes no state
/// notification, and the status strip the shell draws and the Viewer's error
/// line both read the error. The shell's own drawing is also what ends the
/// boot cover on the session's first picture, which this route redraws the
/// same way. Fails when a landed frame leaves the error set, or when a part
/// of the window that draws the error replays its last drawing. Not caught: an
/// error drawn only as an image.
#[gpui::test]
fn a_frame_landing_after_an_error_draws_the_window_without_it(cx: &mut TestAppContext) {
    const ERROR: &str = "Decoding stopped at frame 12";
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "panel-view-error");
    let shell = shell_of(visual);
    let draws_error = |drawn: &Drawing| drawn.runs.iter().any(|(_, run)| run.contains(ERROR));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.report_error(ERROR);
            cx.notify();
        })
    });
    visual.run_until_parked();
    assert!(
        draws_error(&drawing(visual)),
        "the window draws the reported error"
    );

    make(visual, &shell, Change::Frame, 1);
    assert_eq!(
        visual.read(|app| state.read(app).last_error.clone()),
        None,
        "the landed frame clears the error"
    );
    let cached = drawing(visual);
    assert!(
        !draws_error(&cached),
        "the window still draws the error the landed frame cleared"
    );
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let full = drawing(visual);
    assert!(
        cached == full,
        "the landing drew a window that differs from a full redraw\n{}",
        cached.differences(&full)
    );
}

/// A playhead move that finds the project edited under it re-projects, and
/// the redraw after it builds what a notification of the state builds.
///
/// The class closed here is a narrow redraw after a poll that re-projected:
/// another writer of the engine, such as a plugin or a job, edits the project,
/// the next playhead move adopts the edit, and a redraw that built only the
/// bodies that draw the playhead leaves the Project panel replaying the pool
/// from before the edit. Not caught: an edit the projection does not read.
#[gpui::test]
fn a_playhead_move_that_re_projects_builds_what_a_notify_builds(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "panel-view-reproject");
    let shell = shell_of(visual);
    let mut docked = visual.read(|app| {
        let state = state.read(app);
        state
            .dock_tree
            .frames()
            .into_iter()
            .map(|frame| frame.active_panel())
            .collect::<Vec<_>>()
    });
    docked.sort_unstable();
    docked.dedup();
    let before = stamps(&state, visual, &docked);
    assert!(
        docked.iter().zip(&before).any(|(&panel, stamp)| {
            left_alone(&state, visual, panel, Moved::Playhead) && stamp.is_none()
        }),
        "the launch layout docks no panel a playhead move leaves alone and a notify rebuilds"
    );
    visual.update(|_, app| {
        state
            .read(app)
            .engine()
            .lock()
            .apply(aria_commands::Command::Edit(
                aria_commands::EditCommand::CreateBin {
                    name: "Edited elsewhere".into(),
                    parent: None,
                },
            ))
            .expect("create a bin on the engine");
    });
    builds::take();
    make(visual, &shell, Change::Playhead, 1);
    let mut built = builds::take();
    built.sort_unstable();
    built.dedup();
    assert_eq!(
        built,
        rebuilt(&state, visual, &docked, &before),
        "the bodies a re-projecting playhead move built"
    );
    let cached = drawing(visual);
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let full = drawing(visual);
    assert!(
        cached == full,
        "the move drew a window that differs from a full redraw\n{}",
        cached.differences(&full)
    );
}

#[gpui::test]
fn a_clip_edit_skips_panels_whose_inputs_did_not_change(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (_, visual) = opened(cx, "panel-view-edit-skip");
    let shell = shell_of(visual);
    for change in [Change::Param, Change::Marker] {
        builds::take();
        make(visual, &shell, change, 1);
        let built = builds::take();
        assert!(
            built.contains(&PANEL_TIMELINE),
            "a {change:?} change did not rebuild {PANEL_TIMELINE}"
        );
        for untouched in [crate::dock::PANEL_EFFECTS, crate::dock::PANEL_AUDIO_METERS] {
            assert!(
                !built.contains(&untouched),
                "a {change:?} change rebuilt {untouched}, whose inputs did not change: {built:?}"
            );
        }
    }
}
