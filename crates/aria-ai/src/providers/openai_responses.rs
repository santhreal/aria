//! OpenAI Responses API provider for reasoning models with function tools.
//!
//! Requests are stateless (`store: false`). During one agent prompt, the
//! provider retains only the previous response's output items so encrypted
//! reasoning and function calls can be replayed with the corresponding tool
//! outputs. The desktop creates a fresh provider for every user prompt, and
//! terminal responses clear this in-memory state.

use std::collections::{BTreeMap, HashSet};
use std::io::{BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use aria_cloud::http::capped_body;
use base64::Engine as _;
use parking_lot::Mutex;
use serde::Deserialize;

use crate::provider::{
    ChatProvider, ChatRequest, ChatTurn, FinishReason, ImagePart, Message, ProviderError,
    ProviderStreamEvent, ToolCall,
};

use super::openai_compat::{retry_delay, retryable_status, sleep_unless_cancelled};

/// A Responses API transport. One instance is scoped to one agent prompt so
/// replay state cannot leak into persisted conversation history.
pub struct OpenAiResponsesProvider {
    base_url: String,
    model: String,
    api_key: Option<String>,
    reasoning_summary: bool,
    agent: ureq::Agent,
    replay: Mutex<Option<ReplayState>>,
    /// How long a liveness probe may take; see [`super::PROBE_TIMEOUT`]. The
    /// streaming request is deliberately unbounded, because a long answer is a
    /// long answer.
    probe_timeout: Duration,
}

#[derive(Clone)]
struct ReplayState {
    /// Messages through the user prompt and persisted text history. Items after
    /// this boundary belong to the current tool loop and are replayed exactly.
    base_message_count: usize,
    /// Number of provider-agnostic messages present before the response that
    /// produced the latest output. The agent appends its assistant turn here.
    request_message_count: usize,
    /// Exact Responses output and function-call-output items since the latest
    /// user message, including encrypted reasoning from every tool round.
    continuation: Vec<serde_json::Value>,
    call_ids: HashSet<String>,
}

#[derive(Debug)]
struct ParsedResponse {
    turn: ChatTurn,
    output: Vec<serde_json::Value>,
}

impl OpenAiResponsesProvider {
    pub fn new(
        base_url: &str,
        model: &str,
        api_key: Option<String>,
        reasoning_summary: bool,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key,
            reasoning_summary,
            agent: aria_cloud::http::HttpAgentBuilder::for_caller(
                aria_cloud::http::HttpCaller::OpenAiResponses,
            )
            .for_url(base_url)
            .build(),
            replay: Mutex::new(None),
            probe_timeout: super::PROBE_TIMEOUT,
        }
    }

    /// Bound the liveness probe differently. Present so the bound is a value
    /// rather than a hidden constant, and so a test can prove a probe ends.
    pub fn with_probe_timeout(mut self, timeout: Duration) -> Self {
        self.probe_timeout = timeout;
        self
    }

    /// The Responses protocol shares the provider's token-free `/models`
    /// liveness endpoint with Chat Completions.
    pub fn test_connection(&self) -> Result<String, ProviderError> {
        let url = format!("{}/models", self.base_url);
        let mut http = self.agent.get(&url).timeout(self.probe_timeout);
        if let Some(key) = &self.api_key {
            http = http.set("Authorization", &format!("Bearer {key}"));
        }
        match http.call() {
            Ok(response) => {
                let body = response
                    .into_string()
                    .map_err(|e| ProviderError::Network(format!("reading /models: {e}")))?;
                let parsed: serde_json::Value = serde_json::from_str(&body)
                    .map_err(|e| ProviderError::Protocol(format!("bad /models response: {e}")))?;
                let count = parsed["data"].as_array().map_or(0, Vec::len);
                Ok(match count {
                    0 => "Connected.".to_string(),
                    1 => "Connected · 1 model available.".to_string(),
                    n => format!("Connected · {n} models available."),
                })
            }
            Err(ureq::Error::Status(status, response)) => {
                let message = response
                    .into_string()
                    .unwrap_or_else(|_| "<unreadable error body>".to_string());
                Err(ProviderError::Provider {
                    status,
                    message: capped_body(message, 200),
                })
            }
            Err(ureq::Error::Transport(error)) => {
                Err(ProviderError::Network(format!("{url}: {error}")))
            }
        }
    }

    fn request_body(&self, request: &ChatRequest<'_>) -> serde_json::Value {
        let instructions = request
            .messages
            .iter()
            .filter_map(|message| match message {
                Message::System { content } => Some(content.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        // The guard is held across the match rather than cloning what it
        // protects: `ReplayState` owns the whole accumulated continuation, so
        // the clone this replaces copied every prior turn's response items on
        // every request. Nothing under this lock does I/O or re-locks.
        let input = {
            let replay = self.replay.lock();
            match replay
                .as_ref()
                .filter(|state| replay_matches(state, request.messages))
            {
                Some(state) => replay_input(request.messages, state),
                None => to_responses_input(request.messages),
            }
        };
        let tools = request
            .tools
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "type": "function",
                    "name": &tool.name,
                    "description": &tool.description,
                    "parameters": &tool.parameters,
                    "strict": false,
                })
            })
            .collect::<Vec<_>>();

        let mut body = serde_json::json!({
            "model": self.model,
            "stream": true,
            "store": false,
            "instructions": instructions,
            "input": input,
            "include": ["reasoning.encrypted_content"],
        });
        if !tools.is_empty() {
            body["tools"] = tools.into();
        }
        if self.reasoning_summary {
            body["reasoning"] = serde_json::json!({ "summary": "auto" });
        }
        body
    }

    pub const MAX_REPLAY_ITEMS: usize = 64;
    pub const MAX_REPLAY_BYTES: usize = 1024 * 1024;

    fn prune_continuation(continuation: &mut Vec<serde_json::Value>) {
        if continuation.len() > Self::MAX_REPLAY_ITEMS {
            let drain_count = continuation.len() - Self::MAX_REPLAY_ITEMS;
            continuation.drain(0..drain_count);
        }
        let mut total_bytes: usize = continuation.iter().map(|v| v.to_string().len()).sum();
        while total_bytes > Self::MAX_REPLAY_BYTES && continuation.len() > 1 {
            let removed = continuation.remove(0);
            total_bytes = total_bytes.saturating_sub(removed.to_string().len());
        }
    }

    fn update_replay(&self, messages: &[Message], turn: &ChatTurn, output: Vec<serde_json::Value>) {
        let mut replay = self.replay.lock();
        if turn.finish == FinishReason::ToolCalls && !turn.tool_calls.is_empty() {
            let (base_message_count, mut continuation) = match replay.take() {
                Some(state) if replay_matches(&state, messages) => {
                    let tool_outputs = replay_tool_outputs(messages, &state);
                    let base_message_count = state.base_message_count;
                    let mut continuation = state.continuation;
                    continuation.extend(tool_outputs);
                    (base_message_count, continuation)
                }
                _ => (messages.len(), Vec::new()),
            };
            continuation.extend(output);
            Self::prune_continuation(&mut continuation);
            *replay = Some(ReplayState {
                base_message_count,
                request_message_count: messages.len(),
                continuation,
                call_ids: turn.tool_calls.iter().map(|call| call.id.clone()).collect(),
            });
        } else {
            *replay = None;
        }
    }

    fn clear_replay(&self) {
        *self.replay.lock() = None;
    }
}

impl ChatProvider for OpenAiResponsesProvider {
    fn chat(
        &self,
        request: &ChatRequest<'_>,
        cancel: &AtomicBool,
        on_event: &mut dyn FnMut(ProviderStreamEvent<'_>),
    ) -> Result<ChatTurn, ProviderError> {
        let url = format!("{}/responses", self.base_url);
        let body = self.request_body(request).to_string();

        // Match Chat Completions: retry only while sending the initial
        // request. Once an SSE reader exists, emitted deltas are never replayed.
        let mut attempt = 0usize;
        let response = loop {
            if cancel.load(Ordering::Relaxed) {
                self.clear_replay();
                return Err(ProviderError::Cancelled);
            }
            let mut http = self
                .agent
                .post(&url)
                .set("Content-Type", "application/json")
                .set("Accept", "text/event-stream");
            if let Some(key) = &self.api_key {
                http = http.set("Authorization", &format!("Bearer {key}"));
            }
            match http.send_string(&body) {
                Ok(response) => break response,
                Err(ureq::Error::Status(status, response)) => {
                    if retryable_status(status) {
                        match retry_delay(attempt) {
                            Some(delay) if sleep_unless_cancelled(delay, cancel) => {
                                attempt += 1;
                                continue;
                            }
                            Some(_) => {
                                self.clear_replay();
                                return Err(ProviderError::Cancelled);
                            }
                            None => {}
                        }
                    }
                    let message = response
                        .into_string()
                        .unwrap_or_else(|_| "<unreadable error body>".to_string());
                    self.clear_replay();
                    return Err(ProviderError::Provider {
                        status,
                        message: capped_body(message, 500),
                    });
                }
                Err(ureq::Error::Transport(error)) => match retry_delay(attempt) {
                    Some(delay) if sleep_unless_cancelled(delay, cancel) => attempt += 1,
                    Some(_) => {
                        self.clear_replay();
                        return Err(ProviderError::Cancelled);
                    }
                    None => {
                        self.clear_replay();
                        return Err(ProviderError::Network(format!("{url}: {error}")));
                    }
                },
            }
        };

        match consume_responses_sse(response.into_reader(), cancel, on_event) {
            Ok(parsed) => {
                self.update_replay(request.messages, &parsed.turn, parsed.output);
                Ok(parsed.turn)
            }
            Err(error) => {
                self.clear_replay();
                Err(error)
            }
        }
    }
}

fn replay_matches(state: &ReplayState, messages: &[Message]) -> bool {
    let Some(Message::Assistant { tool_calls, .. }) = messages.get(state.request_message_count)
    else {
        return false;
    };
    let assistant_ids = tool_calls
        .iter()
        .map(|call| call.id.as_str())
        .collect::<HashSet<_>>();
    let result_ids = messages[state.request_message_count + 1..]
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    assistant_ids.len() == state.call_ids.len()
        && state
            .call_ids
            .iter()
            .all(|id| assistant_ids.contains(id.as_str()))
        && state
            .call_ids
            .iter()
            .all(|id| result_ids.contains(id.as_str()))
}

fn replay_input(messages: &[Message], state: &ReplayState) -> Vec<serde_json::Value> {
    let mut input = to_responses_input(&messages[..state.base_message_count]);
    input.extend(state.continuation.iter().cloned());
    input.extend(replay_tool_outputs(messages, state));
    input
}

fn replay_tool_outputs(messages: &[Message], state: &ReplayState) -> Vec<serde_json::Value> {
    messages[state.request_message_count + 1..]
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult { call_id, .. } if state.call_ids.contains(call_id) => {
                Some(tool_result_input(message))
            }
            _ => None,
        })
        .collect()
}

fn to_responses_input(messages: &[Message]) -> Vec<serde_json::Value> {
    let mut input = Vec::new();
    for message in messages {
        match message {
            Message::System { .. } => {}
            Message::User { content, images } => {
                input.push(user_input_message(content, images));
            }
            Message::Assistant {
                content,
                tool_calls,
            } => {
                if !content.is_empty() {
                    input.push(assistant_output_message(content));
                }
                input.extend(tool_calls.iter().map(|call| {
                    serde_json::json!({
                        "type": "function_call",
                        "call_id": call.id,
                        "name": call.name,
                        "arguments": call.arguments.to_string(),
                    })
                }));
            }
            Message::ToolResult { .. } => input.push(tool_result_input(message)),
        }
    }
    input
}

fn user_input_message(content: &str, images: &[ImagePart]) -> serde_json::Value {
    let mut parts = vec![serde_json::json!({
        "type": "input_text",
        "text": content,
    })];
    parts.extend(images.iter().map(input_image_part));
    serde_json::json!({
        "role": "user",
        "content": parts,
    })
}

fn assistant_output_message(content: &str) -> serde_json::Value {
    serde_json::json!({
        "role": "assistant",
        "content": [{
            "type": "output_text",
            "text": content,
        }],
    })
}

fn tool_result_input(message: &Message) -> serde_json::Value {
    let Message::ToolResult {
        call_id,
        content,
        images,
    } = message
    else {
        unreachable!("tool_result_input requires a tool result");
    };
    if images.is_empty() {
        return serde_json::json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": content,
        });
    }

    let labels = images
        .iter()
        .map(|image| image.label.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut output = vec![serde_json::json!({
        "type": "input_text",
        "text": format!("{content}\n[attachments: {labels}]"),
    })];
    output.extend(images.iter().map(input_image_part));
    serde_json::json!({
        "type": "function_call_output",
        "call_id": call_id,
        "output": output,
    })
}

fn input_image_part(image: &ImagePart) -> serde_json::Value {
    let encoded = base64::engine::general_purpose::STANDARD.encode(image.data.as_slice());
    serde_json::json!({
        "type": "input_image",
        "image_url": format!("data:{};base64,{encoded}", image.media_type),
        "detail": "auto",
    })
}

#[path = "openai_responses_sse.rs"]
mod sse;

use sse::consume_responses_sse;

#[cfg(test)]
#[path = "openai_responses_tests.rs"]
mod stream_tests;
