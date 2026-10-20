//! Fetching and importing the cloud feeds.
//!
//! `aria-cloud` is blocking, so every call here runs on the background executor and
//! posts its result back into the feed map. Nothing waits on the UI thread and no
//! call is retried automatically: a failed feed shows the reason and Refetch
//! is pressed by hand, because a silent retry loop against a paid provider is worse than a
//! visible failure.
//!
//! The one rule the panels must not own is routing: a configured stock key means the
//! query goes straight to the provider and the backend never sees it. That decision
//! lives in [`crate::cloud::stock_route`].

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use aria_cloud::dto::StockKind;
use aria_cloud::{CloudClient, DirectStockProvider, StockProvider, stock::BackendStockProvider};
use gpui::Context;

use super::AppState;
use crate::cloud::{
    BYOK_PROVIDERS, CloudCard, CloudFeed, FeedState, STOCK_PROVIDERS, StockRoute,
    cards_from_catalog, cards_from_stock, stock_route,
};

/// One BYOK provider as a settings row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatus {
    /// The `[providers.<name>]` key.
    pub name: &'static str,
    /// What a key here unlocks.
    pub purpose: &'static str,
    /// Whether a key actually resolves — an `api_key_env` naming an unset variable is
    /// not a key, which is the case a presence check on the config alone gets wrong.
    pub configured: bool,
    /// Where the key comes from, for the row's second line.
    pub source: String,
}

/// The load state of every cloud feed.
pub type Feeds = BTreeMap<CloudFeed, FeedState>;

/// Where catalog ETags and downloaded assets live under the storage root.
const CATALOG_DIR: &str = "catalogs";

impl AppState {
    /// The feed's current state; `Idle` until it is first fetched.
    pub fn feed(&self, feed: CloudFeed) -> FeedState {
        self.cloud_feeds.get(&feed).cloned().unwrap_or_default()
    }

    /// Where stock search would be routed with the current config.
    pub fn stock_route(&self) -> StockRoute {
        stock_route(&self.cloud_settings())
    }

    /// The config that owns the cloud endpoint and the BYOK keys.
    pub(crate) fn cloud_settings(&self) -> aria_settings::Settings {
        aria_settings::load(&self.config_path()).unwrap_or_default()
    }

    fn cloud_client(&self) -> CloudClient {
        let settings = self.cloud_settings();
        let base = if settings.cloud.base_url.trim().is_empty() {
            aria_cloud::DEFAULT_BASE_URL.to_string()
        } else {
            settings.cloud.base_url
        };
        CloudClient::new(&base, Some(self.download_cache_root().join(CATALOG_DIR)))
    }

    /// The provider stock search should use, per the BYOK rule.
    fn stock_provider(&self) -> Box<dyn StockProvider> {
        let settings = self.cloud_settings();
        match stock_route(&settings) {
            StockRoute::Direct => Box::new(DirectStockProvider::new(
                settings.provider(STOCK_PROVIDERS[0]).resolve_key(),
                settings.provider(STOCK_PROVIDERS[1]).resolve_key(),
            )),
            StockRoute::Backend => Box::new(BackendStockProvider::new(self.cloud_client())),
        }
    }

    /// Every BYOK provider, whether it has a resolvable key, and where the key would
    /// come from.
    ///
    /// Enumerated from [`BYOK_PROVIDERS`], not from the config's map, so a provider
    /// with no key is listed as unset rather than being absent.
    pub fn provider_key_status(&self) -> Vec<ProviderStatus> {
        let settings = self.cloud_settings();
        BYOK_PROVIDERS
            .into_iter()
            .map(|(name, purpose)| {
                let provider = settings.provider(name);
                ProviderStatus {
                    name,
                    purpose,
                    configured: provider.resolve_key().is_some(),
                    source: match (&provider.api_key_env, &provider.api_key) {
                        (Some(var), _) if !var.trim().is_empty() => format!("${var}"),
                        (_, Some(key)) if !key.trim().is_empty() => "config.toml".to_string(),
                        _ => "not set".to_string(),
                    },
                }
            })
            .collect()
    }

    /// Ask the backend for the newest published version.
    ///
    /// The current version is compiled in, so the comparison is honest even when the
    /// check fails: a failure reports the failure and still names what is installed.
    pub fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        self.status_message = Some("Checking for updates…".into());
        let client = self.cloud_client();
        cx.spawn(async move |this, cx| {
            let found = crate::handoff::on_a_thread(
                cx.background_executor(),
                "aria-update-check",
                move || client.latest_version(),
            )
            .await;
            let _ = this.update(cx, |state, _| {
                let current = env!("CARGO_PKG_VERSION");
                match found {
                    Some(Ok(latest)) if latest.version == current => {
                        state.status_message = Some(format!("Up to date ({current})"));
                    }
                    Some(Ok(latest)) => {
                        state.status_message = Some(format!(
                            "Update available: {} (you have {current})",
                            latest.version
                        ));
                    }
                    Some(Err(err)) => {
                        state.report_error(format!("Update check failed: {err}"));
                        state.status_message = Some(format!("Installed version {current}"));
                    }
                    None => {
                        state.report_error("Update check failed: no answer");
                        state.status_message = Some(format!("Installed version {current}"));
                    }
                }
            });
        })
        .detach();
    }

    /// Fetch a catalog feed in the background.
    ///
    /// A feed already loading is left alone, so repeated clicks do not multiply
    /// requests. The search feed is refused here: it has no catalog endpoint, and
    /// silently fetching the wrong one would be a bug with no symptom.
    pub fn fetch_feed(&mut self, feed: CloudFeed, cx: &mut Context<Self>) {
        if feed.is_search() || self.feed(feed) == FeedState::Loading {
            return;
        }
        self.cloud_feeds.insert(feed, FeedState::Loading);
        let client = self.cloud_client();
        cx.spawn(async move |this, cx| {
            let fetched = crate::handoff::on_a_thread(
                cx.background_executor(),
                "aria-cloud-feed",
                move || match feed {
                    CloudFeed::Templates => client.templates(),
                    CloudFeed::Sfx => client.sfx(),
                    CloudFeed::Luts => client.luts(),
                    CloudFeed::TextPresets => client.text_presets(),
                    CloudFeed::Skills => client.skills(),
                    CloudFeed::Lottie => client.lottie(),
                    CloudFeed::Stock => unreachable!("refused above"),
                },
            )
            .await;
            let _ = this.update(cx, |state, _| {
                let next = match fetched {
                    Some(Ok(response)) => FeedState::Ready(cards_from_catalog(feed, &response)),
                    Some(Err(err)) => FeedState::Failed(format!("{}: {err}", feed.label())),
                    None => FeedState::Failed(format!("{}: no answer", feed.label())),
                };
                state.cloud_feeds.insert(feed, next);
            });
        })
        .detach();
    }

    /// Give the search box keyboard focus, or take it away.
    ///
    /// Focus is explicit because the shell routes bare letters to the tool shortcuts.
    /// Capturing them whenever the tab happens to be open would break V/B/H/Z.
    pub fn focus_stock_search(&mut self, focused: bool) {
        self.stock_search_focus = focused;
    }

    /// Append a typed character to the query.
    pub fn stock_query_push(&mut self, ch: char) {
        self.stock_query.push(ch);
    }

    /// Delete the last character of the query.
    pub fn stock_query_pop(&mut self) {
        self.stock_query.pop();
    }

    /// Choose the media kind stock search asks for.
    ///
    /// The pending results are dropped: they answer the previous kind, and leaving
    /// video clips under a "Photo" chip is worse than an empty panel.
    pub fn set_stock_kind(&mut self, kind: StockKind) {
        if self.stock_kind == kind {
            return;
        }
        self.stock_kind = kind;
        self.cloud_feeds.insert(CloudFeed::Stock, FeedState::Idle);
    }

    /// Run the search the box currently describes.
    pub fn run_stock_search(&mut self, cx: &mut Context<Self>) {
        let query = self.stock_query.clone();
        let kind = self.stock_kind;
        self.search_stock(&query, kind, cx);
    }

    /// Run a stock search in the background.
    ///
    /// An empty query clears the feed rather than searching for nothing, which every
    /// provider answers with its idea of "popular".
    pub fn search_stock(&mut self, query: &str, kind: StockKind, cx: &mut Context<Self>) {
        let query = query.trim().to_string();
        if query.is_empty() {
            self.cloud_feeds.insert(CloudFeed::Stock, FeedState::Idle);
            return;
        }
        self.cloud_feeds
            .insert(CloudFeed::Stock, FeedState::Loading);
        let provider = self.stock_provider();
        cx.spawn(async move |this, cx| {
            let found = crate::handoff::on_a_thread(
                cx.background_executor(),
                "aria-stock-search",
                move || provider.search(&query, kind, 1),
            )
            .await;
            let _ = this.update(cx, |state, _| {
                let next = match found {
                    Some(Ok(response)) => FeedState::Ready(cards_from_stock(&response)),
                    Some(Err(err)) => FeedState::Failed(format!("Stock search: {err}")),
                    None => FeedState::Failed("Stock search: no answer".into()),
                };
                state.cloud_feeds.insert(CloudFeed::Stock, next);
            });
        })
        .detach();
    }

    /// The cache path a card's file downloads to.
    ///
    /// Keyed by feed and id so the same asset is fetched once, and given the URL's
    /// extension because the importer dispatches on it.
    pub fn cloud_asset_path(
        &self,
        feed: CloudFeed,
        card: &CloudCard,
    ) -> Result<std::path::PathBuf, String> {
        let key = format!(
            "{}/{}.{}",
            feed.label().to_ascii_lowercase(),
            sanitized(&card.id),
            card.extension
        );
        self.download_cache
            .path_for(&key)
            .map_err(|err| format!("{key}: {err}"))
    }

    /// Download a card and take it into the session.
    ///
    /// The cached copy is reused when it is already there, and pinned for the session
    /// so clearing the cache cannot delete a file an open project references.
    ///
    /// The transfer runs as a job so it reports determinate progress and can be
    /// cancelled: a 400 MB stock clip over a slow link is exactly the wait that needs a
    /// number and a way out. The job's cancellation token is mirrored into the
    /// `Arc<AtomicBool>` the download takes, so neither side learns about the other.
    pub fn import_cloud_card(
        &mut self,
        feed: CloudFeed,
        card: CloudCard,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let dest = self.cloud_asset_path(feed, &card)?;
        if dest.exists() {
            return self.adopt_cloud_asset(feed, &dest, &card);
        }
        self.hold_download_guard();
        let url = card.file_url.clone();
        let target = dest.clone();
        let label = format!("Downloading {}", card.name);
        // The job reports its own outcome over a one-shot channel. Polling the registry
        // would spin a core for the length of the transfer; subscribing globally would
        // wake this closure for every unrelated job.
        let (wake, wakes) = crate::handoff::wakes::<Result<(), String>>();
        self.spawn_job(label, move |context| {
            let cancel = Arc::new(AtomicBool::new(false));
            let mirror = cancel.clone();
            let result = aria_cloud::download::download_to(&url, &target, &cancel, |progress| {
                if context.cancelled() {
                    mirror.store(true, Ordering::Relaxed);
                }
                // `total_bytes` is 0 when the CDN sent no Content-Length.
                match progress.total_bytes {
                    total if total > 0 => context.set_progress(
                        progress.bytes_downloaded as f32 / total as f32,
                        format!(
                            "{} of {}",
                            crate::app::storage::format_bytes(progress.bytes_downloaded),
                            crate::app::storage::format_bytes(total)
                        ),
                    ),
                    // A server that sends no length gives no denominator. Reporting a
                    // made-up fraction would be worse than reporting bytes.
                    _ => context.set_progress(
                        0.0,
                        crate::app::storage::format_bytes(progress.bytes_downloaded),
                    ),
                }
            });
            let outcome = match (&result, context.cancelled()) {
                (_, true) => Err(String::new()),
                (Ok(()), false) => Ok(()),
                (Err(err), false) => Err(err.to_string()),
            };
            wake.wake(outcome);
            result
                .map(|()| aria_jobs::JobCompletion::new("downloaded"))
                .map_err(|err| err.to_string())
        });
        cx.spawn(async move |this, cx| {
            let outcome = wakes.next_from_a_thread(cx.background_executor()).await;
            let _ = this.update(cx, |state, _| {
                // Released whichever way the transfer ended: a failed download that left
                // the guard held would lock Clear and Move for the rest of the session.
                state.release_download_guard();
                match outcome {
                    Some(Ok(())) => {
                        if let Err(err) = state.adopt_cloud_asset(feed, &dest, &card) {
                            state.report_error(err);
                        }
                    }
                    // An empty reason is the cancellation case: the cancel was
                    // requested, so it is a status line and not an error.
                    Some(Err(reason)) if reason.is_empty() => {
                        state.status_message = Some(format!("Cancelled {}", card.name));
                    }
                    Some(Err(reason)) => {
                        state.report_error(format!("{}: {reason}", card.name));
                    }
                    // The writer went with a panicking job, which the registry already
                    // recorded as Failed.
                    None => {
                        state.report_error(format!("{}: the download did not report", card.name))
                    }
                }
            });
        })
        .detach();
        Ok(())
    }

    /// Take a downloaded file into the session: pin it, then do whatever its feed
    /// means.
    ///
    /// One match, one owner. A LUT is not media and a template is not an import; a
    /// panel that decided this per surface would drift the moment a feed grew a second
    /// entry point.
    pub(crate) fn adopt_cloud_asset(
        &mut self,
        feed: CloudFeed,
        path: &std::path::Path,
        card: &CloudCard,
    ) -> Result<(), String> {
        // Pinning first: an adoption that lands after an eviction would reference a
        // file that is no longer there.
        self.download_cache
            .protect_path(path)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        match feed {
            CloudFeed::Stock | CloudFeed::Sfx => {
                self.import_media(path.to_path_buf())
                    .map_err(|err| format!("{}: {err:?}", card.name))?;
                self.status_message = Some(self.adopted_message("Imported", card));
            }
            CloudFeed::Luts => {
                if self.selected_clip().is_none() {
                    return Err(format!("{}: select a clip first", card.name));
                }
                self.set_selected_lut(path.to_string_lossy().to_string())
                    .map_err(|err| format!("{}: {err:?}", card.name))?;
                self.status_message = Some(self.adopted_message("Applied", card));
            }
            CloudFeed::Templates => {
                self.fill_and_apply_template(path.to_path_buf());
                self.status_message = Some(self.adopted_message("Applied", card));
            }
            CloudFeed::TextPresets => {
                self.apply_cloud_text_preset(path, card)?;
            }
            // A Lottie is neither media nor a treatment: it is a generated clip,
            // so adopting one places it at the playhead the way the generator
            // gallery places a sticker. Sized to the canvas, because the file's own
            // intrinsic size is inside the JSON and the renderer scales the layer
            // anyway.
            CloudFeed::Lottie => {
                let [width, height] = self.canvas_size();
                let generator = aria_models::Generator::lottie(
                    path.to_string_lossy().to_string(),
                    width,
                    height,
                );
                self.add_generated(generator)
                    .map_err(|err| format!("{}: {err:?}", card.name))?;
                self.status_message = Some(self.adopted_message("Placed", card));
            }
            // A skill is not media: it installs into the agent directory, where the
            // prompt builder already reads it.
            CloudFeed::Skills => {
                self.install_skill(path, card)?;
            }
        }
        Ok(())
    }

    /// A text-preset catalog entry is a *pack*, not one preset, so downloading it
    /// loads the pack and the Text tab lists what it contains. Applying one to a clip
    /// is a second, explicit click.
    fn apply_cloud_text_preset(
        &mut self,
        path: &std::path::Path,
        card: &CloudCard,
    ) -> Result<(), String> {
        let raw = std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", card.name))?;
        let pack: aria_cloud::dto::TextPresetCatalog =
            serde_json::from_str(&raw).map_err(|err| format!("{}: {err}", card.name))?;
        if pack.presets.is_empty() {
            return Err(format!("{}: the pack carries no presets", card.name));
        }
        let count = pack.presets.len();
        self.text_presets = pack.presets.into();
        self.status_message = Some(format!("Loaded {count} text preset(s) from {}", card.name));
        Ok(())
    }

    fn adopted_message(&self, verb: &str, card: &CloudCard) -> String {
        match card.attribution.is_empty() {
            true => format!("{verb} {}", card.name),
            false => format!("{verb} {} — {}", card.name, card.attribution),
        }
    }
}

/// A filename-safe form of a provider id. Provider ids are opaque and have carried
/// slashes and colons, which would escape the cache key or fail to open on Windows.
fn sanitized(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}
