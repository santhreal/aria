//! One error type for everything this crate does; variants map to the UX
//! the Library needs (offline → placeholder, rate-limited → retry hint,
//! cancelled → silence).

/// Why a cloud operation failed.
#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    /// No cloud server is configured: `cloud.base_url` is empty and none is
    /// built in. Catalog and server-routed stock requests fail with this
    /// before any network traffic.
    #[error("No cloud server is set. Enter its URL in Settings > Cloud > Cloud base URL.")]
    NoServer,

    /// Could not reach the host at all (offline, DNS, refused). The Library
    /// sections degrade to their placeholders on this.
    #[error("Couldn't reach the server. Check your connection.")]
    Network(String),

    /// The server answered with an error status. `retryable` is true for
    /// 429/5xx so callers can back off instead of giving up.
    #[error("server returned {status}: {message}")]
    Status {
        status: u16,
        message: String,
        retryable: bool,
    },

    /// The response body didn't parse as the expected DTO — a contract
    /// drift or a broken proxy, never expected in normal operation.
    #[error("bad response: {0}")]
    Protocol(String),

    /// Local filesystem trouble (cache dir, downloads).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// The caller's cancel flag was set mid-operation.
    #[error("cancelled")]
    Cancelled,
}

impl CloudError {
    pub(crate) fn from_ureq(url: &str, err: ureq::Error) -> Self {
        match err {
            ureq::Error::Status(status, response) => {
                let message = response
                    .into_string()
                    .unwrap_or_else(|_| "<unreadable error body>".into());
                let message = crate::http::capped_body(message, 300);
                CloudError::Status {
                    status,
                    message,
                    retryable: status == 429 || status >= 500,
                }
            }
            ureq::Error::Transport(t) => CloudError::Network(format!("{url}: {t}")),
        }
    }
}
