//! What the turn keeps: image budgets, and the slice of the conversation the
//! session appends.
//!
//! Images are the expensive part of a vision turn, so both the per-result and
//! the whole-request budgets keep the newest and drop the rest in place, with a
//! labeled placeholder so the model knows something was there. Session history
//! is text only: raw bytes would bloat every later request and the persisted
//! file for a frame the agent can simply take again.

use crate::provider::{ImagePart, Message};

/// Bound a single extensible tool result before it reaches either the
/// transcript or the request history. Count and encoded-byte limits both keep
/// the newest attachments, matching the whole-request policy below.
pub(crate) fn enforce_tool_output_image_budget(
    content: &mut String,
    images: &mut Vec<ImagePart>,
    max_images: usize,
    max_bytes: usize,
) {
    let mut count = images.len();
    let mut bytes = images
        .iter()
        .map(|image| image.data.len())
        .fold(0usize, usize::saturating_add);
    let mut drop_count = 0usize;
    for image in images.iter() {
        if count <= max_images && bytes <= max_bytes {
            break;
        }
        count = count.saturating_sub(1);
        bytes = bytes.saturating_sub(image.data.len());
        drop_count += 1;
    }
    for dropped in images.drain(..drop_count) {
        content.push_str(&format!(
            "\n[image not attached because it exceeded the request budget: {}]",
            dropped.label
        ));
    }
}

/// Keep only the newest `max_images` images across the request; older
/// ones are dropped in place and noted with a text placeholder carrying
/// the label, so the model knows what it saw and can re-request it.
/// Newest-wins matches how the agent works with vision: screenshot, look,
/// act — a stale frame is cheaper to re-take than to carry.
pub(crate) fn enforce_image_budget(messages: &mut [Message], max_images: usize, max_bytes: usize) {
    let mut image_total: usize = messages.iter().map(image_count).sum();
    let mut byte_total: usize = messages
        .iter()
        .flat_map(message_images)
        .map(|image| image.data.len())
        .fold(0usize, usize::saturating_add);
    if image_total <= max_images && byte_total <= max_bytes {
        return;
    }

    // Oldest first. Count how much of each image vector to drain before
    // mutating it, keeping this O(number of images) rather than repeatedly
    // removing index zero.
    for message in messages.iter_mut() {
        if image_total <= max_images && byte_total <= max_bytes {
            break;
        }
        let Some((content, images)) = message.attachments_mut() else {
            continue;
        };
        let mut drop_count = 0usize;
        for image in images.iter() {
            if image_total <= max_images && byte_total <= max_bytes {
                break;
            }
            image_total = image_total.saturating_sub(1);
            byte_total = byte_total.saturating_sub(image.data.len());
            drop_count += 1;
        }
        for dropped in images.drain(..drop_count) {
            content.push_str(&format!("\n[image no longer attached: {}]", dropped.label));
        }
    }
}

pub(crate) fn image_count(message: &Message) -> usize {
    message.attachments().len()
}

pub(crate) fn message_images(message: &Message) -> &[ImagePart] {
    message.attachments()
}

/// Session history is text-only: raw image bytes would bloat every later
/// request and the persisted session file for no benefit — the agent can
/// always re-screenshot the *current* state. A labeled placeholder keeps
/// the narrative ("looked at the timeline here") without the payload.
pub(crate) fn strip_images(content: &mut String, images: &mut Vec<ImagePart>) {
    for image in images.drain(..) {
        content.push_str(&format!("\n[image: {}]", image.label));
    }
}

/// This turn's slice of the conversation (`messages[turn_start..]`: the
/// user prompt plus every assistant/tool message the loop appended), with
/// the final text answer added (it isn't pushed during the loop),
/// `describe_project` results collapsed to a placeholder, and images
/// stripped to labels (history is text-only). This is what the session
/// appends to its history so the next prompt remembers the turn.
pub(crate) fn collect_turn_messages(
    messages: Vec<Message>,
    turn_start: usize,
    describe_call_ids: &[String],
    final_text: &str,
) -> Vec<Message> {
    let mut turn: Vec<Message> = messages.into_iter().skip(turn_start).collect();
    for message in &mut turn {
        // A description of the project is the largest thing a turn carries and
        // the one thing the next prompt already has a fresher copy of, so the
        // result is collapsed to a pointer at it.
        if let Message::ToolResult {
            call_id, content, ..
        } = message
            && describe_call_ids.iter().any(|id| id == call_id)
        {
            *content =
                "(project state omitted — see the current state in the system message)".to_string();
        }
        if let Some((content, images)) = message.attachments_mut() {
            strip_images(content, images);
        }
    }
    if !final_text.is_empty() {
        turn.push(Message::Assistant {
            content: final_text.to_string(),
            tool_calls: Vec::new(),
        });
    }
    turn
}
