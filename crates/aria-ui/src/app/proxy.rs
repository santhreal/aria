//! Making a proxy for a pool item, and deciding which file that proxy is.
//!
//! The pipeline could always decode through a proxy — `Engine::set_media_proxy`
//! substitutes one per media id and drops the open decoders so no cursor keeps
//! reading the original — and the registered `proxies` cache could always be
//! measured, cleared, moved, and redirected from Settings. Nothing ever wrote a
//! proxy, so the whole path was a capability with no first step.
//!
//! Three rules this module keeps:
//!
//! - **One file per source, named by its content.** The proxy's name is the same
//!   digest of path, size, and modification time that analysis batches are keyed
//!   by, so editing the source silently orphans its proxy instead of decoding a
//!   picture the file no longer has, and reopening a project finds the proxy that
//!   was already built.
//! - **The proxy registers itself.** The job holds the shared engine and installs
//!   the substitution as its last act, so nothing has to poll for the file or
//!   carry a completion back into the state. The pass itself runs on its own
//!   renderer over a project of its own: a proxy takes minutes, and the preview
//!   cannot hold the engine lock for that.
//! - **Asking for a proxy that exists is instant.** One verb covers both, because
//!   "build one" and "use the one on disk" are the same request from the operator's
//!   side, and a render must never stat a directory to decide which row to draw.

use std::path::PathBuf;
use std::sync::Arc;

use aria_engine::{PROXY_MAX_EDGE, proxy_project};
use aria_jobs::JobCompletion;
use aria_models::MediaId;
use aria_render::ExportSettings;
use aria_storage::CacheId;

use super::AppState;
use super::analysis::content_key_for;

impl AppState {
    /// Where `media`'s proxy lives, or why it has no home.
    ///
    /// Two ways to have none: the `proxies` cache resolved to nothing (an
    /// override the layout refused), or the source file could not be keyed, which
    /// is the same thing as it not being readable.
    pub fn proxy_file(&self, media: MediaId) -> Result<PathBuf, String> {
        let source = self.media_path(media)?;
        let root = self
            .storage_layout()
            .resolve(CacheId::Proxies)
            .ok_or_else(|| "The proxies cache has no directory".to_string())?;
        let key = content_key_for(&source)?;
        Ok(root.join(format!("{key}.mp4")))
    }

    /// The pool file behind `media`, as the project records it.
    fn media_path(&self, media: MediaId) -> Result<PathBuf, String> {
        let engine = self.engine.lock();
        engine
            .project()
            .media(media)
            .map(|source| source.path.clone())
            .ok_or_else(|| "That item is no longer in the pool".to_string())
    }

    /// Decode `media` through a proxy: register the one on disk, or render it.
    ///
    /// Returns the job that is building it, or `None` when the proxy was already
    /// there and the substitution is in force by the time this returns.
    pub fn use_media_proxy(&mut self, media: MediaId) -> Result<Option<u64>, String> {
        if self.media_has_proxy(media) {
            return Err("That item already decodes through its proxy".into());
        }
        let dest = self.proxy_file(media)?;
        if dest.exists() {
            self.engine.lock().set_media_proxy(media, dest);
            self.status_message = Some("Decoding through the proxy on disk".into());
            self.sync_from_engine();
            return Ok(None);
        }
        self.build_media_proxy(media, dest).map(Some)
    }

    /// Render `media` to `dest` as a job, and install the substitution when the
    /// file is published.
    fn build_media_proxy(&mut self, media: MediaId, dest: PathBuf) -> Result<u64, String> {
        // A build with no encoder cannot write one. Refusing here names the
        // reason; spawning would fail minutes into the render.
        if !aria_encoder::capability().available {
            return Err("This build has no native encoder, so it cannot write a proxy.".into());
        }
        let (source, name) = {
            let engine = self.engine.lock();
            let source = engine
                .project()
                .media(media)
                .ok_or_else(|| "That item is no longer in the pool".to_string())?
                .clone();
            let name = super::jobs::file_label(&source.path);
            (source, name)
        };
        // Refused before the job exists: an audio-only source, a still, or a size
        // that is not a canvas has nothing to render, and a board entry that could
        // never have run is worse than a refusal on the status strip.
        let project = proxy_project(source, PROXY_MAX_EDGE).map_err(|err| err.to_string())?;
        let settings = ExportSettings::for_project(&project).without_audio();
        let (width, height) = settings.size;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("creating {}: {err}", parent.display()))?;
        }
        let engine = Arc::clone(&self.engine);
        let gpu = super::render_device::shared_device(&self.ctx);
        let id = self.spawn_job(format!("Proxying {name}"), move |context| {
            let mut renderer = super::render_device::renderer_on(gpu, "the proxy")?;
            let mut guard = super::jobs::CommitGuard::new(context, "the proxy");
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
            let frames = aria_render::export_to_file_observed(
                &mut renderer,
                &project,
                &dest,
                settings,
                &mut observer,
            )
            .map_err(|err| err.to_string())?;
            // The last act, on the shared engine: the file exists, so the
            // substitution is true from here on and the next frame decodes
            // through it. Registering earlier would point the renderer at a
            // file that is still being written.
            let published = dest.display().to_string();
            engine.lock().set_media_proxy(media, dest);
            JobCompletion::new(format!("{frames} frame(s) at {width}×{height}"))
                .with_output("path", published)
                .map_err(String::from)
        });
        self.status_message = Some(format!("Proxying {name} · {width} × {height}"));
        Ok(id.raw())
    }

    /// Build a proxy for every pool item that can have one and does not.
    ///
    /// Reports what it queued and what it refused as a count rather than as one
    /// message per item: a pool of forty stills would otherwise bury the answer
    /// under forty refusals that are all the same refusal.
    pub fn proxy_whole_pool(&mut self) {
        let items: Vec<u64> = self.store.media.iter().map(|item| item.id).collect();
        if items.is_empty() {
            self.status_message = Some("The pool is empty".into());
            return;
        }
        let mut queued = 0usize;
        let mut ready = 0usize;
        let mut refused = 0usize;
        for id in items {
            let media = MediaId::from_raw(id);
            if self.media_has_proxy(media) {
                ready += 1;
                continue;
            }
            match self.use_media_proxy(media) {
                Ok(Some(_)) => queued += 1,
                Ok(None) => ready += 1,
                Err(_) => refused += 1,
            }
        }
        self.status_message = Some(format!(
            "{queued} proxy job(s) started · {ready} already proxied · {refused} cannot be proxied"
        ));
    }

    /// Whether the Viewer decodes through proxies where they exist.
    pub fn proxies_on(&self) -> bool {
        self.ui_settings.use_proxies
    }

    /// Switch the Viewer between proxies and the original files, save the
    /// choice, and render the picture again from the files now in use. Export
    /// reads the originals either way.
    pub fn toggle_proxies(&mut self, cx: &mut gpui::Context<Self>) {
        let on = !self.ui_settings.use_proxies;
        self.ui_settings.use_proxies = on;
        self.engine.lock().set_preview_proxies(on);
        self.persist_settings_later(cx);
        // Frames rendered ahead from the other files are stale.
        self.picture_changed();
        if self.playback_cadence.is_none() {
            let state = cx.entity();
            cx.defer(move |app| super::spawn_preview(state, app));
        }
        self.status_message = Some(if on {
            "Viewer: proxies".to_string()
        } else {
            "Viewer: original media".to_string()
        });
        cx.notify();
    }
}

#[cfg(test)]
#[path = "proxy_tests.rs"]
mod proxy_tests;
