//! The UI's view of `aria-cloud`: what a browsable cloud surface looks like while
//! it loads, and how a provider is chosen.
//!
//! `aria-cloud` is blocking by design and its callers are expected to run it on
//! worker threads, so nothing here performs I/O. This module owns two things the
//! panels and the tests both need: the per-feed load state, and the BYOK routing
//! rule. Keeping the rule here means the sidebar cannot accidentally send a keyed
//! user through the backend, and a test can assert the routing without a socket.

use aria_cloud::dto::{AssetKind, CatalogEntry, CatalogResponse, StockItem, StockSearchResponse};
use gpui::SharedString;

/// A browsable cloud surface.
///
/// One enum for every feed, so a new surface has to be given a title, an endpoint,
/// and a card mapping before it compiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CloudFeed {
    /// Stock video, photo, and audio search.
    Stock,
    /// Template bundles.
    Templates,
    /// Sound effects.
    Sfx,
    /// Colour LUTs.
    Luts,
    /// Animated text presets.
    TextPresets,
    /// File-backed Lottie animations.
    Lottie,
    /// Assistant skill packs.
    Skills,
}

impl CloudFeed {
    /// Every feed, in the order the UI offers them.
    pub const ALL: [CloudFeed; 7] = [
        CloudFeed::Stock,
        CloudFeed::Templates,
        CloudFeed::Sfx,
        CloudFeed::Luts,
        CloudFeed::TextPresets,
        CloudFeed::Lottie,
        CloudFeed::Skills,
    ];

    /// The section title.
    pub fn label(self) -> &'static str {
        match self {
            CloudFeed::Stock => "Stock",
            CloudFeed::Templates => "Templates",
            CloudFeed::Sfx => "SFX",
            CloudFeed::Luts => "LUTs",
            CloudFeed::TextPresets => "Text presets",
            CloudFeed::Lottie => "Lottie",
            CloudFeed::Skills => "Skills",
        }
    }

    /// Whether the feed is driven by a search query rather than a fixed catalog.
    pub fn is_search(self) -> bool {
        matches!(self, CloudFeed::Stock)
    }

    /// The asset kind a catalog feed's entries must carry, or `None` for the search
    /// feed. Entries of another kind are dropped, because the server may add a kind
    /// this build does not render.
    pub fn asset_kind(self) -> Option<AssetKind> {
        match self {
            CloudFeed::Stock => None,
            CloudFeed::Templates => Some(AssetKind::Template),
            CloudFeed::Sfx => Some(AssetKind::Sfx),
            CloudFeed::Luts => Some(AssetKind::Lut),
            CloudFeed::TextPresets => Some(AssetKind::TextPreset),
            CloudFeed::Lottie => Some(AssetKind::Lottie),
            CloudFeed::Skills => Some(AssetKind::Skill),
        }
    }
}

/// One browsable cloud item, flattened for display.
///
/// The panels never see a DTO: a card is what both a catalog entry and a stock item
/// reduce to, so one row widget renders every feed.
#[derive(Debug, Clone, PartialEq)]
pub struct CloudCard {
    /// Provider- or catalog-scoped id.
    pub id: SharedString,
    pub name: SharedString,
    /// The one line under the name: size, duration, or licence.
    pub detail: SharedString,
    /// Direct CDN URL of the file to download.
    pub file_url: String,
    /// The extension the downloaded file should carry, derived from the URL, because
    /// the importer dispatches on it.
    pub extension: String,
    /// Attribution the licence requires keeping, empty when the licence needs none.
    pub attribution: String,
}

/// How a feed is doing.
///
/// `Failed` carries the sentence to show. A failed fetch is not an empty feed: an
/// empty `Ready` means the server has nothing, and conflating the two is how an
/// outage looks like an empty catalog.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FeedState {
    /// Never fetched.
    #[default]
    Idle,
    /// A fetch is in flight.
    Loading,
    Ready(Vec<CloudCard>),
    Failed(String),
}

impl FeedState {
    /// The cards to draw, empty unless the feed is ready.
    pub fn cards(&self) -> &[CloudCard] {
        match self {
            FeedState::Ready(cards) => cards,
            _ => &[],
        }
    }

    /// The line to show instead of cards, or `None` when cards are the answer.
    pub fn placeholder(&self) -> Option<String> {
        match self {
            FeedState::Idle => Some("Not loaded".into()),
            FeedState::Loading => Some("Loading…".into()),
            FeedState::Failed(err) => Some(err.clone()),
            FeedState::Ready(cards) if cards.is_empty() => Some("Nothing found".into()),
            FeedState::Ready(_) => None,
        }
    }
}

/// Where stock search is routed.
///
/// The BYOK rule: a configured stock key routes search straight at the provider, so
/// the backend never sees the query. Without a key only the anonymous surface is
/// available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockRoute {
    /// Locally configured Pexels / Pixabay keys.
    Direct,
    /// Anonymous, through the Aria backend.
    Backend,
}

/// Every `[providers.<name>]` key the app can use, and what it powers.
///
/// A settings surface cannot enumerate a `BTreeMap` of *configured* providers and
/// call that a list: the interesting case is the provider with no key, which is
/// exactly the one missing from the map. This table is the list of names that mean
/// something, so an unset one is shown as unset instead of being invisible.
pub const BYOK_PROVIDERS: [(&str, &str); 4] = [
    ("pexels", "Stock search"),
    ("pixabay", "Stock search"),
    ("elevenlabs", "Voice generation"),
    ("fal", "Image and video generation"),
];

/// The provider names `[providers.<name>]` uses for stock search.
pub const STOCK_PROVIDERS: [&str; 2] = ["pexels", "pixabay"];

/// Pick the stock route for a config.
///
/// Lives here rather than in the panel so the rule has one owner and one test.
pub fn stock_route(settings: &aria_settings::Settings) -> StockRoute {
    if STOCK_PROVIDERS
        .iter()
        .any(|name| settings.provider(name).is_configured())
    {
        StockRoute::Direct
    } else {
        StockRoute::Backend
    }
}

/// Flatten a catalog response into cards, dropping entries of another kind.
pub fn cards_from_catalog(feed: CloudFeed, response: &CatalogResponse) -> Vec<CloudCard> {
    let wanted = feed.asset_kind();
    response
        .entries
        .iter()
        .filter(|entry| wanted.is_none_or(|kind| entry.kind == kind))
        .map(card_from_entry)
        .collect()
}

fn card_from_entry(entry: &CatalogEntry) -> CloudCard {
    let mut detail = String::new();
    if let Some(seconds) = entry.duration_seconds {
        detail.push_str(&format!("{seconds:.0}s"));
    }
    if let Some(slots) = entry.slot_count {
        push_part(&mut detail, &format!("{slots} slot(s)"));
    }
    if entry.size_bytes > 0 {
        push_part(
            &mut detail,
            &crate::app::storage::format_bytes(entry.size_bytes),
        );
    }
    if !entry.license.is_empty() {
        push_part(&mut detail, &entry.license);
    }
    CloudCard {
        id: entry.id.clone().into(),
        name: entry.name.clone().into(),
        detail: detail.into(),
        extension: extension_of(&entry.file_url),
        file_url: entry.file_url.clone(),
        attribution: entry.author.clone(),
    }
}

/// Flatten a stock search response into cards, best file first.
///
/// An item with no downloadable file is dropped: a card that cannot be imported is
/// worse than one fewer result.
pub fn cards_from_stock(response: &StockSearchResponse) -> Vec<CloudCard> {
    response.items.iter().filter_map(card_from_stock).collect()
}

fn card_from_stock(item: &StockItem) -> Option<CloudCard> {
    let file = item.files.first()?;
    let mut detail = String::new();
    if let Some(seconds) = item.duration_seconds {
        detail.push_str(&format!("{seconds:.0}s"));
    }
    if item.width > 0 && item.height > 0 {
        push_part(&mut detail, &format!("{}×{}", item.width, item.height));
    }
    if !file.quality.is_empty() {
        push_part(&mut detail, &file.quality);
    }
    if !item.license.is_empty() {
        push_part(&mut detail, &item.license);
    }
    let name = if item.author.is_empty() {
        item.id.clone()
    } else {
        format!("{} — {}", item.author, item.id)
    };
    Some(CloudCard {
        id: item.id.clone().into(),
        name: name.into(),
        detail: detail.into(),
        extension: extension_of(&file.url),
        file_url: file.url.clone(),
        attribution: item.attribution.clone(),
    })
}

fn push_part(detail: &mut String, part: &str) {
    if !detail.is_empty() {
        detail.push_str(" · ");
    }
    detail.push_str(part);
}

/// The file extension a URL implies, lowercased, without the dot.
///
/// Query strings and fragments are stripped first, because a CDN URL routinely
/// carries both and `mp4?token=…` is not an extension the importer knows. Falls back
/// to `bin`, so a downloaded file always has one.
pub fn extension_of(url: &str) -> String {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .trim_end_matches('/');
    let last = path.rsplit('/').next().unwrap_or(path);
    match last.rsplit_once('.') {
        Some((_, ext))
            if !ext.is_empty() && ext.len() <= 8 && ext.chars().all(char::is_alphanumeric) =>
        {
            ext.to_ascii_lowercase()
        }
        _ => "bin".into(),
    }
}
