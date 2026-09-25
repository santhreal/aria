//! Render and replace: turning one clip into a file, and putting it back.
//!
//! A clip with a deep effect stack is re-rendered on every frame of every
//! playback. Rendering it once and decoding that instead is the standard answer,
//! and the whole feature is the two rules that make it reversible.
//!
//! - **The file lives beside the project, not in a cache.** Every cache in the
//!   registry is clearable, and clearing one that a project decodes through
//!   would break the project — which is why the cache contracts forbid a cache
//!   from holding user data. So an intermediate goes in `<project> Renders/`,
//!   and a project that has never been saved is refused by name rather than
//!   given a home that will be swept.
//! - **The file is named by what it shows.** The name is a digest of the clip as
//!   it was rendered plus the canvas and rate it was rendered at, so re-rendering
//!   an unchanged clip adopts the file that is already there, and changing the
//!   clip's look renders a new one instead of overwriting a file that the undo
//!   stack still points at.
//!
//! The render itself is [`aria_engine::bake_project`] exported through the
//! ordinary export path, on a renderer of its own: a bake takes minutes and the
//! preview cannot hold the engine lock for that. The swap is the job's last act,
//! as one history entry — the import of the rendered file and the replacement
//! are grouped, so one undo puts the clip back and leaves no orphan pool item.

use std::path::PathBuf;
use std::sync::Arc;

use aria_commands::{Command, EditCommand, ProjectCommand};
use aria_engine::{ApplyOutcome, BakeMedium, BakePass, bake_project};
use aria_jobs::JobCompletion;
use aria_models::ClipId;
use aria_plugin::{CommandProposed, PluginContext};
use aria_render::ExportSettings;
use sha2::{Digest, Sha256};

use super::AppState;

impl AppState {
    /// Render the selected clip and put the rendered file in its place.
    pub fn render_and_replace(&mut self) -> Result<Option<u64>, String> {
        let clip = self
            .selected_clip()
            .ok_or_else(|| "Select a clip to render".to_string())?;
        self.render_clip(clip)
    }

    /// Render `clip` and put the rendered file in its place.
    ///
    /// Returns the job doing the render, or `None` when the file was already on
    /// disk and the swap is in force by the time this returns. Takes the clip
    /// rather than reading the selection, because the assistant addresses a clip
    /// by id and a render started for the wrong one costs minutes of encoding.
    pub fn render_clip(&mut self, clip: ClipId) -> Result<Option<u64>, String> {
        let pass = {
            let engine = self.engine.lock();
            bake_project(engine.project(), clip).map_err(|err| err.to_string())?
        };
        let dest = self.bake_file(clip, &pass)?;
        if dest.exists() {
            self.adopt_bake(clip, dest)?;
            self.status_message = Some("Decoding through the render on disk".into());
            self.sync_from_engine();
            return Ok(None);
        }
        self.build_bake(clip, pass, dest).map(Some)
    }

    /// Put back the clip a render replaced.
    pub fn restore_unrendered(&mut self) -> Result<(), String> {
        let clip = self
            .selected_clip()
            .ok_or_else(|| "Select a rendered clip to restore".to_string())?;
        self.apply_command(Command::Edit(EditCommand::RestoreBakedClip { clip }))
            .map_err(|err| err.to_string())?;
        self.status_message = Some("Restored the clip that was rendered".into());
        Ok(())
    }

    /// Where `clip`'s render lives, given the pass that would produce it.
    ///
    /// The digest covers the clip as the renderer will read it and the canvas
    /// and rate it will be written at, which is everything that decides the
    /// file's pixels.
    ///
    /// Visible to the rest of `app` so a dressed state can stand a render on
    /// disk where this says it goes, which is how a fixture reaches the adoption
    /// path without spending a minute of GPU on an encode.
    pub(super) fn bake_file(&self, clip: ClipId, pass: &BakePass) -> Result<PathBuf, String> {
        let project = self.project_path.as_ref().ok_or_else(|| {
            "Save the project first: a rendered clip is kept beside the project file.".to_string()
        })?;
        let stem = project
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .ok_or_else(|| "The project file has no name to render beside".to_string())?;
        let dir = project
            .parent()
            .ok_or_else(|| "The project file has no directory to render into".to_string())?
            .join(format!("{stem} Renders"));
        let rendered = pass
            .project
            .clip(clip)
            .ok_or_else(|| "That clip is no longer in the sequence".to_string())?;
        let mut hasher = Sha256::new();
        hasher.update(
            serde_json::to_vec(rendered)
                .map_err(|err| format!("reading the clip to render: {err}"))?,
        );
        hasher.update(
            serde_json::to_vec(&pass.project.timeline().canvas())
                .map_err(|err| format!("reading the canvas to render at: {err}"))?,
        );
        let rate = pass.project.timeline().frame_rate;
        hasher.update(format!("{}/{}", rate.num, rate.den).as_bytes());
        let key = format!("{:x}", hasher.finalize());
        Ok(dir.join(format!("{key}.{}", bake_extension(pass.medium))))
    }

    /// Import `dest` and swap it in for `clip`, as one history entry.
    fn adopt_bake(&mut self, clip: ClipId, dest: PathBuf) -> Result<(), String> {
        let engine = Arc::clone(&self.engine);
        adopt(&engine, &self.ctx, clip, dest)?;
        self.sync_from_engine();
        Ok(())
    }

    /// Render `clip` to `dest` as a job, and swap the file in when it is written.
    fn build_bake(&mut self, clip: ClipId, pass: BakePass, dest: PathBuf) -> Result<u64, String> {
        let medium = pass.medium;
        // A picture needs the platform encoder; a build without one cannot write
        // it. Refusing here names the reason, where spawning would fail minutes
        // into the render. Sound is written in Rust and needs nothing.
        if medium == BakeMedium::Picture && !aria_encoder::capability().available {
            return Err(
                "This build has no native encoder, so it cannot render a clip to a file.".into(),
            );
        }
        let settings = ExportSettings::for_project(&pass.project);
        let (width, height) = settings.size;
        let written = match medium {
            BakeMedium::Picture => format!("{width}×{height}"),
            BakeMedium::Sound => aria_encoder::AUDIO_CAPABILITY.label(),
        };
        // The clip the render is for, not whatever is selected: the assistant
        // names one by id, and the operator may have clicked elsewhere since.
        let name = self
            .ui_clip_ref(clip)
            .map(|clip| clip.name.clone())
            .unwrap_or_else(|| "the clip".into());
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("creating {}: {err}", parent.display()))?;
        }
        let engine = Arc::clone(&self.engine);
        let ctx = Arc::clone(&self.ctx);
        // Only a picture is composited, so only a picture claims a device: a
        // sound bake that opened one would hold GPU memory to write samples.
        let gpu = match medium {
            BakeMedium::Picture => Some(super::render_device::shared_device(&self.ctx)),
            BakeMedium::Sound => None,
        };
        let label = name.clone();
        let published = dest.display().to_string();
        self.status_message = Some(format!("Rendering {name} · {written}"));
        let BakePass { project, .. } = pass;
        let id = self.spawn_job(format!("Rendering {name}"), move |context| {
            let mut guard = super::jobs::CommitGuard::new(context, "the render");
            let mut observer = |done: u64, total: u64| {
                let fraction = if total == 0 {
                    0.0
                } else {
                    done as f32 / total as f32
                };
                context.set_progress(fraction, format!("frame {done}/{total}"));
                if done == total {
                    return guard.begin().is_ok();
                }
                !context.cancelled()
            };
            let frames = match gpu {
                Some(gpu) => {
                    let mut renderer = super::render_device::renderer_on(gpu, "the render")?;
                    aria_render::export_to_file_observed(
                        &mut renderer,
                        &project,
                        &dest,
                        settings,
                        &mut observer,
                    )
                }
                None => aria_render::export_audio_to_file_observed(
                    &project,
                    &dest,
                    settings,
                    &mut observer,
                ),
            }
            .map_err(|err| err.to_string())?;
            // The last act, on the shared engine: the file exists, so the
            // clip can decode it from here on. Swapping earlier would point
            // the renderer at a file that is still being written.
            adopt(&engine, &ctx, clip, dest)?;
            JobCompletion::new(format!("{label}: {frames} frame(s) at {written}"))
                .with_output("path", published)
                .map_err(String::from)
        });
        Ok(id.raw())
    }
}

/// The extension a bake's file carries, from whichever sink writes it.
///
/// One owner of the answer, so the name of the file and the muxer that produced
/// it cannot disagree.
fn bake_extension(medium: BakeMedium) -> &'static str {
    match medium {
        BakeMedium::Picture => aria_encoder::capability().extension,
        BakeMedium::Sound => aria_encoder::AUDIO_CAPABILITY.extension,
    }
}

/// Import the rendered file and swap it in, as one undo entry.
///
/// Free function rather than a method because the job owns the shared engine and
/// nothing else: the state it was spawned from may be rendering a frame. The
/// plugin context comes along because the two commands are still commands: a
/// plugin that rewrites or refuses an edit has to see the swap the same way it
/// sees a razor, and a surface census that watches proposals cannot see an edit
/// applied behind them.
fn adopt(
    engine: &Arc<parking_lot::Mutex<aria_engine::Engine>>,
    ctx: &PluginContext,
    clip: ClipId,
    dest: PathBuf,
) -> Result<(), String> {
    let import = proposed(ctx, Command::Project(ProjectCommand::Import { path: dest }))?;
    let mut engine = engine.lock();
    engine.begin_group();
    let outcome = match engine.apply(import) {
        Ok(outcome) => outcome,
        Err(err) => {
            engine.rollback_group();
            return Err(err.to_string());
        }
    };
    let ApplyOutcome::Imported { media } = outcome else {
        engine.rollback_group();
        return Err("importing the rendered file reported no pool item".into());
    };
    let swap = match proposed(ctx, Command::Edit(EditCommand::BakeClip { clip, media })) {
        Ok(swap) => swap,
        Err(err) => {
            engine.rollback_group();
            return Err(err);
        }
    };
    if let Err(err) = engine.apply(swap) {
        engine.rollback_group();
        return Err(err.to_string());
    }
    engine.commit_group();
    Ok(())
}

/// Put one command past the plugin boundary and report what the handlers left of
/// it, or why it must not be applied.
fn proposed(ctx: &PluginContext, command: Command) -> Result<Command, String> {
    let mut proposed = CommandProposed::new(command);
    ctx.events().emit_waterfall(&mut proposed);
    match proposed.veto {
        Some(reason) => Err(reason),
        None => Ok(proposed.command),
    }
}

#[cfg(test)]
#[path = "bake_tests.rs"]
mod render_and_replace;
