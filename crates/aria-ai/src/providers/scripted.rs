//! Deterministic provider double: canned turns, no network.
//!
//! The substrate for agent-loop tests and the eval harness — scripted
//! prompts run against a real engine in CI without a live model.

use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;

use crate::provider::{
    ChatProvider, ChatRequest, ChatTurn, Message, ProviderError, ProviderStreamEvent,
};

pub struct ScriptedProvider {
    turns: Mutex<std::vec::IntoIter<ChatTurn>>,
    /// Every request's messages, recorded for assertions.
    requests: Mutex<Vec<Vec<Message>>>,
    /// Tool names offered on each request, recorded for assertions.
    tools: Mutex<Vec<Vec<String>>>,
}

impl std::fmt::Debug for ScriptedProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedProvider").finish_non_exhaustive()
    }
}

impl ScriptedProvider {
    pub fn new(turns: Vec<ChatTurn>) -> Self {
        Self {
            turns: Mutex::new(turns.into_iter()),
            requests: Mutex::new(Vec::new()),
            tools: Mutex::new(Vec::new()),
        }
    }

    /// The message histories this provider was called with, in order.
    pub fn requests(&self) -> Vec<Vec<Message>> {
        self.requests.lock().clone()
    }

    /// The tool names offered on each request, in order.
    pub fn tool_names(&self) -> Vec<Vec<String>> {
        self.tools.lock().clone()
    }
}

/// Bytes per streamed delta. A real provider hands an answer over in pieces,
/// so the double does too: a cancellation that could only land between whole
/// turns would never exercise the mid-answer path the panel's Stop takes.
const CHUNK: usize = 16;

/// Emits `text` in [`CHUNK`]-sized deltas, checking `cancel` before each one.
/// Splits on a character boundary, so a delta is always valid `str`.
fn stream(
    text: &str,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(&str),
) -> Result<(), ProviderError> {
    let mut rest = text;
    while !rest.is_empty() {
        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }
        let end = rest.ceil_char_boundary(CHUNK);
        emit(&rest[..end]);
        rest = &rest[end..];
    }
    Ok(())
}

impl ChatProvider for ScriptedProvider {
    fn chat(
        &self,
        request: &ChatRequest<'_>,
        cancel: &AtomicBool,
        on_event: &mut dyn FnMut(ProviderStreamEvent<'_>),
    ) -> Result<ChatTurn, ProviderError> {
        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }
        self.requests.lock().push(request.messages.to_vec());
        let names = request.tools.iter().map(|t| t.name.clone()).collect();
        self.tools.lock().push(names);
        let turn = self.turns.lock().next().ok_or_else(|| {
            ProviderError::Protocol("scripted provider ran out of turns".to_string())
        })?;
        stream(&turn.reasoning_summary, cancel, &mut |delta| {
            on_event(ProviderStreamEvent::ReasoningSummaryDelta(delta))
        })?;
        stream(&turn.text, cancel, &mut |delta| {
            on_event(ProviderStreamEvent::TextDelta(delta))
        })?;
        Ok(turn)
    }
}
