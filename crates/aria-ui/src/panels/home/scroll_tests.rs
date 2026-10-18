//! Home's body scrolls, and its edges fade only toward more of the pane.
//!
//! The class closed here: a fade over an edge with nothing past it, which
//! cut the border and title of the recent cards at the top and bottom of a
//! list that fit, and a Home pane longer than the window with no way to its
//! end. A list that fits draws every card clear of every painted fade band.
//! The most recents the list holds, in the smallest window, scroll by the
//! wheel to the last card, with a band at the bottom only while the list
//! continues below and at the top only while it continues above. Every other
//! rail chosen from the end of that list opens its pane at the top.
//!
//! Not covered: the fade's pixels, which the test platform does not raster
//! on Linux.

use std::path::PathBuf;

use gpui::{
    Bounds, EdgeFade, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, point, px, size,
};

use crate::app::AppState;
use crate::app::recents::MAX_RECENTS;
use crate::settings::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, UiSettings};
use crate::test_support::drawn_shell_at;
use crate::types::HomeRail;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

/// Home on its Recents rail in a `width` x `height` window, listing `count`
/// projects that exist on disk.
fn home_with_recents(
    cx: &mut TestAppContext,
    tag: &str,
    (width, height): (f32, f32),
    count: usize,
) -> (gpui::Entity<AppState>, VisualTestContext) {
    cx.executor().allow_parking();
    let at = Bounds::new(point(px(0.), px(0.)), size(px(width), px(height)));
    let (state, mut visual) = drawn_shell_at(cx, tag, at);
    // The route and the cards glide in; reduced motion lands them at once.
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let dir: PathBuf = state.data_dir.join("projects");
            std::fs::create_dir_all(&dir)
                .unwrap_or_else(|err| panic!("a folder for the projects: {err}"));
            for n in 0..count {
                let path = dir.join(format!("project-{n}.aria"));
                std::fs::write(&path, b"")
                    .unwrap_or_else(|err| panic!("project {n} on disk: {err}"));
                state.remember_recent(&path);
            }
            state.go_home();
            state.set_home_rail(HomeRail::Recents);
            cx.notify();
        })
    });
    assert_eq!(
        visual.read(|app| state.read(app).recents().len()),
        count,
        "Home lists every project remembered"
    );
    (state, visual)
}

/// The fades of one full draw, after the follow-ups of the last change.
fn draw(visual: &mut VisualTestContext) -> Vec<EdgeFade> {
    visual.run_until_parked();
    crate::edge_fade::take_painted();
    visual.update(|window, _| window.refresh());
    crate::edge_fade::take_painted()
}

fn bounds(visual: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("Home drew no {selector}"))
}

fn card(visual: &mut VisualTestContext, index: usize) -> Bounds<Pixels> {
    bounds(visual, format!("recent-card-{index}").leak())
}

/// Turn the wheel down over the middle of `body`, further than any pane runs.
fn wheel_to_end(visual: &mut VisualTestContext, body: Bounds<Pixels>) {
    let position = body.center();
    visual.simulate_mouse_move(position, None, Modifiers::none());
    visual.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Lines(point(0.0, -400.0)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    visual.run_until_parked();
}

/// The strips a fade's bands cover, one per edge with a band.
fn bands(fade: &EdgeFade) -> Vec<(&'static str, Bounds<Pixels>)> {
    let Bounds {
        origin,
        size: whole,
    } = fade.bounds;
    let b = fade.bands;
    [
        ("top", origin, size(whole.width, b.top)),
        (
            "bottom",
            point(origin.x, origin.y + whole.height - b.bottom),
            size(whole.width, b.bottom),
        ),
        ("left", origin, size(b.left, whole.height)),
        (
            "right",
            point(origin.x + whole.width - b.right, origin.y),
            size(b.right, whole.height),
        ),
    ]
    .into_iter()
    .filter(|(_, _, extent)| f32::from(extent.width) > 0.0 && f32::from(extent.height) > 0.0)
    .map(|(edge, origin, extent)| (edge, Bounds::new(origin, extent)))
    .collect()
}

/// The top and bottom bands of the fade painted over Home's body.
fn body_bands(fades: &[EdgeFade], body: Bounds<Pixels>) -> (f32, f32) {
    fades
        .iter()
        .find(|fade| fade.bounds.intersects(&body))
        .map_or((0.0, 0.0), |fade| {
            (f32::from(fade.bands.top), f32::from(fade.bands.bottom))
        })
}

#[gpui::test]
fn a_recents_list_that_fits_draws_every_card_clear_of_every_fade(cx: &mut TestAppContext) {
    let opening = UiSettings::default().resolve_window_placement(&[[0.0, 0.0, 1920.0, 1080.0]]);
    for count in [1, 3] {
        let tag = format!("home-fit-{count}");
        let (_state, mut visual) =
            home_with_recents(cx, &tag, (opening.width, opening.height), count);
        let visual = &mut visual;
        let fades = draw(visual);
        let body = bounds(visual, "home-body");
        for index in 0..count {
            let card = card(visual, index);
            assert!(
                f32::from(card.bottom()) <= f32::from(body.bottom()) + EPS,
                "with {count} recents card {index} {card:?} ends past Home's body {body:?}, \
                 so the list does not fit and cannot tell a fade toward more from one over \
                 nothing"
            );
            for fade in &fades {
                for (edge, band) in bands(fade) {
                    assert!(
                        !band.intersects(&card),
                        "with {count} recents, which fit, the {edge} band {band:?} of a fade \
                         over {:?} covers card {index} {card:?}",
                        fade.bounds
                    );
                }
            }
        }
    }
}

#[gpui::test]
fn a_full_recents_list_scrolls_to_its_last_card_in_the_smallest_window(cx: &mut TestAppContext) {
    let (_state, mut visual) = home_with_recents(
        cx,
        "home-scroll",
        (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT),
        MAX_RECENTS,
    );
    let visual = &mut visual;
    let last = MAX_RECENTS - 1;

    let fades = draw(visual);
    let body = bounds(visual, "home-body");
    let resting = card(visual, last);
    assert!(
        f32::from(resting.bottom()) > f32::from(body.bottom()) + EPS,
        "the last of {MAX_RECENTS} recents {resting:?} fits in Home's body {body:?} at \
         {MIN_WINDOW_WIDTH} x {MIN_WINDOW_HEIGHT}, so this window cannot show the list \
         continuing"
    );
    let (top, bottom) = body_bands(&fades, body);
    assert!(
        top == 0.0 && bottom > 0.0,
        "at the top of a list that continues below, Home's body fades {top} at the top and \
         {bottom} at the bottom; it fades the bottom only"
    );

    wheel_to_end(visual, body);
    let fades = draw(visual);
    let scrolled = card(visual, last);
    assert!(
        f32::from(scrolled.bottom()) <= f32::from(body.bottom()) + EPS
            && f32::from(scrolled.top()) >= f32::from(body.top()) - EPS,
        "the wheel turned to the end leaves the last card {scrolled:?} outside Home's body \
         {body:?}"
    );
    let (top, bottom) = body_bands(&fades, body);
    assert!(
        top > 0.0 && bottom == 0.0,
        "at the end of a list that continues above, Home's body fades {top} at the top and \
         {bottom} at the bottom; it fades the top only"
    );
}

/// Every rail. The match stops the build when a rail is added, until the rail
/// is listed here.
fn every_rail() -> [HomeRail; 5] {
    let rails = [
        HomeRail::New,
        HomeRail::Open,
        HomeRail::Recents,
        HomeRail::Samples,
        HomeRail::Learn,
    ];
    for rail in rails {
        match rail {
            HomeRail::New
            | HomeRail::Open
            | HomeRail::Recents
            | HomeRail::Samples
            | HomeRail::Learn => {}
        }
    }
    rails
}

#[gpui::test]
fn a_rail_chosen_opens_its_pane_at_the_top(cx: &mut TestAppContext) {
    let (state, mut visual) = home_with_recents(
        cx,
        "home-rail-top",
        (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT),
        MAX_RECENTS,
    );
    let visual = &mut visual;
    // A loaded template catalog runs the Samples pane past the window, so a
    // pane opened from the end of the recents has room to open part way down.
    visual.update(|_, app| {
        state.update(app, |state, _| {
            let card = |n: usize| crate::cloud::CloudCard {
                id: format!("template-{n}").into(),
                name: format!("Template {n}").into(),
                detail: "1920 x 1080".into(),
                file_url: format!("https://example.com/template-{n}.aria"),
                extension: "aria".into(),
                attribution: String::new(),
            };
            state.cloud_feeds.insert(
                crate::cloud::CloudFeed::Templates,
                crate::cloud::FeedState::Ready((0..40).map(card).collect()),
            );
        })
    });
    let choose = |visual: &mut VisualTestContext, rail: HomeRail| {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.set_home_rail(rail);
                cx.notify();
            })
        })
    };
    let mut long = 0;
    for rail in every_rail()
        .into_iter()
        .filter(|rail| *rail != HomeRail::Recents)
    {
        choose(visual, HomeRail::Recents);
        draw(visual);
        let body = bounds(visual, "home-body");
        wheel_to_end(visual, body);
        draw(visual);
        let left = visual.read(|app| state.read(app).home_scroll.offset().y);
        choose(visual, rail);
        draw(visual);
        let (offset, end) = visual.read(|app| {
            let scroll = &state.read(app).home_scroll;
            (scroll.offset().y, scroll.max_offset().y)
        });
        if f32::from(end) > 1.0 {
            long += 1;
        }
        assert_eq!(
            f32::from(offset),
            0.0,
            "{rail:?} opens {offset:?} down, where Recents was left at {left:?}"
        );
    }
    assert!(
        long > 0,
        "no other pane runs past the window, so a rail opening at the top cannot be told \
         from a frame clamping the recents' offset"
    );
}
