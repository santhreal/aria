//! Shared HTTP client infrastructure for Aria.
//!
//! Provides a centralized [`HttpAgentBuilder`] that enforces standard transport
//! policies across all Aria HTTP callers:
//! - Standard connect timeout (10s)
//! - Standard read timeout (30s)
//! - Standard write timeout (30s)
//! - HTTPS-only enforcement by default to prevent accidental plaintext leaks
//! - Explicit caller registration ([`HttpCaller`]) to maintain an auditable census
//!   of every HTTP boundary in the codebase.

use std::time::Duration;

/// Standard connect timeout across all Aria HTTP clients (10 seconds).
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Standard read timeout across all Aria HTTP clients (30 seconds).
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Standard write timeout across all Aria HTTP clients (30 seconds).
pub const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(30);

macro_rules! http_callers {
    (
        $(
            $(#[$meta:meta])*
            $variant:ident => {
                crate_name: $crate_name:literal,
                target: $target:literal,
                source: $source:literal,
            }
        ),+ $(,)?
    ) => {
        /// Registered inventory of all HTTP client boundaries across
        /// `aria-cloud` and `aria-ai`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum HttpCaller {
            $(
                $(#[$meta])*
                $variant,
            )+
        }

        impl HttpCaller {
            /// All registered HTTP callers in the census.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub const fn id(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant),)+
                }
            }

            /// Owning crate of this caller.
            pub const fn crate_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $crate_name,)+
                }
            }

            /// Primary type or function representing the HTTP client boundary.
            pub const fn target_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $target,)+
                }
            }

            /// Source path relative to the `aria-cloud` manifest.
            pub const fn source_path(self) -> &'static str {
                match self {
                    $(Self::$variant => $source,)+
                }
            }
        }
    };
}

http_callers! {
    /// Anonymous backend API client.
    CloudClient => {
        crate_name: "aria-cloud",
        target: "CloudClient",
        source: "src/client.rs",
    },
    /// Direct CDN asset and media downloads.
    CloudDownload => {
        crate_name: "aria-cloud",
        target: "download_to",
        source: "src/download.rs",
    },
    /// BYOK fal.ai media generation.
    FalGeneration => {
        crate_name: "aria-cloud",
        target: "FalGenerationProvider",
        source: "src/generate.rs",
    },
    /// BYOK direct Pexels and Pixabay stock search.
    DirectStock => {
        crate_name: "aria-cloud",
        target: "DirectStockProvider",
        source: "src/stock.rs",
    },
    /// OpenAI-compatible Chat Completions endpoints.
    OpenAiCompat => {
        crate_name: "aria-ai",
        target: "OpenAiCompatProvider",
        source: "../aria-ai/src/providers/openai_compat.rs",
    },
    /// OpenAI Responses SSE API.
    OpenAiResponses => {
        crate_name: "aria-ai",
        target: "OpenAiResponsesProvider",
        source: "../aria-ai/src/providers/openai_responses.rs",
    },
    /// Verified Whisper model downloads.
    WhisperModelDownload => {
        crate_name: "aria-transcription",
        target: "HttpDownloader",
        source: "../aria-transcription/src/model/download.rs",
    },
}

/// Centralized builder for `ureq::Agent` instances.
///
/// Enforces standard transport timeouts and HTTPS-only defaults while allowing
/// targeted overrides (e.g. for local developer endpoints or custom whole-request deadlines).
#[derive(Debug, Clone)]
pub struct HttpAgentBuilder {
    caller: Option<HttpCaller>,
    connect_timeout: Duration,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    overall_timeout: Option<Duration>,
    https_only: bool,
}

impl HttpAgentBuilder {
    /// Creates the standard policy before a registered caller is attached.
    fn new() -> Self {
        Self {
            caller: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            read_timeout: Some(DEFAULT_READ_TIMEOUT),
            write_timeout: Some(DEFAULT_WRITE_TIMEOUT),
            overall_timeout: None,
            https_only: true,
        }
    }

    /// Associates this builder with a registered [`HttpCaller`].
    pub fn for_caller(caller: HttpCaller) -> Self {
        let mut builder = Self::new();
        builder.caller = Some(caller);
        builder
    }

    /// Sets the connect timeout.
    pub fn timeout_connect(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets the read timeout.
    pub fn timeout_read(mut self, timeout: Duration) -> Self {
        self.read_timeout = Some(timeout);
        self
    }

    /// Sets the write timeout.
    pub fn timeout_write(mut self, timeout: Duration) -> Self {
        self.write_timeout = Some(timeout);
        self
    }

    /// Sets the overall request timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.overall_timeout = Some(timeout);
        self
    }

    /// Enables or disables HTTPS-only enforcement.
    ///
    /// By default, HTTPS-only is enabled (`true`). It should only be disabled
    /// for local endpoints (e.g. Ollama or mock servers in tests).
    pub fn https_only(mut self, https_only: bool) -> Self {
        self.https_only = https_only;
        self
    }

    /// Allows plaintext only for loopback development endpoints. A remote
    /// `http://` endpoint remains rejected by the agent's HTTPS-only policy.
    pub fn for_url(mut self, url: &str) -> Self {
        let authority = url
            .strip_prefix("http://")
            .and_then(|rest| rest.split('/').next());
        let loopback = authority.is_some_and(|host| {
            host == "localhost"
                || host.starts_with("localhost:")
                || host == "127.0.0.1"
                || host.starts_with("127.0.0.1:")
                || host == "[::1]"
                || host.starts_with("[::1]:")
        });
        if loopback {
            self.https_only = false;
        }
        self
    }

    /// Builds the configured [`ureq::Agent`].
    pub fn build(&self) -> ureq::Agent {
        let mut builder = ureq::AgentBuilder::new()
            .timeout_connect(self.connect_timeout)
            .https_only(self.https_only);

        if let Some(read) = self.read_timeout {
            builder = builder.timeout_read(read);
        }
        if let Some(write) = self.write_timeout {
            builder = builder.timeout_write(write);
        }
        if let Some(overall) = self.overall_timeout {
            builder = builder.timeout(overall);
        }

        builder.build()
    }

    /// Returns the registered caller if set.
    pub fn caller(&self) -> Option<HttpCaller> {
        self.caller
    }

    /// Returns the configured connect timeout.
    pub fn get_connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    /// Returns the configured read timeout.
    pub fn get_read_timeout(&self) -> Option<Duration> {
        self.read_timeout
    }

    /// Returns the configured write timeout.
    pub fn get_write_timeout(&self) -> Option<Duration> {
        self.write_timeout
    }

    /// Returns the configured overall timeout.
    pub fn get_overall_timeout(&self) -> Option<Duration> {
        self.overall_timeout
    }

    /// Returns whether HTTPS-only is enforced.
    pub fn is_https_only(&self) -> bool {
        self.https_only
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::path::Path;

    #[test]
    fn default_builder_has_standard_timeouts_and_https_only() {
        let builder = HttpAgentBuilder::new();
        assert_eq!(builder.get_connect_timeout(), Duration::from_secs(10));
        assert_eq!(builder.get_read_timeout(), Some(Duration::from_secs(30)));
        assert_eq!(builder.get_write_timeout(), Some(Duration::from_secs(30)));
        assert_eq!(builder.get_overall_timeout(), None);
        assert!(builder.is_https_only());
    }

    #[test]
    fn for_url_allows_loopback_but_not_remote_plaintext() {
        assert!(
            HttpAgentBuilder::new()
                .for_url("https://api.aria.sh/v1")
                .is_https_only()
        );
        assert!(
            !HttpAgentBuilder::new()
                .for_url("http://localhost:11434/v1")
                .is_https_only()
        );
        assert!(
            !HttpAgentBuilder::new()
                .for_url("http://127.0.0.1:8080")
                .is_https_only()
        );
        assert!(
            HttpAgentBuilder::new()
                .for_url("http://example.com/v1")
                .is_https_only()
        );
    }

    #[test]
    fn https_only_rejects_http_requests() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let port = listener.local_addr().unwrap().port();

        // Standard builder is HTTPS-only
        let agent = HttpAgentBuilder::new().build();
        let res = agent.get(&format!("http://127.0.0.1:{port}")).call();
        assert!(
            res.is_err(),
            "HTTPS-only agent must reject plaintext HTTP URLs"
        );

        // Disabling HTTPS-only allows connecting
        let server = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
                let _ = stream.flush();
            }
        });

        let allowed_agent = HttpAgentBuilder::new().https_only(false).build();
        let allowed_res = allowed_agent
            .get(&format!("http://127.0.0.1:{port}"))
            .call();
        assert!(
            allowed_res.is_ok(),
            "Agent with https_only(false) must allow plaintext HTTP"
        );
        server.join().unwrap();
    }

    #[test]
    fn builder_customization_is_explicit() {
        let builder = HttpAgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(15))
            .timeout_write(Duration::from_secs(20))
            .timeout(Duration::from_secs(45))
            .https_only(false);

        assert_eq!(builder.get_connect_timeout(), Duration::from_secs(5));
        assert_eq!(builder.get_read_timeout(), Some(Duration::from_secs(15)));
        assert_eq!(builder.get_write_timeout(), Some(Duration::from_secs(20)));
        assert_eq!(builder.get_overall_timeout(), Some(Duration::from_secs(45)));
        assert!(!builder.is_https_only());
    }

    #[test]
    fn caller_census_registration_complete() {
        assert_eq!(HttpCaller::ALL.len(), 7);
        for caller in HttpCaller::ALL {
            assert!(!caller.crate_name().is_empty());
            assert!(!caller.target_name().is_empty());
        }

        let builder = HttpAgentBuilder::for_caller(HttpCaller::CloudClient);
        assert_eq!(builder.caller(), Some(HttpCaller::CloudClient));
    }

    #[test]
    fn census_no_adhoc_agent_builders_in_assigned_crates() {
        // Enforce that no production source file in aria-cloud, aria-ai, or
        // aria-transcription uses ureq::AgentBuilder directly.
        let manifests = [
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("aria-ai/src"),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("aria-transcription/src"),
        ];

        let mut scanned_files = 0;
        for src_dir in &manifests {
            if !src_dir.exists() {
                continue;
            }
            for entry in walk_rs_files(src_dir) {
                let path_str = entry.to_string_lossy();
                // http.rs is the sole place where ureq::AgentBuilder is legitimately used
                if path_str.ends_with("/http.rs") || path_str.ends_with("\\http.rs") {
                    continue;
                }

                let content = std::fs::read_to_string(&entry)
                    .unwrap_or_else(|e| panic!("failed to read {path_str}: {e}"));

                assert!(
                    !content.contains("ureq::AgentBuilder::new"),
                    "Found ad-hoc ureq::AgentBuilder in {path_str}. All HTTP agents must use HttpAgentBuilder."
                );
                assert!(
                    !content.contains("AgentBuilder::new"),
                    "Found ad-hoc AgentBuilder in {path_str}. All HTTP agents must use HttpAgentBuilder."
                );
                scanned_files += 1;
            }
        }
        assert!(
            scanned_files > 5,
            "Expected to scan multiple source files in census"
        );
    }

    #[test]
    fn census_all_registered_callers_use_their_declared_source() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        for caller in HttpCaller::ALL {
            let path = manifest.join(caller.source_path());
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            assert!(
                content.contains(caller.target_name()),
                "{} does not declare {}",
                path.display(),
                caller.target_name()
            );
            let registration = format!("HttpCaller::{}", caller.id());
            assert!(
                content.contains(&registration),
                "{} does not register through {registration}",
                path.display()
            );
        }
    }

    fn walk_rs_files(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut results = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    results.extend(walk_rs_files(&path));
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    results.push(path);
                }
            }
        }
        results
    }
}
