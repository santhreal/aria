//! Render-side capabilities the editor shipped without a way to ask for them.
//!
//! Three holes, all measured the same way: the verb existed in `aria-render`,
//! it was tested, and nothing in this crate named it.
//!
//! - **The starter LUT pack.** `aria_render::starter_lut_pack` bakes one `.cube`
//!   per filter-catalog recipe, and the only thing that ever called it was the
//!   authoring binary that uploads the pack to a CDN. In the editor a LUT could
//!   be loaded from disk or downloaded from the cloud feed, so the ten looks the
//!   build already contains were the one source the operator could not reach.
//!   Baking one is cheap; baking ten is not, so the picker lists
//!   `starter_lut_entries` and bakes the chosen row on demand, writing it into
//!   the registered LUT cache so a second use is a file read.
//!
//! - **Rendering a frame sequence.** `PngSequenceEncoder` is the codec-free
//!   sink: it turns the same export pass into numbered PNGs on any platform,
//!   including one whose build has no native encoder at all. It was reachable
//!   from an example program and from tests. Now it is a palette row and a
//!   button on the export sheet, driven by the sheet's own settings so a
//!   half-resolution proof sequence and a half-resolution proof MP4 mean the
//!   same thing.
//!
//! - **What the last frame cost.** `FrameStats` splits a render into decode,
//!   raster, and composite, and distinguishes the part that shrinks with output
//!   resolution from the part that does not. The engine exposed it and no
//!   surface read it, so "playback is slow" had no answer in the app.
//!
//! Plus one repair: a pool item whose file changed on disk kept serving the
//! decoders and still bitmaps opened from the old bytes. Reloading is a relink
//! to the item's own path — the one command that re-probes the file and drops
//! every id-keyed cache for it — rather than a second path into the same caches.

use std::path::{Path, PathBuf};

use aria_jobs::JobCompletion;
use aria_models::MediaId;
use aria_render::{ExportSettings, StarterLut};
use aria_storage::CacheId;

use super::picker::{Ask, FileSpec};
use super::{AppState, DialogKind};

/// One shipped look, as the picker draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiLook {
    /// Pack id (`aria-<filter id>`), the identity a row is applied by.
    pub id: String,
    pub label: String,
    /// What the recipe does to `aria_render::SWATCH_REFERENCE`: one colour per
    /// reference entry, so the row shows the look rather than only naming it.
    pub swatch: Vec<[u8; 3]>,
    /// Whether the selection is currently graded through this look's table.
    pub applied: bool,
}

/// What the last rendered frame cost, split by stage.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UiRenderStats {
    pub decode_ms: f64,
    pub raster_ms: f64,
    pub composite_ms: f64,
    pub total_ms: f64,
    /// The part of the cost that shrinks when the preview renders smaller.
    pub scaled_cost_ms: f64,
    /// Effect passes the frame drew, across every layer.
    pub effect_passes: u32,
    /// Bind groups those passes needed built rather than reusing one already
    /// describing that exact pass.
    pub effect_pass_bind_groups: u32,
}

impl UiRenderStats {
    /// The rows a readout draws, in the order it draws them.
    ///
    /// One owner: the panel and the status line report the same five numbers,
    /// and neither holds its own list of them.
    pub fn rows(&self) -> [(&'static str, f64); 5] {
        [
            ("Decode", self.decode_ms),
            ("Raster", self.raster_ms),
            ("Composite", self.composite_ms),
            ("Frame", self.total_ms),
            ("Scales with size", self.scaled_cost_ms),
        ]
    }

    /// The effect-pass count, and how much of it had to be built. `None` when
    /// the frame drew no effect pass: a scene without effects has nothing to
    /// report, and "0 passes" would read as a measurement.
    pub fn passes(&self) -> Option<(u32, u32)> {
        (self.effect_passes > 0).then_some((self.effect_passes, self.effect_pass_bind_groups))
    }

    /// The effect-pass line both readouts print.
    pub fn passes_line(&self) -> Option<String> {
        self.passes()
            .map(|(passes, built)| format!("{passes} effect pass(es), {built} bind group(s) built"))
    }

    /// Whether any frame has been rendered yet. A zeroed split is "nothing has
    /// been measured", not "a frame cost nothing".
    pub fn measured(&self) -> bool {
        self.total_ms > 0.0
    }
}

/// The `.cube` file name a baked look is cached under.
fn look_file_name(id: &str) -> String {
    format!("{id}.cube")
}

/// Write one baked look into the LUT cache.
fn write_look(path: &Path, lut: &StarterLut) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("creating {}: {err}", parent.display()))?;
    }
    std::fs::write(path, lut.cube.to_cube_string(&lut.label))
        .map_err(|err| format!("writing {}: {err}", path.display()))
}

/// Whether `dir` already holds frames from an earlier sequence render.
///
/// Read rather than assumed: `PngSequenceEncoder` numbers from zero every time,
/// so rendering a shorter sequence into a used directory would leave the tail of
/// the older one behind and the folder would play as two takes spliced together.
fn holds_rendered_frames(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("frame_") && name.ends_with(".png"))
    })
}

impl AppState {
    /// Where a baked starter look is cached, or `None` when the registry puts
    /// LUTs in memory and there is nowhere on disk to write one.
    fn starter_look_path(&self, id: &str) -> Option<PathBuf> {
        Some(
            self.storage_layout()
                .resolve(CacheId::Luts)?
                .join(look_file_name(id)),
        )
    }

    /// Every look the build ships, in catalog order.
    ///
    /// The list is `aria_render::starter_lut_entries`, which is the filter
    /// catalog: a preset added to `filters.toml` appears here with no edit, and
    /// this surface holds no copy of the list. Listing costs a catalog walk and
    /// a six-pixel grade per row; baking a table is deferred to the click.
    pub fn starter_looks(&self) -> Vec<UiLook> {
        let applied = self.selected_lut().map(|lut| lut.path);
        aria_render::starter_lut_entries()
            .into_iter()
            .map(|row| {
                let file = look_file_name(&row.id);
                UiLook {
                    applied: applied.as_ref().is_some_and(|path| path.ends_with(&file)),
                    swatch: aria_render::starter_lut_swatch(&row.id).unwrap_or_default(),
                    id: row.id,
                    label: row.label,
                }
            })
            .collect()
    }

    /// Bake `id` if it is not cached yet, and report where it is.
    fn ensure_starter_look(&self, id: &str) -> Result<PathBuf, String> {
        let path = self.starter_look_path(id).ok_or_else(|| {
            "The LUT cache lives in memory, so a baked look has nowhere to be written.".to_string()
        })?;
        if path.exists() {
            return Ok(path);
        }
        let lut = aria_render::bake_starter_lut(id)
            .ok_or_else(|| format!("{id} is not a look this build ships."))?;
        write_look(&path, &lut)?;
        Ok(path)
    }

    /// Grade the selection through one shipped look.
    pub fn apply_starter_look(&mut self, id: &str) {
        if self.selected_clip().is_none() {
            self.status_message = Some("Select a clip first".into());
            return;
        }
        let path = match self.ensure_starter_look(id) {
            Ok(path) => path,
            Err(err) => {
                self.report_error(err);
                return;
            }
        };
        let label = aria_render::starter_lut_entry(id)
            .map(|row| row.label)
            .unwrap_or_else(|| id.to_string());
        if let Err(err) = self.set_selected_lut(path.to_string_lossy().to_string()) {
            self.report_error(&err);
            return;
        }
        self.status_message = Some(format!("Applied {label}"));
    }

    /// Bake every shipped look into the LUT cache at once.
    ///
    /// Worth its own action because the pack is also usable from outside the
    /// picker — a `.cube` on disk is what a colourist hands to another tool —
    /// and because baking ten tables on the click that applies one would make
    /// the picker feel broken.
    pub fn install_starter_looks(&mut self) {
        let mut written = 0usize;
        for lut in aria_render::starter_lut_pack() {
            let Some(path) = self.starter_look_path(&lut.id) else {
                self.report_error(
                    "The LUT cache lives in memory, so the pack has nowhere to be written.",
                );
                return;
            };
            if let Err(err) = write_look(&path, &lut) {
                self.report_error(err);
                return;
            }
            written += 1;
        }
        self.status_message = Some(match self.storage_layout().resolve(CacheId::Luts) {
            Some(dir) => format!("Baked {written} look(s) into {}", dir.display()),
            None => format!("Baked {written} look(s)"),
        });
    }

    /// What the last rendered frame cost, split by stage.
    ///
    /// Recorded where the frame landed, so the readout is a field read: a panel
    /// drawn while a preview decode holds the session still draws.
    pub fn last_render_stats(&self) -> UiRenderStats {
        let stats = self.frame_stats;
        UiRenderStats {
            decode_ms: stats.decode_ms,
            raster_ms: stats.raster_ms,
            composite_ms: stats.composite_ms,
            total_ms: stats.total_ms(),
            scaled_cost_ms: stats.scaled_cost_ms(),
            effect_passes: stats.effect_passes,
            effect_pass_bind_groups: stats.effect_pass_bind_groups,
        }
    }

    /// The same split as one line, for the status strip.
    pub fn report_render_cost(&mut self) {
        let stats = self.last_render_stats();
        self.status_message = Some(if stats.measured() {
            let mut parts: Vec<String> = stats
                .rows()
                .iter()
                .map(|(name, ms)| format!("{name} {ms:.1}ms"))
                .collect();
            parts.extend(stats.passes_line());
            parts.join(" · ")
        } else {
            "No frame has been rendered yet".to_string()
        });
    }

    /// Re-read one pool item from disk.
    ///
    /// A relink to the item's own path, because that is the one command that
    /// re-probes the file and invalidates every cache keyed by its id — the open
    /// decoders, the still bitmap, and any proxy standing in for it. A file
    /// replaced under the editor otherwise keeps rendering from the bytes that
    /// were open when it was imported.
    pub fn reload_media_from_disk(&mut self, media: MediaId) {
        let Some(item) = self.store.media.iter().find(|item| item.id == media.raw()) else {
            self.status_message = Some("That pool item is no longer in the project".into());
            return;
        };
        let (name, path) = (item.name.to_string(), PathBuf::from(&*item.path));
        match self.relink_media(media, path) {
            Ok(()) => self.status_message = Some(format!("Reloaded {name} from disk")),
            Err(err) => self.report_error(&err),
        }
    }

    /// Re-read the pool item the bin last acted on.
    ///
    /// The bin's own notion of "this one", not a second selection: the palette
    /// row and the pool's context menu have to mean the same item, or reloading
    /// from the keyboard would repair a file the operator is not looking at.
    pub fn reload_selected_media(&mut self) {
        let Some(id) = self.pool_selection.focus() else {
            self.status_message = Some("Pick a pool item to reload".into());
            return;
        };
        self.reload_media_from_disk(MediaId::from_raw(id));
    }

    /// Re-read every pool item from disk, reporting what could not be read.
    pub fn reload_all_media_from_disk(&mut self) {
        let items: Vec<(u64, String)> = self
            .store
            .media
            .iter()
            .map(|item| (item.id, item.path.to_string()))
            .collect();
        if items.is_empty() {
            self.status_message = Some("The pool is empty".into());
            return;
        }
        let mut reloaded = 0usize;
        let mut failed = 0usize;
        for (id, path) in items {
            match self.relink_media(MediaId::from_raw(id), PathBuf::from(path)) {
                Ok(()) => reloaded += 1,
                Err(_) => failed += 1,
            }
        }
        self.status_message = Some(if failed == 0 {
            format!("Reloaded {reloaded} item(s) from disk")
        } else {
            format!("Reloaded {reloaded} item(s); {failed} could not be read")
        });
    }

    /// The settings a frame sequence renders with: the export sheet's own
    /// choices, minus the audio track a folder of stills cannot carry.
    pub fn image_sequence_settings(&self) -> ExportSettings {
        self.export_settings().without_audio()
    }

    /// Render the sequence to numbered PNGs in `dir`, as a cancellable job.
    ///
    /// No commit boundary, unlike the video export: a frame sequence has no
    /// atomic publication step, every frame is visible the moment it is written,
    /// and a cancel leaves the frames rendered so far where the operator can see
    /// them. Refusing a directory that already holds frames is what keeps that
    /// honest — numbering restarts at zero, so a shorter second render into the
    /// same folder would leave the tail of the first behind.
    pub fn export_image_sequence_job(&mut self, dir: PathBuf) -> Result<u64, String> {
        let project = self.host().snapshot();
        let has_clips = project
            .timeline()
            .tracks_ordered()
            .any(|track| track.clips().next().is_some());
        if !has_clips {
            return Err("Nothing to render. Your timeline is empty.".into());
        }
        if holds_rendered_frames(&dir) {
            return Err(format!(
                "{} already holds rendered frames. Choose an empty folder so two takes are not mixed.",
                dir.display()
            ));
        }
        let settings = self.image_sequence_settings();
        let (start, end) = settings.frame_span(&project);
        if end <= start {
            return Err("That span covers no frames.".into());
        }
        let label = format!("Rendering frames to {}", folder_label(&dir));
        let dest = dir;
        let gpu = super::render_device::shared_device(&self.ctx);
        let kind = aria_plugin::RenderKind::ImageSequence;
        let id = self.spawn_render(label, kind, dest.clone(), move |context| {
            let mut renderer = super::render_device::renderer_on(gpu, "the render")?;
            let mut encoder = aria_render::PngSequenceEncoder::new(&dest)
                .map_err(|err| format!("opening {}: {err}", dest.display()))?;
            let mut observer = |done: u64, total: u64| {
                let fraction = if total == 0 {
                    0.0
                } else {
                    done as f32 / total as f32
                };
                context.set_progress(fraction, format!("frame {done}/{total}"));
                !context.cancelled()
            };
            aria_render::export_observed(
                &mut renderer,
                &project,
                &mut encoder,
                settings,
                &mut observer,
            )
            .map_err(|err| err.to_string())?;
            // The sink's own count, not the loop's: it is the number of files on
            // disk, which is what the operator is about to open.
            let written = encoder.frames_written();
            JobCompletion::new(format!("{written} frame(s)"))
                .with_output("path", dest.display().to_string())
                .map_err(String::from)
        });
        let (w, h) = settings.size;
        self.status_message = Some(format!(
            "Rendering frames · {w} × {h} · {}",
            self.export_range_label()
        ));
        Ok(id.raw())
    }

    /// Ask where the frames go, then start the render.
    pub fn pick_and_export_image_sequence(&mut self) {
        self.request_pick(
            Ask::Folder(FileSpec::new("Render image sequence")),
            |state, answer| {
                let Some(dir) = answer.one() else {
                    return;
                };
                match state.export_image_sequence_job(dir) {
                    Ok(_) => state.open_dialog(DialogKind::Jobs),
                    Err(err) => state.report_error(err),
                }
            },
        );
    }
}

/// The last path component, for a job label that has to fit on one line.
fn folder_label(dir: &Path) -> String {
    dir.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("the chosen folder")
        .to_string()
}

#[cfg(test)]
#[path = "render_reach_tests.rs"]
mod tests;
