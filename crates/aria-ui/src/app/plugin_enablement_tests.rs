//! Plugin command enablement and icons, as the editor publishes and draws them.
//!
//! Closes the class of a condition read against keys the editor did not
//! publish, a disabled command that still runs or still shows in a menu, and an
//! icon a row names but the asset source does not serve. The keys are checked
//! against `ContextKey::ALL` and `ActivePanel::ALL`, so a key or a panel added
//! without a mapping fails here. Not covered: the pixels of a dimmed row, and
//! the macOS bar, which needs a Mac.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use aria_plugin::{
    CommandIcon, CommandSpec, ContextKey, ContextKeys, Enablement, MenuCommand, MenuLocation,
    MenuPath, PluginContext, Registration,
};

use crate::app::palette::Offered;
use crate::app::premiere_tests::with_clip;
use crate::app::{AppState, ContextPluginRow};
use crate::chrome::menu_items::MenuItem;
use crate::chrome::plugin_menus::{bar_items, icon_of, rows_in};
use crate::types::{ActivePanel, AppMenu, ContextTarget};

const SQUARE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2 2h12v12H2z"/></svg>"#;

fn on(keys: ContextKeys) -> Vec<ContextKey> {
    ContextKey::ALL
        .into_iter()
        .filter(|key| keys.has(*key))
        .collect()
}

fn register(state: &AppState, spec: CommandSpec) -> (Registration, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&runs);
    let registration = state
        .ctx
        .register_command(spec, move |_: &PluginContext| {
            seen.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .expect("the command registers");
    (registration, runs)
}

fn place(state: &AppState, command: &str, path: &str) -> Registration {
    state
        .ctx
        .register_menu_entry(MenuCommand {
            command: command.into(),
            path: MenuPath::parse(path).expect("a menu path"),
        })
        .expect("the menu entry registers")
}

#[test]
fn the_published_keys_follow_the_editor_state() {
    use ContextKey::{
        Clips, FocusTimeline, InOut, MarkIn, MarkOut, Playing, Selection, SingleSelection,
    };

    let (mut state, clip) = with_clip();
    state.selection.clear();
    state.mark_in = None;
    state.mark_out = None;
    state.playing = false;
    state.active_panel = ActivePanel::Timeline;
    state.publish_view();
    assert_eq!(on(state.ctx.context_keys()), [Clips, FocusTimeline]);

    state.selection = vec![clip];
    state.publish_view();
    assert_eq!(
        on(state.ctx.context_keys()),
        [Clips, Selection, SingleSelection, FocusTimeline]
    );

    state.selection = vec![clip, clip];
    state.mark_in = Some(0);
    state.publish_view();
    assert_eq!(
        on(state.ctx.context_keys()),
        [Clips, Selection, MarkIn, FocusTimeline]
    );

    state.mark_in = None;
    state.mark_out = Some(10);
    state.publish_view();
    assert_eq!(
        on(state.ctx.context_keys()),
        [Clips, Selection, MarkOut, FocusTimeline]
    );

    state.mark_in = Some(0);
    state.playing = true;
    state.publish_view();
    assert_eq!(
        on(state.ctx.context_keys()),
        [
            Clips,
            Selection,
            MarkIn,
            MarkOut,
            InOut,
            Playing,
            FocusTimeline
        ]
    );

    let empty = AppState::new(
        PluginContext::new(),
        crate::scratch_pool::scratch("plugin-enablement"),
    )
    .expect("state");
    empty.publish_view();
    assert!(
        !empty.ctx.context_keys().has(Clips),
        "no clip, yet clips held"
    );
}

#[test]
fn each_panel_publishes_one_focus_key_of_its_own() {
    let (mut state, _) = with_clip();
    let focus: Vec<ContextKey> = ContextKey::ALL
        .into_iter()
        .filter(|key| key.name().starts_with("focus."))
        .collect();
    let mut seen = Vec::new();
    for panel in ActivePanel::ALL {
        state.active_panel = panel;
        state.publish_view();
        let held: Vec<ContextKey> = on(state.ctx.context_keys())
            .into_iter()
            .filter(|key| focus.contains(key))
            .collect();
        assert_eq!(held.len(), 1, "{panel:?} holds {held:?}");
        assert!(!seen.contains(&held[0]), "{panel:?} shares {:?}", held[0]);
        seen.push(held[0]);
    }
    assert_eq!(seen.len(), focus.len(), "a focus key no panel publishes");
}

/// A disabled command stays in the palette and is refused with its condition;
/// the menu bar and the context menus leave it out until the condition holds.
#[test]
fn a_disabled_command_is_refused_and_left_out_of_menus() {
    let (mut state, clip) = with_clip();
    let mut spec = CommandSpec::new("grade.match", "Match Grade");
    spec.enablement = Some(Enablement::parse("single-selection && !playing").expect("parses"));
    let (_command, runs) = register(&state, spec);
    let _bar = place(&state, "grade.match", "Sequence");
    let _clip_menu = place(&state, "grade.match", "Clip");
    let target = ContextTarget::Clip {
        clip: clip.raw(),
        track: 0,
    };

    state.selection.clear();
    state.publish_view();
    assert!(rows_in(&state.ctx, MenuLocation::Sequence).is_empty());
    assert!(state.plugin_menu_rows(&target).is_empty());
    for ch in "match grade".chars() {
        state.palette_push(ch);
    }
    assert!(state.run_palette_highlight().is_none());
    assert_eq!(runs.load(Ordering::Relaxed), 0, "a disabled command ran");
    assert_eq!(
        state.last_error.as_deref(),
        Some("Match Grade: needs single-selection && !playing")
    );

    state.selection = vec![clip];
    state.last_error = None;
    state.publish_view();
    let bar = rows_in(&state.ctx, MenuLocation::Sequence);
    assert_eq!(bar.len(), 1);
    assert_eq!(bar[0].command, "grade.match");
    assert_eq!(state.plugin_menu_rows(&target).len(), 1);
    assert!(state.run_palette_highlight().is_none());
    assert_eq!(runs.load(Ordering::Relaxed), 1);
    assert_eq!(state.last_error, None);

    state.playing = true;
    state.publish_view();
    assert!(rows_in(&state.ctx, MenuLocation::Sequence).is_empty());
    state.run_plugin_command("grade.match");
    assert_eq!(runs.load(Ordering::Relaxed), 1, "ran while playing");
}

#[test]
fn a_command_icon_is_served_to_every_row_that_draws_it() {
    let (state, clip) = with_clip();
    let icon = CommandIcon::from_svg(SQUARE.as_bytes()).expect("parses");
    let path = crate::icons::plugin_icon_path(&icon);
    assert_eq!(path, format!("plugin-icon/{:016x}.svg", icon.key()));
    let again = CommandIcon::from_svg(SQUARE.as_bytes()).expect("parses");
    assert!(std::ptr::eq(path, crate::icons::plugin_icon_path(&again)));
    let served = gpui::AssetSource::load(&crate::icons::Assets, path)
        .expect("loads")
        .expect("served");
    assert_eq!(&*served, icon.svg().as_bytes());
    let unpadded = format!("plugin-icon/{:x}.svg", icon.key());
    let other = format!("plugin-icon/{:016x}.svg", icon.key() ^ 1);
    for unserved in [
        unpadded.as_str(),
        other.as_str(),
        "plugin-icon/zz.svg",
        "plugin-icon/.svg",
    ] {
        if unserved == path {
            continue;
        }
        let loaded = gpui::AssetSource::load(&crate::icons::Assets, unserved).expect("loads");
        assert!(loaded.is_none(), "{unserved} served");
    }

    let mut spec = CommandSpec::new("grade.icon", "Icon Grade");
    spec.icon = Some(icon.clone());
    let (_command, _) = register(&state, spec);
    let _bar = place(&state, "grade.icon", "Sequence");
    let _clip_menu = place(&state, "grade.icon", "Clip");
    state.publish_view();

    let rows = rows_in(&state.ctx, MenuLocation::Sequence);
    assert_eq!(rows[0].icon, Some(path));
    let items = bar_items(AppMenu::Sequence, &state);
    assert_eq!(icon_of(&rows, &items[0]), Some(path));
    let shipped = MenuItem::new(
        Box::leak(items[0].id.to_owned().into_boxed_str()),
        Box::leak(items[0].label.to_owned().into_boxed_str()),
        |_, _, _| {},
    );
    assert_eq!(
        icon_of(&rows, &shipped),
        None,
        "a shipped row took the icon"
    );

    let target = ContextTarget::Clip {
        clip: clip.raw(),
        track: 0,
    };
    let context = state.plugin_menu_rows(&target);
    assert!(matches!(
        context.first(),
        Some(row @ ContextPluginRow::Command { .. }) if row.icon() == Some(path)
    ));

    let offered = state
        .palette_catalog()
        .iter()
        .find_map(|offer| match &offer.action {
            Offered::Plugin(spec) if spec.id == "grade.icon" => spec.icon.clone(),
            _ => None,
        });
    assert_eq!(offered, Some(icon));
}
