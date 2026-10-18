//! Rows: the File rows built from session state (Open Recent, Export by
//! format, Revert) and the guard every row is drawn disabled by, from the cases
//! in [`super`].
//!
//! Only `#[test]` fns live here. Every fixture and helper stays in the parent,
//! reached through `use super::*`.

use super::menu_reach_dress::with_timeline_plugin;
use super::*;
use crate::chrome::menu_items::{EXPORT_PREFIX, MenuItem, RECENT_PREFIX};

fn row(menu: AppMenu, state: &AppState, id: &str) -> MenuItem {
    items_of(menu, state)
        .into_iter()
        .find(|item| item.id == id)
        .unwrap_or_else(|| panic!("{id} is not on offer against this fixture"))
}

/// A row declared to edit its dressing is drawn enabled over it. A guard
/// stricter than the row's own code draws a working row disabled, and the
/// operator then cannot reach it from the bar, the palette, or a key.
#[test]
fn no_row_is_drawn_disabled_over_the_state_it_edits() {
    let mut wrong = Vec::new();
    for row in &CENSUS.rows {
        if row.kind == Edits
            && let Some(reason) = row.blocked
        {
            wrong.push(format!("{} against {:?}: {reason}", row.id, row.dress));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Revert reopens the saved file: the edit made since is gone, and the row is
/// drawn disabled again because nothing is unsaved.
#[test]
fn revert_drops_the_edits_made_since_the_save() {
    let mut state = saved();
    let path = state.project_path.clone();
    let before = clip_count(&state);
    assert_eq!(
        row(AppMenu::File, &state, "file-revert").blocked_by(&state),
        Some("There are no changes since the last save.")
    );
    let ui = state
        .selected_ui_clip()
        .expect("the fixture selects its clip");
    let mid = ui.timeline_start_ticks + (ui.timeline_end_ticks - ui.timeline_start_ticks) / 2;
    state.set_playhead(mid);
    state.split_at_playhead().expect("an unsaved edit");
    assert_eq!(clip_count(&state), before + 1);
    assert_eq!(
        row(AppMenu::File, &state, "file-revert").blocked_by(&state),
        None
    );
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-revert");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(clip_count(state), before, "the split survived the revert");
        assert!(!state.store.is_dirty);
        assert_eq!(state.project_path, path);
        assert_eq!(state.picks_asked(), 0, "Revert asks for no file");
    });
}

/// A project never saved has nothing to revert to: the row is drawn disabled
/// with the reason, and running it anyway changes nothing.
#[test]
fn revert_on_a_project_never_saved_is_disabled_and_inert() {
    let (state, _) = with_clip();
    let before = clip_count(&state);
    assert_eq!(
        row(AppMenu::File, &state, "file-revert").blocked_by(&state),
        Some("The project has never been saved.")
    );
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-revert");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(clip_count(state), before);
        assert!(state.project_path.is_none());
    });
}

/// Each remembered project is a row under Open Recent, and choosing one opens
/// it. A remembered file that is gone is drawn disabled with the reason.
#[test]
fn open_recent_lists_each_remembered_project_and_opens_the_one_chosen() {
    let (_, other) = Answered::Open.state();
    let gone = own_scratch("recent-gone").join("gone.aria");
    let mut state = saved();
    state.remember_recent(&gone);
    state.remember_recent(&other);
    let open = format!("{RECENT_PREFIX}{}", other.display());
    let missing = format!("{RECENT_PREFIX}{}", gone.display());
    assert_eq!(row(AppMenu::File, &state, &open).blocked_by(&state), None);
    assert_eq!(
        row(AppMenu::File, &state, &missing).blocked_by(&state),
        Some("The project file is missing.")
    );
    assert_eq!(
        row(AppMenu::File, &state, &open).under,
        Some("Open Recent"),
        "the row sits in the Open Recent submenu"
    );
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, &missing);
    cx.update(|app| {
        let state = entity.read(app);
        assert_ne!(state.project_path.as_deref(), Some(gone.as_path()));
    });

    run_row(&entity, &cx, AppMenu::File, &open);
    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.project_path.as_deref(), Some(other.as_path()));
        assert_eq!(clip_count(state), 2, "the other project's two clips");
        assert_eq!(state.picks_asked(), 0, "Open Recent asks for no file");
    });
}

/// Each mounted timeline exporter has its own row under Export, and the file
/// it writes takes that exporter's extension whatever name was typed.
#[test]
fn each_timeline_exporter_has_an_export_row_that_writes_its_own_format() {
    // No answer is queued ahead of the ones below: answers are taken in
    // order, so a queued one would be the first row's file.
    let (mut state, _) = with_timeline_plugin("export-as");
    // A caption on a text track, so the subtitle exporters have a cue to write.
    let text = state
        .ensure_kind_track(aria_models::TrackKind::Text)
        .expect("a text track");
    let rate = state.playhead.rate;
    state
        .apply_command(aria_commands::Command::Edit(
            aria_commands::EditCommand::AddGenerated {
                track: text,
                generator: aria_models::Generator::text("A caption".to_owned()),
                timeline: aria_models::TimeRange::at_rate(0, 24, rate),
            },
        ))
        .expect("a caption");
    let exporters = state.ctx.exporters();
    assert!(!exporters.is_empty(), "the plugin registers exporters");
    let dir = own_scratch("export-as");
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);
    for exporter in &exporters {
        let id = format!("{EXPORT_PREFIX}{}", exporter.id);
        let typed = dir.join(format!("as-{}.txt", exporter.id));
        cx.update(|app| {
            entity.update(app, |state: &mut AppState, _| {
                assert_eq!(row(AppMenu::File, state, &id).under, Some("Export"));
                state.supply_pick(Answer::file(&typed));
            })
        });

        run_row(&entity, &cx, AppMenu::File, &id);

        let written = typed.with_extension(&exporter.extension);
        assert!(
            written.exists(),
            "{} wrote no {}",
            exporter.name,
            written.display()
        );
        assert!(
            !typed.exists(),
            "{} kept the typed extension",
            exporter.name
        );
    }
}

/// Insert Pool Selection places the media picked in the pool, so it is drawn
/// disabled with the reason until something online is picked: nothing picked
/// and a picked item whose file is missing both refuse.
#[test]
fn insert_pool_selection_is_disabled_until_online_media_is_picked() {
    let (mut state, _) = with_clip();
    let media = state
        .store
        .media
        .first()
        .map(|item| item.id)
        .expect("the fixture imported one pool item");
    let refused = Some("No media is selected in the pool.");
    let blocked =
        |state: &AppState| row(AppMenu::Sequence, state, "seq-ripple-insert").blocked_by(state);
    state.pool_selection.clear();
    assert_eq!(blocked(&state), refused, "nothing picked");
    state.select_pool_item(media);
    assert_eq!(blocked(&state), None, "an online item picked");
    state.store_mut().media[0].missing = true;
    assert_eq!(blocked(&state), refused, "the only item picked is offline");
}

/// View > Canvas Handles offers each set by name: choosing one makes it the
/// live set, and only its row is checked. Derived from [`OverlayMode::ALL`],
/// so a set added without a row fails here.
#[test]
fn canvas_handles_rows_choose_the_live_set_and_check_only_it() {
    use crate::types::OverlayMode;
    let (state, _) = with_clip();
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);
    let under = |app: &gpui::App| -> Vec<MenuItem> {
        items_of(AppMenu::View, entity.read(app))
            .into_iter()
            .filter(|item| item.under == Some("Canvas Handles"))
            .collect()
    };
    // The default set last, so every press is a change.
    for mode in OverlayMode::ALL.into_iter().rev() {
        let id = cx
            .update(|app| {
                under(app)
                    .into_iter()
                    .find(|item| item.label == mode.label())
            })
            .unwrap_or_else(|| panic!("no Canvas Handles row for {mode:?}"))
            .id;
        run_row(&entity, &cx, AppMenu::View, id);
        cx.update(|app| {
            let state = entity.read(app);
            assert_eq!(state.overlay_mode(), mode, "{id}");
            let checked: Vec<_> = under(app)
                .into_iter()
                .filter(|item| item.checked.is_some_and(|checked| checked(item.id, state)))
                .map(|item| item.id)
                .collect();
            assert_eq!(checked, [id], "{id}");
        });
    }
    assert_eq!(
        cx.update(|app| under(app).len()),
        OverlayMode::ALL.len(),
        "a Canvas Handles row for a set that does not exist"
    );
}
