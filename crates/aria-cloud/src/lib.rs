//! Aria cloud client (see `docs/cloud-roadmap.md`).
//!
//! One crate makes every backend and provider HTTP request for the editor, shaped like
//! `aria-ai`: engine-free, blocking HTTP on worker threads, trait-based
//! so tests use scripted fakes. Responsibilities:
//!
//! - [`dto`]: the request/response types for every `aria-backend` route —
//!   **the contract source of truth**. The backend consumes this crate as a
//!   git dependency and its contract tests fail CI on drift. Everything is
//!   unknown-field tolerant (additive-only `/v1` evolution: old clients keep
//!   working).
//! - [`client`] / [`stock`]: the anonymous half — asset catalogs and stock
//!   search need no account. [`stock::StockProvider`] has two
//!   implementations: backend-routed (anonymous, server holds the provider
//!   keys) and direct Pexels/Pixabay (BYOK stock keys — then even search
//!   skips the backend).
//! - [`download`] / [`cache`]: media files never transit the backend; the
//!   client downloads **directly from provider CDNs** into a quota-managed
//!   cache (LRU eviction, clear-cache action) with atomic tmp-then-rename
//!   writes, progress callbacks, and cancellation.
//! - [`generate`]: BYOK fal.ai generation (image / video / TTS).
//!
//! **The routing rule** (BYOK): a user-configured provider key routes the
//! call direct to the provider; else only the anonymous surface (stock,
//! catalogs) is available.
//!
//! Invariants carried from the rest of the app: network stays off the UI
//! thread (callers run this on workers); the editor never blocks on the
//! backend (catalog fetches are background work, failures degrade to the
//! Library placeholders); BYOK keys never transit our servers.

pub mod cache;
pub mod client;
pub mod download;
pub mod dto;
pub mod error;
pub mod generate;
pub mod http;
pub mod stock;

pub use client::CloudClient;
pub use error::CloudError;
pub use http::{HttpAgentBuilder, HttpCaller};
pub use stock::{DirectStockProvider, StockProvider};

/// Default production backend base URL. Overridable via the `[cloud]`
/// table in `~/.aria/config.toml` (points at staging or a self-hosted
/// instance).
pub const DEFAULT_BASE_URL: &str = "https://api.aria.sh";

#[cfg(test)]
mod tests_debug_redaction {
    //! WHY: every type in this crate that holds a BYOK key is `Debug`, and a
    //! `Debug` impl reaches places a deliberate log call does not: a panic
    //! payload, an `unwrap` message, an error chain printed with `{:?}`. A
    //! derived impl on any of them would print the key verbatim. These format
    //! each key-bearing type with a sentinel and assert the sentinel is absent,
    //! so replacing a hand-written impl with `#[derive(Debug)]` goes red.
    //!
    //! It does not close the class by construction: a new key-bearing type added
    //! to this crate is not enumerated here, because Rust cannot list its own
    //! types at run time. `HttpCaller` is the registry that a new *caller* must
    //! join, and it is checked separately.

    use crate::generate::FalGenerationProvider;
    use crate::stock::DirectStockProvider;

    /// A value no legitimate field can contain, so a match is the key itself.
    const SENTINEL: &str = "sk-sentinel-9d4f2a1c-must-not-appear";

    #[track_caller]
    fn assert_redacted(what: &str, rendered: String) {
        assert!(
            !rendered.contains(SENTINEL),
            "{what} printed its key: {rendered}"
        );
        assert!(
            !rendered.is_empty(),
            "{what} rendered nothing, so the assertion above proves nothing"
        );
    }

    #[test]
    fn a_generation_provider_never_prints_its_key() {
        let provider = FalGenerationProvider::new(SENTINEL);
        assert_redacted("FalGenerationProvider", format!("{provider:?}"));
        assert_redacted("FalGenerationProvider (alt)", format!("{provider:#?}"));
    }

    #[test]
    fn a_direct_stock_provider_never_prints_either_key() {
        let both =
            DirectStockProvider::new(Some(SENTINEL.to_string()), Some(format!("{SENTINEL}-two")));
        assert_redacted("DirectStockProvider", format!("{both:?}"));
        assert_redacted("DirectStockProvider (alt)", format!("{both:#?}"));

        // A single configured key must still be reported as configured, so the
        // redaction cannot be mistaken for "no key".
        let pexels_only = DirectStockProvider::new(Some(SENTINEL.to_string()), None);
        let rendered = format!("{pexels_only:?}");
        assert_redacted("DirectStockProvider (one key)", rendered.clone());
        assert!(
            rendered.contains("pexels_key: true") && rendered.contains("pixabay_key: false"),
            "presence must still be legible: {rendered}"
        );
    }
}
