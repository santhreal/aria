//! The typed HTTP client for the cloud server's anonymous surface.
//!
//! Nothing here sends a token. Catalog responses are ETag-cached on disk so
//! Library sections repaint instantly offline and revalidate in the
//! background; callers run this on worker threads.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::dto::{CatalogResponse, StockKind, StockSearchResponse};
use crate::error::CloudError;

/// Blocking client over the cloud server's anonymous routes.
pub struct CloudClient {
    base_url: String,
    agent: ureq::Agent,
    /// Where ETag-validated catalog responses persist; `None` disables
    /// disk caching (tests).
    cache_dir: Option<PathBuf>,
}

/// Names the endpoint and whether responses persist. The `ureq::Agent` is left
/// out: its own `Debug` prints the whole transport configuration, which is
/// noise in a client trace and grows every time the agent gains a field.
impl std::fmt::Debug for CloudClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudClient")
            .field("base_url", &self.base_url)
            .field("cache_dir", &self.cache_dir)
            .finish_non_exhaustive()
    }
}

impl CloudClient {
    /// `base_url` is the server root, e.g. `https://cloud.example.com`; a
    /// trailing slash is dropped. An empty or blank `base_url` means no server:
    /// every request returns [`CloudError::NoServer`] and sends nothing.
    /// `cache_dir` is the app-data catalog cache (created on demand).
    pub fn new(base_url: &str, cache_dir: Option<PathBuf>) -> Self {
        let base_url = base_url.trim().trim_end_matches('/');
        Self {
            base_url: base_url.to_string(),
            agent: crate::http::HttpAgentBuilder::for_caller(crate::http::HttpCaller::CloudClient)
                .for_url(base_url)
                .timeout(Duration::from_secs(30))
                .build(),
            cache_dir,
        }
    }

    /// The full URL of `path` on the configured server, or
    /// [`CloudError::NoServer`] when none is set.
    fn url(&self, path: &str) -> Result<String, CloudError> {
        if self.base_url.is_empty() {
            return Err(CloudError::NoServer);
        }
        Ok(format!("{}{path}", self.base_url))
    }

    /// `GET /v1/stock/search` — normalized stock metadata. Not disk-cached
    /// (queries are ad hoc; the server holds the shared response cache).
    pub fn stock_search(
        &self,
        query: &str,
        kind: StockKind,
        page: u32,
    ) -> Result<StockSearchResponse, CloudError> {
        let url = self.url("/v1/stock/search")?;
        let page_string = page.to_string();
        let response = self
            .agent
            .get(&url)
            .query("q", query)
            .query("kind", kind.as_str())
            .query("page", &page_string)
            .call()
            .map_err(|e| CloudError::from_ureq(&url, e))?;
        parse_json(&url, response)
    }

    /// `GET /v1/templates` — the template gallery.
    pub fn templates(&self) -> Result<CatalogResponse, CloudError> {
        self.catalog("/v1/templates", "templates.json")
    }

    /// `GET /v1/assets/text-presets`.
    pub fn text_presets(&self) -> Result<CatalogResponse, CloudError> {
        self.catalog("/v1/assets/text-presets", "text-presets.json")
    }

    /// `GET /v1/assets/sfx`.
    pub fn sfx(&self) -> Result<CatalogResponse, CloudError> {
        self.catalog("/v1/assets/sfx", "sfx.json")
    }

    /// `GET /v1/assets/luts`.
    pub fn luts(&self) -> Result<CatalogResponse, CloudError> {
        self.catalog("/v1/assets/luts", "luts.json")
    }

    /// `GET /v1/assets/lottie` — file-backed Lottie animations.
    pub fn lottie(&self) -> Result<CatalogResponse, CloudError> {
        self.catalog("/v1/assets/lottie", "lottie.json")
    }

    /// `GET /v1/assets/skills` — agent skill packs.
    pub fn skills(&self) -> Result<CatalogResponse, CloudError> {
        self.catalog("/v1/assets/skills", "skills.json")
    }

    /// A catalog GET with ETag revalidation against the disk cache: 304
    /// (or any network failure with a cache on disk) serves the cached
    /// body, so browsing keeps working offline with stale-but-usable data.
    fn catalog(&self, path: &str, cache_name: &str) -> Result<CatalogResponse, CloudError> {
        let url = self.url(path)?;
        let cached = self.read_cache(cache_name);

        let mut request = self.agent.get(&url);
        if let Some((etag, _)) = &cached
            && !etag.is_empty()
        {
            request = request.set("If-None-Match", etag);
        }

        match request.call() {
            Ok(response) if response.status() == 304 => {
                // `If-None-Match` goes out only with a cached ETag, but a
                // server or a proxy can answer 304 to any request.
                let Some((_, body)) = cached else {
                    return Err(CloudError::Protocol(format!(
                        "{url}: 304 Not Modified with no cached copy of {cache_name}"
                    )));
                };
                serde_json::from_str(&body)
                    .map_err(|e| CloudError::Protocol(format!("{url}: stale cache: {e}")))
            }
            Ok(response) => {
                let etag = response.header("ETag").unwrap_or_default().to_string();
                let body = response
                    .into_string()
                    .map_err(|e| CloudError::Network(format!("{url}: reading body: {e}")))?;
                let parsed: CatalogResponse = serde_json::from_str(&body)
                    .map_err(|e| CloudError::Protocol(format!("{url}: {e}")))?;
                self.write_cache(cache_name, &etag, &body);
                Ok(parsed)
            }
            Err(err) => {
                // Offline (or server trouble) with a cache on disk: serve
                // stale rather than blank out the Library section.
                if let Some((_, body)) = cached
                    && let Ok(parsed) = serde_json::from_str(&body)
                {
                    tracing::debug!("serving stale catalog {cache_name} after: {err}");
                    return Ok(parsed);
                }
                Err(CloudError::from_ureq(&url, err))
            }
        }
    }

    /// Cached (etag, body) for a catalog, if present and well-formed.
    /// Format: first line the ETag (may be empty), rest the JSON body.
    fn read_cache(&self, name: &str) -> Option<(String, String)> {
        let path = self.cache_path(name)?;
        let raw = std::fs::read_to_string(path).ok()?;
        let (etag, body) = raw.split_once('\n')?;
        Some((etag.to_string(), body.to_string()))
    }

    fn write_cache(&self, name: &str, etag: &str, body: &str) {
        let Some(path) = self.cache_path(name) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Single-line ETag header then the body; atomic rename so a crash
        // can't leave a torn cache a later session would trust.
        let tmp = path.with_extension("tmp");
        let content = format!("{}\n{}", etag.replace('\n', ""), body);
        if std::fs::write(&tmp, content).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    fn cache_path(&self, name: &str) -> Option<PathBuf> {
        Some(self.cache_dir.as_ref()?.join("catalogs").join(name))
    }

    /// The configured backend base URL (for UI display).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The catalog cache directory, when disk caching is on.
    pub fn cache_dir(&self) -> Option<&Path> {
        self.cache_dir.as_deref()
    }
}

pub(crate) fn parse_json<T: serde::de::DeserializeOwned>(
    url: &str,
    response: ureq::Response,
) -> Result<T, CloudError> {
    let body = response
        .into_string()
        .map_err(|e| CloudError::Network(format!("{url}: reading body: {e}")))?;
    serde_json::from_str(&body).map_err(|e| CloudError::Protocol(format!("{url}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_write_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let client = CloudClient::new("https://example.invalid", Some(dir.path().to_path_buf()));
        client.write_cache("templates.json", "W/\"abc\"", r#"{"entries":[]}"#);
        let (etag, body) = client.read_cache("templates.json").expect("cached");
        assert_eq!(etag, "W/\"abc\"");
        assert_eq!(body, r#"{"entries":[]}"#);
    }

    #[test]
    fn offline_with_cache_serves_stale_catalog() {
        let dir = tempfile::tempdir().unwrap();
        // .invalid TLD guarantees resolution failure — the offline path.
        let client = CloudClient::new("https://example.invalid", Some(dir.path().to_path_buf()));
        client.write_cache("templates.json", "", r#"{"entries":[]}"#);
        let catalog = client.templates().expect("stale cache served offline");
        assert!(catalog.entries.is_empty());
    }

    #[test]
    fn offline_without_cache_is_a_network_error() {
        let dir = tempfile::tempdir().unwrap();
        let client = CloudClient::new("https://example.invalid", Some(dir.path().to_path_buf()));
        match client.templates() {
            Err(CloudError::Network(_)) => {}
            other => panic!("expected Network error, got {other:?}"),
        }
    }

    /// WHY: no cloud server is built in, so an empty `cloud.base_url` must stop
    /// every route before it builds a URL. Without the check the request goes to
    /// a host-less path and fails as `Network`, which tells the reader to check a
    /// connection that is fine; worse, a cached catalog from an earlier server
    /// would be served as if a server answered. Each route is called with a
    /// blank base and a cache holding that route's catalog.
    ///
    /// Not covered: a new public route added without going through `url`; the
    /// list below names the routes by hand.
    #[test]
    fn every_route_with_no_server_fails_with_no_server() {
        type Route = fn(&CloudClient) -> Result<(), CloudError>;
        let routes: [(&str, Route); 7] = [
            ("templates.json", |c| c.templates().map(drop)),
            ("text-presets.json", |c| c.text_presets().map(drop)),
            ("sfx.json", |c| c.sfx().map(drop)),
            ("luts.json", |c| c.luts().map(drop)),
            ("lottie.json", |c| c.lottie().map(drop)),
            ("skills.json", |c| c.skills().map(drop)),
            ("stock.json", |c| {
                c.stock_search("sea", StockKind::Video, 1).map(drop)
            }),
        ];
        let dir = tempfile::tempdir().unwrap();
        for base in ["", "   ", "/"] {
            let client = CloudClient::new(base, Some(dir.path().to_path_buf()));
            assert_eq!(client.base_url(), "", "base {base:?}");
            for (cache_name, route) in routes {
                client.write_cache(cache_name, "", r#"{"entries":[]}"#);
                match route(&client) {
                    Err(CloudError::NoServer) => {}
                    other => {
                        panic!("base {base:?}, {cache_name}: expected NoServer, got {other:?}")
                    }
                }
            }
        }
        assert_eq!(
            CloudError::NoServer.to_string(),
            "No cloud server is set. Enter its URL in Settings > Cloud > Cloud base URL."
        );
    }

    /// A 304 with no cached copy to serve is a protocol error naming the 304.
    ///
    /// The class closed here is a panic on a status the server picks. Cases
    /// cover a client with no cache directory and one whose cache holds no
    /// copy of the catalog. Not covered: a 304 that names an ETag other than
    /// the cached one, which serves the cached copy.
    #[test]
    fn not_modified_without_a_cached_copy_is_a_protocol_error() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let dir = tempfile::tempdir().unwrap();
        for cache_dir in [None, Some(dir.path().to_path_buf())] {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("the client connects");
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(b"HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\n\r\n");
                let _ = stream.flush();
            });
            let client = CloudClient::new(&format!("http://127.0.0.1:{port}"), cache_dir.clone());
            match client.templates() {
                Err(CloudError::Protocol(message)) => {
                    assert!(message.contains("304"), "{message}");
                }
                other => panic!("cache {cache_dir:?}: expected a protocol error, got {other:?}"),
            }
            server.join().unwrap();
        }
    }
}
