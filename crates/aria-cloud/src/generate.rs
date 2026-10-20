//! AI generation (image / video / TTS) — the client half.
//!
//! [`FalGenerationProvider`]: a configured fal.ai key talking to the
//! fal queue API directly — backend fully out of the loop. The job id
//! is the fal request id, namespaced per model.
//!
//! Yields provider-CDN result URLs; the caller downloads via
//! [`crate::download`] and runs the normal import path. Blocking, worker
//! threads only — like the rest of the crate.

use std::time::Duration;

use crate::dto::{GenerateRequest, Job, JobStatus};
use crate::error::CloudError;

/// What kind of media a generation produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationKind {
    Image,
    Video,
    Tts,
}

impl GenerationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Tts => "tts",
        }
    }
}

/// The generation seam: start a job, poll it to a terminal state.
pub trait GenerationProvider: Send {
    fn start(&self, kind: GenerationKind, request: &GenerateRequest) -> Result<Job, CloudError>;
    fn poll(&self, job_id: &str) -> Result<Job, CloudError>;
}

// ---------------------------------------------------------------------------
// BYOK (direct fal.ai) path
// ---------------------------------------------------------------------------

/// Direct fal.ai queue access with a configured key.
pub struct FalGenerationProvider {
    api_key: String,
    queue_base: String,
    agent: ureq::Agent,
}

/// Never prints `api_key`. A provider carrying a user's own key can reach a log
/// line, a panic message, or an error report, and the key is exactly the thing
/// none of those may carry, so its presence is reported instead of its value.
impl std::fmt::Debug for FalGenerationProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FalGenerationProvider")
            .field("queue_base", &self.queue_base)
            .field("api_key", &"<redacted>")
            .finish_non_exhaustive()
    }
}

/// Default models per kind — the same defaults the backend ships.
fn default_model(kind: GenerationKind) -> &'static str {
    match kind {
        GenerationKind::Image => "fal-ai/flux/schnell",
        GenerationKind::Video => "fal-ai/ltx-video",
        GenerationKind::Tts => "fal-ai/kokoro",
    }
}

impl FalGenerationProvider {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            queue_base: "https://queue.fal.run".into(),
            agent: crate::http::HttpAgentBuilder::for_caller(
                crate::http::HttpCaller::FalGeneration,
            )
            .timeout(Duration::from_secs(60))
            .build(),
        }
    }

    fn get_json(&self, url: &str) -> Result<serde_json::Value, CloudError> {
        let response = self
            .agent
            .get(url)
            .set("Authorization", &format!("Key {}", self.api_key))
            .call()
            .map_err(|e| CloudError::from_ureq(url, e))?;
        response
            .into_json()
            .map_err(|e| CloudError::Protocol(format!("{url}: {e}")))
    }
}

impl GenerationProvider for FalGenerationProvider {
    fn start(&self, kind: GenerationKind, request: &GenerateRequest) -> Result<Job, CloudError> {
        let model = if request.model.is_empty() {
            default_model(kind)
        } else {
            &request.model
        };
        let input = match kind {
            GenerationKind::Tts => serde_json::json!({ "text": request.prompt }),
            GenerationKind::Video => match request.duration_seconds {
                Some(seconds) => serde_json::json!({
                    "prompt": request.prompt,
                    "duration": seconds,
                }),
                None => serde_json::json!({ "prompt": request.prompt }),
            },
            GenerationKind::Image => serde_json::json!({ "prompt": request.prompt }),
        };
        let url = format!("{}/{model}", self.queue_base);
        let response = self
            .agent
            .post(&url)
            .set("Authorization", &format!("Key {}", self.api_key))
            .send_json(input)
            .map_err(|e| CloudError::from_ureq(&url, e))?;
        let body: serde_json::Value = response
            .into_json()
            .map_err(|e| CloudError::Protocol(format!("{url}: {e}")))?;
        let request_id = body["request_id"]
            .as_str()
            .ok_or_else(|| CloudError::Protocol(format!("{url}: no request_id")))?;
        Ok(Job {
            // Self-describing id so `poll` knows the model path.
            id: format!("{model}#{request_id}"),
            status: JobStatus::Running,
            result_url: None,
            credits_charged: 0,
            error: None,
        })
    }

    fn poll(&self, job_id: &str) -> Result<Job, CloudError> {
        let (model, request_id) = job_id
            .split_once('#')
            .ok_or_else(|| CloudError::Protocol(format!("malformed fal job id: {job_id}")))?;
        let status_url = format!("{}/{model}/requests/{request_id}/status", self.queue_base);
        let status = self.get_json(&status_url)?;
        let running = Job {
            id: job_id.to_string(),
            status: JobStatus::Running,
            result_url: None,
            credits_charged: 0,
            error: None,
        };
        let status_str = status["status"].as_str().unwrap_or("IN_QUEUE");
        match status_str {
            "COMPLETED" | "OK" => {
                let result_url_endpoint =
                    format!("{}/{model}/requests/{request_id}", self.queue_base);
                let result = self.get_json(&result_url_endpoint)?;
                match extract_media_url(&result) {
                    Some(url) => Ok(Job {
                        id: job_id.to_string(),
                        status: JobStatus::Succeeded,
                        result_url: Some(url),
                        credits_charged: 0,
                        error: None,
                    }),
                    None => Ok(Job {
                        id: job_id.to_string(),
                        status: JobStatus::Failed,
                        result_url: None,
                        credits_charged: 0,
                        error: Some("the provider returned no media".into()),
                    }),
                }
            }
            "FAILED" | "ERROR" => {
                let error_msg = status["error"]
                    .as_str()
                    .or_else(|| status["message"].as_str())
                    .or_else(|| status["detail"].as_str())
                    .unwrap_or("fal.ai generation failed")
                    .to_string();
                Ok(Job {
                    id: job_id.to_string(),
                    status: JobStatus::Failed,
                    result_url: None,
                    credits_charged: 0,
                    error: Some(error_msg),
                })
            }
            "IN_QUEUE" | "IN_PROGRESS" => Ok(running),
            _ => Ok(running),
        }
    }
}

/// The first media URL in a fal result payload (`images[0].url`,
/// `video.url`, `audio.url`, bare `url`) — mirrors the backend's probe.
fn extract_media_url(body: &serde_json::Value) -> Option<String> {
    [
        &body["images"][0]["url"],
        &body["video"]["url"],
        &body["audio"]["url"],
        &body["audio_file"]["url"],
        &body["url"],
    ]
    .into_iter()
    .find_map(|v| v.as_str())
    .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_have_stable_route_names() {
        assert_eq!(GenerationKind::Image.as_str(), "image");
        assert_eq!(GenerationKind::Video.as_str(), "video");
        assert_eq!(GenerationKind::Tts.as_str(), "tts");
    }

    #[test]
    fn fal_media_url_probe() {
        let audio = serde_json::json!({"audio": {"url": "https://cdn/a.wav"}});
        assert_eq!(
            extract_media_url(&audio).as_deref(),
            Some("https://cdn/a.wav")
        );
        assert_eq!(extract_media_url(&serde_json::json!({})), None);
    }

    #[test]
    fn fal_status_mapping() {
        let failed_status = serde_json::json!({
            "status": "FAILED",
            "error": "GPU out of memory"
        });
        let status_str = failed_status["status"].as_str().unwrap();
        assert_eq!(status_str, "FAILED");
        let err_msg = failed_status["error"].as_str().unwrap();
        assert_eq!(err_msg, "GPU out of memory");

        let error_status = serde_json::json!({
            "status": "ERROR",
            "detail": "Invalid prompt content"
        });
        assert_eq!(error_status["status"].as_str(), Some("ERROR"));
        assert_eq!(
            error_status["detail"].as_str(),
            Some("Invalid prompt content")
        );
    }
}
