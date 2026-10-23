//! Palette rows for the render side: the shipped looks, a frame sequence, what
//! the last frame cost, and re-reading a file that changed under the editor.
//!
//! One of the tables [`super::BUILTIN_COMMANDS`] joins. Every row here
//! closes a measured hole in `aria-render`: the capability existed, was tested,
//! and nothing in this crate named it. The bodies are one call each, because the
//! decision each one makes lives on `AppState` where the look picker and the
//! export sheet reach the same verb.

use super::host::CommandEntry;

/// Rows that drive the renderer, the shipped LUT pack, and the media pool's
/// on-disk backing.
pub(super) const RENDER_ROWS: &[CommandEntry] = &[
    CommandEntry {
        id: "pal-look-install",
        label: "Install Starter LUT Pack",
        run: |state| state.install_starter_looks(),
    },
    // Rendering a frame sequence is not a row here. It asks where the frames go,
    // and the surface that already holds the scale, the rate, and the range it
    // renders with is the export sheet's Frames button — the same precedent as
    // `pal-export`, which opens the sheet rather than starting a render.
    CommandEntry {
        id: "pal-render-cost",
        label: "Last Frame Cost",
        run: |state| state.report_render_cost(),
    },
    CommandEntry {
        id: "pal-render-in-to-out",
        label: "Render In to Out",
        run: |state| {
            if let Err(err) = state.render_in_to_out() {
                state.status_message = Some(err);
            }
        },
    },
    CommandEntry {
        id: "pal-render-selection",
        label: "Render Selection",
        run: |state| {
            if let Err(err) = state.render_selection() {
                state.status_message = Some(err);
            }
        },
    },
    CommandEntry {
        id: "pal-delete-render-files",
        label: "Delete Render Files",
        run: |state| {
            if let Err(err) = state.delete_render_files() {
                state.status_message = Some(err);
            }
        },
    },
    CommandEntry {
        id: "pal-media-reload",
        label: "Reload Media from Disk",
        run: |state| state.reload_selected_media(),
    },
    CommandEntry {
        id: "pal-media-reload-all",
        label: "Reload All Media from Disk",
        run: |state| state.reload_all_media_from_disk(),
    },
];
