//! Palette rows for media support: what this build decodes, and what the
//! selected source is made of.
//!
//! One of the tables [`super::BUILTIN_COMMANDS`] joins. Split by
//! concern like its siblings: a row here asks the decoder a question about a
//! file or a build, rather than editing the project. The two rows that decode a
//! whole file go to the job board, so the palette returns immediately and the
//! answer lands in the jobs panel.

use super::host::CommandEntry;

/// Rows that reach the decoder, the image and pixel catalogs, and the gain and
/// scope measurements.
pub(super) const MEDIA_ROWS: &[CommandEntry] = &[
    CommandEntry {
        id: "pal-media-support",
        label: "Media Support Report",
        run: |state| state.open_dialog(crate::app::DialogKind::MediaSupport),
    },
    CommandEntry {
        id: "pal-media-verify",
        label: "Verify Decode of Selected Source",
        run: |state| state.verify_selected_decode(),
    },
    CommandEntry {
        id: "pal-media-loudness",
        label: "Loudness Overview of Selected Source",
        run: |state| state.file_loudness_overview(),
    },
    CommandEntry {
        id: "pal-media-exposure",
        label: "Measure Exposure of the Frame on Screen",
        run: |state| state.measure_exposure(),
    },
    CommandEntry {
        id: "pal-media-normalise",
        label: "Normalize Selected Clip to Target",
        run: |state| state.normalise_selected_clip(),
    },
    CommandEntry {
        id: "pal-media-target",
        label: "Cycle Normalize Target Level",
        run: |state| state.cycle_normalise_target(),
    },
    CommandEntry {
        id: "pal-media-import-still",
        label: "Import Still or Animation…",
        run: |state| state.import_still_image(),
    },
    CommandEntry {
        id: "pal-media-lottie",
        label: "Import Lottie Animation…",
        run: |state| state.import_lottie(),
    },
];
