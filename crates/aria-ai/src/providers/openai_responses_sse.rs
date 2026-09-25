//! The Responses stream: typed events in, one parsed response out.
//!
//! The transport next door decides what to send and what to do with the result;
//! this module owns the wire vocabulary — every event the API emits, the three
//! terminals it can end on, and how a run of argument deltas becomes a tool
//! call. Splitting it keeps the vocabulary readable as a list.

use super::*;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ResponsesEvent {
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta { delta: String },
    #[serde(rename = "response.reasoning_summary_text.delta")]
    ReasoningSummaryTextDelta { delta: String },
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        output_index: usize,
        item: serde_json::Value,
    },
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgumentsDelta { output_index: usize, delta: String },
    #[serde(rename = "response.function_call_arguments.done")]
    FunctionCallArgumentsDone {
        output_index: usize,
        arguments: String,
    },
    #[serde(rename = "response.output_item.done")]
    OutputItemDone {
        output_index: usize,
        item: serde_json::Value,
    },
    #[serde(rename = "response.completed")]
    Completed { response: ResponseEnvelope },
    #[serde(rename = "response.incomplete")]
    Incomplete { response: ResponseEnvelope },
    #[serde(rename = "response.failed")]
    Failed { response: ResponseEnvelope },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(other)]
    Other,
}

#[derive(Debug, Default, Deserialize)]
struct ResponseEnvelope {
    #[serde(default)]
    output: Vec<serde_json::Value>,
    #[serde(default)]
    error: Option<ResponseApiError>,
    #[serde(default)]
    incomplete_details: Option<IncompleteDetails>,
}

#[derive(Debug, Default, Deserialize)]
struct ResponseApiError {
    #[serde(default)]
    message: String,
}

#[derive(Debug, Default, Deserialize)]
struct IncompleteDetails {
    #[serde(default)]
    reason: String,
}

enum Terminal {
    Completed(ResponseEnvelope),
    Incomplete(ResponseEnvelope),
}

/// Parse typed Responses events while preserving separate answer and provider
/// reasoning-summary channels.
pub(super) fn consume_responses_sse(
    reader: impl Read,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(ProviderStreamEvent<'_>),
) -> Result<ParsedResponse, ProviderError> {
    let mut streamed_text = String::new();
    let mut streamed_reasoning = String::new();
    let mut output_items = BTreeMap::<usize, serde_json::Value>::new();
    let mut terminal = None;
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    while crate::providers::openai_compat::read_line_bounded(
        &mut reader,
        &mut line,
        crate::providers::openai_compat::MAX_SSE_LINE_BYTES,
    )?
    .is_some()
    {
        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            break;
        }
        let event: ResponsesEvent = serde_json::from_str(data).map_err(|error| {
            ProviderError::Protocol(format!("bad Responses SSE event: {error}: {data}"))
        })?;
        match event {
            ResponsesEvent::OutputTextDelta { delta } => {
                if !delta.is_empty() {
                    streamed_text.push_str(&delta);
                    on_event(ProviderStreamEvent::TextDelta(&delta));
                }
            }
            ResponsesEvent::ReasoningSummaryTextDelta { delta } => {
                if !delta.is_empty() {
                    streamed_reasoning.push_str(&delta);
                    on_event(ProviderStreamEvent::ReasoningSummaryDelta(&delta));
                }
            }
            ResponsesEvent::OutputItemAdded { output_index, item }
            | ResponsesEvent::OutputItemDone { output_index, item } => {
                output_items.insert(output_index, item);
            }
            ResponsesEvent::FunctionCallArgumentsDelta {
                output_index,
                delta,
            } => append_function_arguments(&mut output_items, output_index, &delta),
            ResponsesEvent::FunctionCallArgumentsDone {
                output_index,
                arguments,
            } => set_function_arguments(&mut output_items, output_index, arguments),
            ResponsesEvent::Completed { response } => {
                terminal = Some(Terminal::Completed(response));
                break;
            }
            ResponsesEvent::Incomplete { response } => {
                terminal = Some(Terminal::Incomplete(response));
                break;
            }
            ResponsesEvent::Failed { response } => {
                return Err(ProviderError::Protocol(format!(
                    "Responses request failed: {}",
                    response_error_message(&response)
                )));
            }
            ResponsesEvent::Error { message } => {
                return Err(ProviderError::Protocol(format!(
                    "Responses stream error: {message}"
                )));
            }
            ResponsesEvent::Other => {}
        }
    }

    if cancel.load(Ordering::Relaxed) {
        return Err(ProviderError::Cancelled);
    }
    let Some(terminal) = terminal else {
        return Err(ProviderError::Protocol(
            "Responses stream ended without a terminal event".to_string(),
        ));
    };
    let (envelope, finish) = match terminal {
        Terminal::Completed(response) => (response, FinishReason::Stop),
        Terminal::Incomplete(response) => {
            let reason = response
                .incomplete_details
                .as_ref()
                .map(|details| details.reason.as_str())
                .unwrap_or("unknown");
            if !matches!(reason, "max_output_tokens" | "max_tokens") {
                return Err(ProviderError::Protocol(format!(
                    "Responses request was incomplete: {reason}"
                )));
            }
            (response, FinishReason::Length)
        }
    };

    let output = if envelope.output.is_empty() {
        output_items.into_values().collect()
    } else {
        envelope.output
    };
    let final_text = output_text(&output);
    let text = if streamed_text.is_empty() {
        if !final_text.is_empty() {
            on_event(ProviderStreamEvent::TextDelta(&final_text));
        }
        final_text
    } else {
        streamed_text
    };
    let final_reasoning = reasoning_summary(&output);
    let reasoning_summary = if streamed_reasoning.is_empty() {
        if !final_reasoning.is_empty() {
            on_event(ProviderStreamEvent::ReasoningSummaryDelta(&final_reasoning));
        }
        final_reasoning
    } else {
        streamed_reasoning
    };
    let tool_calls = if finish == FinishReason::Stop {
        parse_function_calls(&output)?
    } else {
        Vec::new()
    };
    let finish = if !tool_calls.is_empty() {
        FinishReason::ToolCalls
    } else {
        finish
    };

    Ok(ParsedResponse {
        turn: ChatTurn {
            text,
            reasoning_summary,
            tool_calls,
            finish,
        },
        output,
    })
}

fn append_function_arguments(
    items: &mut BTreeMap<usize, serde_json::Value>,
    output_index: usize,
    delta: &str,
) {
    let item = items.entry(output_index).or_insert_with(|| {
        serde_json::json!({
            "type": "function_call",
            "arguments": "",
        })
    });
    let arguments = item
        .as_object_mut()
        .expect("internally-created output item is an object")
        .entry("arguments")
        .or_insert_with(|| serde_json::Value::String(String::new()));
    let combined = arguments
        .as_str()
        .map(|current| format!("{current}{delta}"))
        .unwrap_or_else(|| delta.to_string());
    *arguments = serde_json::Value::String(combined);
}

fn set_function_arguments(
    items: &mut BTreeMap<usize, serde_json::Value>,
    output_index: usize,
    arguments: String,
) {
    let item = items.entry(output_index).or_insert_with(|| {
        serde_json::json!({
            "type": "function_call",
        })
    });
    item["arguments"] = arguments.into();
}

fn output_text(output: &[serde_json::Value]) -> String {
    output
        .iter()
        .filter(|item| item["type"] == "message")
        .filter_map(|item| item["content"].as_array())
        .flatten()
        .filter(|part| part["type"] == "output_text")
        .filter_map(|part| part["text"].as_str())
        .collect()
}

fn reasoning_summary(output: &[serde_json::Value]) -> String {
    output
        .iter()
        .filter(|item| item["type"] == "reasoning")
        .filter_map(|item| item["summary"].as_array())
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .collect()
}

fn parse_function_calls(output: &[serde_json::Value]) -> Result<Vec<ToolCall>, ProviderError> {
    output
        .iter()
        .filter(|item| item["type"] == "function_call")
        .map(|item| {
            let id = item["call_id"]
                .as_str()
                .or_else(|| item["id"].as_str())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    ProviderError::Protocol(
                        "Responses function call is missing call_id".to_string(),
                    )
                })?;
            let name = item["name"]
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    ProviderError::Protocol(format!(
                        "Responses function call {id} is missing a name"
                    ))
                })?;
            let raw_arguments = item["arguments"].as_str().unwrap_or("");
            let arguments = if raw_arguments.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(raw_arguments).map_err(|error| {
                    ProviderError::Protocol(format!(
                        "Responses function call '{name}' has unparseable arguments: \
                         {error}: {raw_arguments}"
                    ))
                })?
            };
            Ok(ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments,
            })
        })
        .collect()
}

fn response_error_message(response: &ResponseEnvelope) -> String {
    response
        .error
        .as_ref()
        .map(|error| error.message.as_str())
        .filter(|message| !message.is_empty())
        .unwrap_or("unknown provider error")
        .to_string()
}
