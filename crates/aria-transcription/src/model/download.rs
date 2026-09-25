use std::io::Read;
use std::time::Duration;

use super::catalog::{DownloadError, ModelSpec};

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_TOTAL_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// A readable stream returned by a [`ModelDownloader`].
pub type DownloadReader = Box<dyn Read + Send + 'static>;

/// An injectable source for model bytes.
///
/// Tests and embedding applications can provide deterministic readers without
/// network access. Integrity and length enforcement remain the model manager's
/// responsibility.
pub trait ModelDownloader: Send + Sync {
    /// Opens a streaming response for `spec`.
    ///
    /// The returned stream is consumed at most through the catalogued exact
    /// byte count plus one detecting read.
    fn download(&self, spec: &ModelSpec) -> Result<DownloadReader, DownloadError>;
}

impl<F> ModelDownloader for F
where
    F: Fn(&ModelSpec) -> Result<DownloadReader, DownloadError> + Send + Sync,
{
    fn download(&self, spec: &ModelSpec) -> Result<DownloadReader, DownloadError> {
        self(spec)
    }
}

/// Blocking HTTPS model downloader backed by `ureq`.
///
/// The default uses a 15-second connect timeout, a 30-second read timeout, and
/// a 30-minute whole-request deadline. Redirects are handled by `ureq`; the
/// final response must be a 2xx status.
#[derive(Debug, Clone)]
pub struct HttpDownloader {
    agent: ureq::Agent,
}

impl HttpDownloader {
    /// Creates a downloader with explicit non-zero timeout bounds.
    ///
    /// # Errors
    ///
    /// Returns [`DownloadError::InvalidTimeout`] if any timeout is zero.
    pub fn with_timeouts(
        connect_timeout: Duration,
        read_timeout: Duration,
        total_timeout: Duration,
    ) -> Result<Self, DownloadError> {
        if connect_timeout.is_zero() || read_timeout.is_zero() || total_timeout.is_zero() {
            return Err(DownloadError::InvalidTimeout);
        }

        let agent =
            aria_cloud::HttpAgentBuilder::for_caller(aria_cloud::HttpCaller::WhisperModelDownload)
                .timeout_connect(connect_timeout)
                .timeout_read(read_timeout)
                .timeout(total_timeout)
                .build();
        Ok(Self { agent })
    }
}

impl Default for HttpDownloader {
    fn default() -> Self {
        Self::with_timeouts(
            DEFAULT_CONNECT_TIMEOUT,
            DEFAULT_READ_TIMEOUT,
            DEFAULT_TOTAL_TIMEOUT,
        )
        .expect("default model download timeouts are non-zero")
    }
}

impl ModelDownloader for HttpDownloader {
    fn download(&self, spec: &ModelSpec) -> Result<DownloadReader, DownloadError> {
        let mut last_error = None;
        for url in spec.urls() {
            match self
                .agent
                .get(url)
                .set("Accept", "application/octet-stream")
                .set("Accept-Encoding", "identity")
                .set(
                    "User-Agent",
                    concat!("aria-transcription/", env!("CARGO_PKG_VERSION")),
                )
                .call()
            {
                Ok(response) if (200..300).contains(&response.status()) => {
                    return Ok(response.into_reader());
                }
                Ok(response) => {
                    last_error = Some(DownloadError::HttpStatus {
                        status: response.status(),
                    });
                }
                Err(ureq::Error::Status(status, _)) => {
                    last_error = Some(DownloadError::HttpStatus { status });
                }
                Err(ureq::Error::Transport(error)) => {
                    last_error = Some(DownloadError::Transport {
                        message: error.to_string(),
                    });
                }
            }
        }
        Err(last_error.unwrap_or(DownloadError::HttpStatus { status: 500 }))
    }
}

#[cfg(test)]
mod tests {
    use crate::model::catalog::WhisperModel;

    #[test]
    fn model_spec_urls_include_mirrors() {
        let spec = WhisperModel::BaseEn.spec();
        let urls: Vec<&'static str> = spec.urls().collect();
        assert!(urls.len() >= 2);
        assert_eq!(urls[0], spec.url());
        assert_eq!(&urls[1..], spec.mirrors());
    }
}
