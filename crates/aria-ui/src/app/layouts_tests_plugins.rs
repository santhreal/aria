use super::*;

/// A plugin workspace with every field set, sizes inside the pane bounds.
fn grading() -> aria_plugin::WorkspaceSpec {
    aria_plugin::WorkspaceSpec {
        id: "grading".into(),
        name: "Grading Room".into(),
        panes: aria_plugin::WorkspacePanes {
            sidebar_width: Some(260.0),
            sidebar_collapsed: Some(true),
            inspector_width: Some(520.0),
            inspector_collapsed: Some(false),
            timeline_height: Some(230.0),
            scopes: Some(true),
            focus: Some(aria_plugin::WorkspaceFocus::Preview),
            ..aria_plugin::WorkspacePanes::default()
        },
    }
}

/// A plugin workspace is a Window-menu row and a Settings row after the
/// shipped ones, applies the arrangement it registered, reads as active once
/// applied, and leaves the menu when its registration drops.
#[test]
fn a_plugin_workspace_is_offered_applied_and_withdrawn() {
    let cx = gpui::TestAppContext::single();
    let state = state_in(&dir());
    let ctx = std::sync::Arc::clone(&state.ctx);
    let subject = entity(state, &cx);
    let held = ctx.register_workspace(grading()).expect("registers");
    let id = format!("{}/grading", layouts::PLUGIN_PREFIX);

    let ids = window_ids(&subject, &cx);
    let at = ids
        .iter()
        .position(|row| *row == id)
        .expect("the Window menu has no row for the plugin workspace");
    let last_shipped = ids
        .iter()
        .position(|row| *row == layouts::WORKSPACES[layouts::WORKSPACES.len() - 1].id)
        .expect("shipped rows");
    assert!(at > last_shipped, "plugin rows follow the shipped ones");
    assert!(window_labels(&subject, &cx).contains(&"Grading Room"));
    let row = cx.update(|app| {
        subject
            .read(app)
            .workspace_rows()
            .into_iter()
            .find(|row| row.id == id)
            .map(|row| (row.plugin, row.active))
    });
    assert_eq!(row, Some((Some("Host"), false)));

    assert!(apply(&subject, &cx, &id));
    let here = panes_of(&subject, &cx);
    let wanted = PaneLayout {
        sidebar_width: 260.0,
        sidebar_collapsed: true,
        inspector_width: 520.0,
        inspector_collapsed: false,
        timeline_height: 230.0,
        scopes_open: true,
        panel: ActivePanel::Preview,
        route: here.route,
    };
    assert_eq!(here, wanted);
    let active = cx.update(|app| {
        subject
            .read(app)
            .workspace_rows()
            .into_iter()
            .filter(|row| row.active)
            .map(|row| row.id)
            .collect::<Vec<_>>()
    });
    assert_eq!(active, [id.as_str()]);

    drop(held);
    assert!(!window_ids(&subject, &cx).contains(&id.as_str()));
    assert!(
        !apply(&subject, &cx, &id),
        "a withdrawn workspace still applied"
    );
}

/// A field a plugin leaves unset takes the Editing arrangement, and a size
/// outside the pane bounds is pulled inside them.
#[test]
fn a_plugin_workspace_fills_unset_fields_and_clamps_sizes() {
    let editing = PaneLayout::default();
    let unset = layouts::plugin_panes(&aria_plugin::WorkspacePanes::default());
    assert_eq!(
        unset,
        PaneLayout {
            route: UiRoute::Editor,
            ..editing
        }
    );
    let wild = layouts::plugin_panes(&aria_plugin::WorkspacePanes {
        sidebar_width: Some(9000.0),
        timeline_height: Some(1.0),
        ..aria_plugin::WorkspacePanes::default()
    });
    assert_eq!(wild.sidebar_width, SIDEBAR_MAX);
    assert_eq!(wild.timeline_height, TIMELINE_MIN_HEIGHT);
}

/// Every focus a plugin can name lands on a distinct pane, so a new focus
/// variant that maps onto an existing pane turns this red.
#[test]
fn every_plugin_focus_names_its_own_pane() {
    use aria_plugin::WorkspaceFocus;
    let all = [
        WorkspaceFocus::Media,
        WorkspaceFocus::Preview,
        WorkspaceFocus::Timeline,
        WorkspaceFocus::Inspector,
        WorkspaceFocus::Assistant,
        WorkspaceFocus::Mixer,
    ];
    let panels: Vec<ActivePanel> = all
        .iter()
        .map(|focus| {
            layouts::plugin_panes(&aria_plugin::WorkspacePanes {
                focus: Some(*focus),
                ..aria_plugin::WorkspacePanes::default()
            })
            .panel
        })
        .collect();
    for panel in ActivePanel::ALL {
        assert_eq!(
            panels.iter().filter(|p| **p == panel).count(),
            1,
            "{panel:?} is not the focus of exactly one plugin focus"
        );
    }
}

#[test]
fn all_five_shipped_workspaces_produce_valid_dock_trees_and_remember_edits_until_reset() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let names: Vec<&str> = layouts::WORKSPACES.iter().map(|ws| ws.name).collect();
    assert_eq!(
        names,
        vec!["Editing", "Animation", "Color", "Audio", "Effects"]
    );
    for ws in &layouts::WORKSPACES {
        assert!(apply(&subject, &cx, ws.id));
        cx.update(|app| {
            subject
                .read(app)
                .dock_tree
                .check_invariants()
                .unwrap_or_else(|e| panic!("{} broke dock invariants: {e}", ws.id));
        });
    }

    // Switch to Editing, close the Inspector tab (`FrameId::RIGHT`), switch to
    // Colour, and switch back to Editing: the remembered edit persists until
    // Reset to Saved Layout (`RESET_ID`) restores the shipped Editing tree.
    assert!(apply(&subject, &cx, "win-workspace-editing"));
    cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            assert!(state.close_dock_panel(crate::dock::PANEL_INSPECTOR, cx));
        });
    });
    assert!(cx.update(|app| {
        !subject
            .read(app)
            .dock_tree
            .is_open(crate::dock::PANEL_INSPECTOR)
    }));
    assert!(apply(&subject, &cx, "win-workspace-colour"));
    assert!(apply(&subject, &cx, "win-workspace-editing"));
    assert!(
        cx.update(|app| {
            !subject
                .read(app)
                .dock_tree
                .is_open(crate::dock::PANEL_INSPECTOR)
        }),
        "switching back to Editing must restore its remembered dock edits"
    );

    let reset_ran = cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            crate::chrome::menu_items::run_item(AppMenu::Window, layouts::RESET_ID, state, cx)
        })
    });
    assert!(reset_ran);
    assert!(
        cx.update(|app| {
            subject
                .read(app)
                .dock_tree
                .is_open(crate::dock::PANEL_INSPECTOR)
        }),
        "Reset to Saved Layout must restore the shipped Editing tree"
    );
}

#[test]
fn save_as_new_workspace_writes_file_round_trips_and_rejects_stale_version() {
    let scratch = dir();
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&scratch), &cx);
    assert!(apply(&subject, &cx, "win-workspace-editing"));

    let path = cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            state.dock_tree.dock(
                crate::dock::PANEL_MIXER,
                crate::dock::FrameId::RIGHT,
                crate::dock::Zone::Bottom,
            );
            state.record_dock_edit(cx);
            state
                .save_workspace_as("Dialogue Cut", cx)
                .expect("save named workspace")
        })
    });
    assert_eq!(path, scratch.join("workspaces/Dialogue Cut.json"));
    assert!(path.exists());
    let expected_tree = cx.update(|app| subject.read(app).dock_tree.clone());

    // Switch away, then load the named workspace back from disk.
    assert!(apply(&subject, &cx, "win-workspace-colour"));
    cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            state
                .load_named_workspace("Dialogue Cut", cx)
                .expect("load named workspace");
        });
    });
    assert_eq!(
        cx.update(|app| subject.read(app).dock_tree.clone()),
        expected_tree
    );

    // Export to an external path and import into a fresh session.
    let export_path = scratch.join("exported-dialogue.json");
    cx.update(|app| {
        subject.update(app, |state: &mut AppState, _| {
            state
                .export_workspace(&export_path)
                .expect("export workspace");
        });
    });
    let fresh = entity(state_in(&scratch.join("fresh-profile")), &cx);
    let imported_name = cx.update(|app| {
        fresh.update(app, |state: &mut AppState, cx| {
            state
                .import_workspace(&export_path, cx)
                .expect("import workspace")
        })
    });
    assert_eq!(imported_name, "Dialogue Cut");
    assert_eq!(
        cx.update(|app| fresh.read(app).dock_tree.clone()),
        expected_tree
    );

    // A stale version in the JSON is rejected and records the error via `report_error`.
    let raw = std::fs::read_to_string(&export_path).expect("read exported json");
    let stale = raw
        .replace("\"version\": 2", "\"version\": 1")
        .replace("\"version\":2", "\"version\":1");
    let stale_path = scratch.join("stale-workspace.json");
    std::fs::write(&stale_path, stale).expect("write stale json");
    let err = cx.update(|app| {
        fresh.update(app, |state: &mut AppState, cx| {
            state.import_workspace(&stale_path, cx)
        })
    });
    assert!(err.is_err());
    assert!(
        cx.update(|app| fresh.read(app).last_error.is_some()),
        "rejecting a stale workspace file must report the error"
    );
}

#[test]
fn plugin_workspace_with_declared_dock_tree_applies_and_audio_workspace_filters_effects() {
    let cx = gpui::TestAppContext::single();
    let state = state_in(&dir());
    let ctx = std::sync::Arc::clone(&state.ctx);
    let subject = entity(state, &cx);

    // Audio workspace filters the Effects browser to Audio Effects; switching
    // to another workspace clears the filter.
    assert!(apply(&subject, &cx, "win-workspace-audio"));
    assert_eq!(
        cx.update(|app| subject.read(app).effects_browser.only),
        Some(crate::app::effects_browser::Group::AudioEffects)
    );
    assert!(apply(&subject, &cx, "win-workspace-editing"));
    assert_eq!(
        cx.update(|app| subject.read(app).effects_browser.only),
        None
    );

    // Plugin workspace with an explicit `DockTree` JSON in `WorkspacePanes::tree`.
    let mut custom_tree = crate::dock::DockTree::editing();
    custom_tree.float(crate::dock::PANEL_MIXER, (100.0, 100.0, 500.0, 320.0));
    let tree_json = custom_tree.to_json().expect("tree json");
    let _held = ctx
        .register_workspace(aria_plugin::WorkspaceSpec {
            id: "float-mix".into(),
            name: "Floating Mix".into(),
            panes: aria_plugin::WorkspacePanes {
                tree: Some(tree_json),
                ..aria_plugin::WorkspacePanes::default()
            },
        })
        .expect("register plugin workspace with tree");
    let plugin_id = format!("{}/float-mix", layouts::PLUGIN_PREFIX);
    assert!(apply(&subject, &cx, &plugin_id));
    assert_eq!(
        cx.update(|app| subject.read(app).dock_tree.floating.len()),
        1
    );
}
