//! The rows a recent-project row's menu draws.
//!
//! Split out of the parent by size. Two rows copy the path they were drawn for,
//! because they hand it to a dialog that answers after the click is over.

use std::path::PathBuf;

use crate::app::picker::Ask;

use super::{MenuItem, MenuRow, always};

/// A recent-project menu. Its facts are the path the row was drawn for. Two rows
/// copy it, because they hand it to a dialog that answers after the click is
/// over; the other two only read it.
pub const RECENT_ITEMS: &[MenuItem<PathBuf>] = &[
    MenuItem::Row(MenuRow {
        id: "ctx-recent-open",
        label: |_| "Open",
        when: always(),
        checked: None,
        run: |state, path| {
            // A project that moved is relinked rather than reported: the request
            // is to open this project, and the answer is a file dialog, not an
            // error about a path they did not type.
            if !path.exists() {
                crate::app::recents::relink_recent(state, path);
                return Ok(());
            }
            state.request_open_project(path.to_path_buf());
            Ok(())
        },
    }),
    MenuItem::Outside(MenuRow {
        id: "ctx-recent-reveal",
        label: |_| "Reveal",
        when: always(),
        checked: None,
        run: |_state, path| {
            crate::app::recents::reveal_in_folder(path);
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-recent-remove",
        label: |_| "Remove from List",
        when: always(),
        checked: None,
        run: |state, path| {
            state.forget_recent(path);
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-recent-delete",
        label: |_| "Delete File…",
        when: always(),
        checked: None,
        run: |state, path| {
            // A file leaves the disk only after the question is answered, and the
            // question names the file.
            let doomed = path.to_path_buf();
            state.request_pick(
                Ask::Confirm {
                    title: "Delete Project File".into(),
                    question: format!("Delete {}?", path.display()),
                },
                move |state, answer| {
                    if !answer.is_yes() {
                        return;
                    }
                    if let Err(err) = state.delete_recent(&doomed) {
                        state.report_error(err);
                    }
                },
            );
            Ok(())
        },
    }),
];
