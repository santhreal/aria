//! Rows: the File rows built from session state (Open Recent, Export by
//! format, Revert) and the guard every row is drawn disabled by, from the cases
//! in [`super`].
//!
//! Only `#[test]` fns live here. Every fixture and helper stays in the parent,
//! reached through `use super::*`.

use super::menu_reach_dress::with_timeline_plugin;
use super::*;
use crate::chrome::menu_file::{EXPORT_PREFIX, IMPORT_RECENT_PREFIX, RECENT_PREFIX};
use crate::chrome::menu_items::MenuItem;
use std::path::Path;

fn row(menu: AppMenu, state: &AppState, id: &str) -> MenuItem {
    items_of(menu, state)
        .into_iter()
        .find(|item| item.id == id)
        .unwrap_or_else(|| panic!("{id} is not on offer against this fixture"))
}

/// A row declared to edit its dressing is drawn enabled over it. A guard
/// stricter than the row's own code draws a working row disabled, and the
/// command is then unreachable from the bar, the palette, or a key.
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
    let before = state.store.sequence.clip_count();
    assert_eq!(
        row(AppMenu::File, &state, "file-revert").blocked_by(&state),
        Some("There are no changes since the last save")
    );
    let ui = state
        .selected_ui_clip()
        .expect("the fixture selects its clip");
    let mid = ui.timeline_start_ticks + (ui.timeline_end_ticks - ui.timeline_start_ticks) / 2;
    state.set_playhead(mid);
    state.split_at_playhead().expect("an unsaved edit");
    assert_eq!(state.store.sequence.clip_count(), before + 1);
    assert_eq!(
        row(AppMenu::File, &state, "file-revert").blocked_by(&state),
        None
    );
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-revert");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(
            state.store.sequence.clip_count(),
            before,
            "the split survived the revert"
        );
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
    let before = state.store.sequence.clip_count();
    assert_eq!(
        row(AppMenu::File, &state, "file-revert").blocked_by(&state),
        Some("The project has never been saved")
    );
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-revert");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.store.sequence.clip_count(), before);
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
        Some("The project file is missing")
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
        assert_eq!(
            state.store.sequence.clip_count(),
            2,
            "the other project's two clips"
        );
        assert_eq!(state.picks_asked(), 0, "Open Recent asks for no file");
    });
}

/// A recent row reads the file's name, and its folder only when another row
/// of the same list has that name. A full path is cut at the menu's and the
/// palette's edge from the right, so the name, the part read, was lost first.
#[test]
fn recent_rows_read_the_file_name_and_the_folder_only_to_tell_two_apart() {
    let root = own_scratch("recent-labels");
    let (a, b) = (root.join("a"), root.join("b"));
    let mut state = saved();
    for path in [a.join("cut.aria"), b.join("cut.aria"), a.join("solo.aria")] {
        state.remember_recent(&path);
    }
    state.remember_imports(&[a.join("shot.mov"), b.join("shot.mov"), a.join("only.wav")]);
    for (prefix, shared, unique) in [
        (RECENT_PREFIX, "cut.aria", "solo.aria"),
        (IMPORT_RECENT_PREFIX, "shot.mov", "only.wav"),
    ] {
        for dir in [&a, &b] {
            let want = format!("{shared} ({})", dir.display());
            assert_eq!(label(&state, prefix, &dir.join(shared)), want);
        }
        assert_eq!(label(&state, prefix, &a.join(unique)), unique);
    }
}

/// The label of the File row whose id is `prefix` and `path`.
fn label(state: &AppState, prefix: &str, path: &Path) -> &'static str {
    let id = format!("{prefix}{}", path.display());
    row(AppMenu::File, state, &id).label
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
    let refused = Some("No media is selected in the pool");
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

fn blocked(menu: AppMenu, state: &AppState, id: &str) -> Option<&'static str> {
    row(menu, state, id).blocked_by(state)
}

/// WHY: closes the defect class where Edit, Clip, and Sequence menu rows for
/// Paste Attributes, Paste to Fit, Freeze Frame, Render and Replace, Restore
/// Unrendered, Ripple Trim In/Out, Cross Fade / Remove Transition, Shift Lane,
/// and Nested Sequence actions omitted precondition checks and were offered
/// enabled when their target clip, mark range, project path, cut junction, or
/// nested sequence did not exist.
///
/// What this does not catch: the visual styling of disabled rows in the popup.
#[test]
fn menu_rows_withhold_paste_bake_ripple_transition_shift_and_nest_until_ready() {
    use super::menu_reach_dress::Dress;
    let (mut state, clip) = with_clip();
    assert_eq!(
        blocked(AppMenu::Edit, &state, "edit-paste-attributes"),
        Some("Nothing has been copied or cut")
    );
    assert_eq!(
        blocked(AppMenu::Edit, &state, "edit-paste-fit"),
        Some("Nothing has been copied or cut")
    );
    state.copy_selected();
    state.clear_selection();
    assert_eq!(
        blocked(AppMenu::Edit, &state, "edit-paste-attributes"),
        Some("No clip is selected")
    );
    assert_eq!(
        blocked(AppMenu::Edit, &state, "edit-paste-fit"),
        Some("Mark In and Out to paste into a range")
    );
    state.select_clip(clip);
    assert_eq!(
        blocked(AppMenu::Edit, &state, "edit-paste-attributes"),
        None
    );
    state.clear_selected().expect("remove copied clip");
    assert_eq!(
        blocked(AppMenu::Edit, &state, "edit-paste-attributes"),
        Some("Nothing has been copied or cut")
    );
    state
        .paste_attributes()
        .expect("deleted clipboard clip is inert");
    assert_eq!(
        blocked(AppMenu::Edit, &Dress::Fitted.state(), "edit-paste-fit"),
        None
    );

    let (mut plain, _) = with_clip();
    assert_eq!(
        blocked(AppMenu::Clip, &plain, "seq-bake-clip"),
        Some("Save the project first: a rendered clip is kept beside the project file.")
    );
    assert_eq!(
        blocked(AppMenu::Clip, &plain, "seq-bake-restore"),
        Some("Select a rendered clip to restore")
    );
    plain.project_path = Some(crate::scratch_pool::scratch("menu-bake-guard").join("p.aria"));
    assert_eq!(
        blocked(AppMenu::Clip, &plain, "seq-bake-clip"),
        Some("This clip has nothing to render into a file")
    );
    assert_eq!(
        blocked(AppMenu::Clip, &Dress::Rendered.state(), "seq-bake-clip"),
        None
    );
    assert_eq!(
        blocked(AppMenu::Clip, &Dress::Baked.state(), "seq-bake-restore"),
        None
    );

    let (mut ripple, clip) = with_clip();
    let end = ripple
        .ui_clip_ref(clip)
        .map(|c| c.timeline_end_ticks)
        .expect("clip end");
    ripple.set_playhead(0);
    assert_eq!(
        blocked(AppMenu::Sequence, &ripple, "seq-ripple-in"),
        Some("No earlier edit on a targeted track")
    );
    assert_eq!(blocked(AppMenu::Sequence, &ripple, "seq-ripple-out"), None);
    ripple.set_playhead(end);
    assert_eq!(blocked(AppMenu::Sequence, &ripple, "seq-ripple-in"), None);
    assert_eq!(
        blocked(AppMenu::Sequence, &ripple, "seq-ripple-out"),
        Some("No later edit on a targeted track")
    );

    assert_eq!(
        blocked(AppMenu::Sequence, &ripple, "seq-transition-crossfade"),
        Some("No clip on the other side of that cut")
    );
    assert_eq!(
        blocked(AppMenu::Sequence, &ripple, "seq-transition-clear"),
        Some("No transition at that cut")
    );
    assert_eq!(
        blocked(
            AppMenu::Sequence,
            &Dress::Cut.state(),
            "seq-transition-crossfade"
        ),
        None
    );
    assert_eq!(
        blocked(
            AppMenu::Sequence,
            &Dress::Blended.state(),
            "seq-transition-clear"
        ),
        None
    );

    ripple.clear_selection();
    assert_eq!(
        blocked(AppMenu::Sequence, &ripple, "seq-lane-shift-later"),
        Some("No clip is selected")
    );
    assert_eq!(
        blocked(AppMenu::Sequence, &ripple, "seq-lane-shift-earlier"),
        Some("No clip is selected")
    );
    ripple.select_clip(clip);
    assert_eq!(
        blocked(AppMenu::Sequence, &ripple, "seq-lane-shift-later"),
        None
    );
    assert_eq!(
        blocked(AppMenu::Sequence, &ripple, "seq-lane-shift-earlier"),
        None
    );
    crate::commands::reach::shift_lane(&mut ripple, false);
    assert_eq!(
        ripple.status_message.as_deref(),
        Some("That offset would move a clip before the start of the sequence")
    );
    assert_eq!(ripple.last_error, None);

    assert_eq!(
        blocked(AppMenu::Sequence, &ripple, "seq-nest-open"),
        Some("No nested sequence is selected")
    );
    assert_eq!(
        blocked(AppMenu::Sequence, &ripple, "seq-nest-place"),
        Some("No nested sequence is selected")
    );
    assert_eq!(
        blocked(AppMenu::Sequence, &ripple, "seq-nest-leave"),
        Some("Already at the root sequence")
    );
    let nest = Dress::Nest.state();
    assert_eq!(blocked(AppMenu::Sequence, &nest, "seq-nest-open"), None);
    assert_eq!(blocked(AppMenu::Sequence, &nest, "seq-nest-place"), None);
    assert_eq!(
        blocked(AppMenu::Sequence, &Dress::Inside.state(), "seq-nest-leave"),
        None
    );
}

/// WHY: closes the defect class where `ungroup_selected` and
/// `duplicate_selection_to_next_lane` dispatched invalid engine commands when
/// the selected clip was not grouped or when the first compatible track was
/// already occupied at the clip's span.
#[test]
fn ungroup_and_duplicate_to_next_lane_decline_without_engine_errors() {
    use super::menu_reach_dress::Dress;
    let (state, _) = with_clip();
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);
    run_row(&entity, &cx, AppMenu::Clip, "clip-ungroup");
    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(
            state.status_message.as_deref(),
            Some("The selected clips are not a group")
        );
        assert_eq!(state.last_error, None);
    });

    let (mut occupied, clip) = with_clip();
    occupied
        .add_track(aria_models::TrackKind::Video, "V2")
        .expect("add V2");
    occupied
        .duplicate_selection_to_next_lane()
        .expect("occupy V2 at the same span");
    occupied.select_clip(clip);
    occupied
        .duplicate_selection_to_next_lane()
        .expect("occupied compatible track declines cleanly");
    assert_eq!(
        occupied.status_message.as_deref(),
        Some("Every other compatible track already has a clip at that time")
    );
    assert_eq!(occupied.last_error, None);

    occupied
        .add_track(aria_models::TrackKind::Video, "V3")
        .expect("add V3");
    occupied
        .duplicate_selection_to_next_lane()
        .expect("skips occupied V2 and duplicates onto V3");
    assert_eq!(
        occupied.status_message.as_deref(),
        Some("Duplicated onto V3")
    );
    assert_eq!(occupied.last_error, None);

    let mut linked_video = Dress::Sound.state();
    let media = aria_models::MediaId::from_raw(linked_video.store.media[0].id);
    let end = linked_video.store.sequence.duration_ticks;
    linked_video
        .drop_media_on_timeline(media, None, end)
        .expect("drop linked video+audio pair");
    let v_clip = linked_video.store.sequence.tracks[0]
        .clips
        .iter()
        .find(|c| c.timeline_start_ticks == end)
        .map(|c| aria_models::ClipId::from_raw(c.id))
        .expect("dropped video clip");
    linked_video.select_clip(v_clip);
    assert_eq!(
        blocked(AppMenu::Clip, &linked_video, "seq-freeze"),
        Some("Move the playhead onto the selected clip")
    );
    linked_video.set_playhead(end + 5);
    assert_eq!(
        blocked(AppMenu::Clip, &linked_video, "seq-freeze"),
        Some("Unlink the clip before freezing a frame")
    );
    linked_video
        .freeze_selected_frame()
        .expect("linked video clip declines without engine error");
    assert_eq!(
        linked_video.status_message.as_deref(),
        Some("Unlink the clip before freezing a frame")
    );
    assert_eq!(linked_video.last_error, None);
}
