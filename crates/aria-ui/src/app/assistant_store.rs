//! The conversation on disk, one file per project.
//!
//! A conversation is worth keeping for the same reason an undo history is: the
//! next prompt's answer depends on it, and closing a window is not a decision to
//! forget what was said. It is stored beside the app's other per-session data
//! rather than inside the project file, because a project is a document that
//! gets sent to somebody and a conversation is not.
//!
//! Two things are persisted and one is not:
//!
//! - **The transcript** is what the panel draws.
//! - **The provider history** is what the next prompt carries. Without it a
//!   reloaded conversation would look continuous and behave as if the model had
//!   never spoken.
//! - **Images are dropped.** `aria-ai` already strips them from history on every
//!   request, so persisting them would store megabytes the next prompt cannot
//!   use.
//!
//! The file is versioned and a version this build does not know is ignored
//! rather than guessed at: a mis-read conversation would be fed to a model as
//! fact.

use std::path::PathBuf;

use aria_ai::{Message, provider::ToolCall};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::AppState;
use crate::types::{AgentEntryKind, UiAgentEntry};

/// The format this build writes and reads.
const VERSION: u32 = 1;

/// Where conversations live under the data directory.
const DIR: &str = "conversations";

#[derive(Debug, Serialize, Deserialize)]
struct StoredConversation {
    version: u32,
    /// The project this conversation belongs to, so a hash collision or a
    /// hand-copied file is caught rather than served for the wrong project.
    project: String,
    entries: Vec<StoredEntry>,
    history: Vec<StoredMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    kind: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
enum StoredMessage {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: String,
        #[serde(default)]
        tool_calls: Vec<StoredToolCall>,
    },
    ToolResult {
        call_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredToolCall {
    id: String,
    name: String,
    arguments: serde_json::Value,
}

impl StoredMessage {
    fn from_message(message: &Message) -> Self {
        match message {
            Message::System { content } => Self::System {
                content: content.clone(),
            },
            Message::User { content, .. } => Self::User {
                content: content.clone(),
            },
            Message::Assistant {
                content,
                tool_calls,
            } => Self::Assistant {
                content: content.clone(),
                tool_calls: tool_calls
                    .iter()
                    .map(|call| StoredToolCall {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                    })
                    .collect(),
            },
            Message::ToolResult {
                call_id, content, ..
            } => Self::ToolResult {
                call_id: call_id.clone(),
                content: content.clone(),
            },
        }
    }

    fn into_message(self) -> Message {
        match self {
            Self::System { content } => Message::System { content },
            Self::User { content } => Message::User {
                content,
                images: Vec::new(),
            },
            Self::Assistant {
                content,
                tool_calls,
            } => Message::Assistant {
                content,
                tool_calls: tool_calls
                    .into_iter()
                    .map(|call| ToolCall {
                        id: call.id,
                        name: call.name,
                        arguments: call.arguments,
                    })
                    .collect(),
            },
            Self::ToolResult { call_id, content } => Message::ToolResult {
                call_id,
                content,
                images: Vec::new(),
            },
        }
    }
}

impl AppState {
    /// Where this project's conversation is kept, or `None` for a project that
    /// has never been saved.
    ///
    /// An unsaved project has no identity to key on: two untitled projects in one
    /// session would otherwise share one conversation. The transcript still works
    /// in memory; it starts being kept when the project is saved.
    pub fn conversation_path(&self) -> Option<PathBuf> {
        let project = self.project_path.as_ref()?;
        let mut hasher = Sha256::new();
        hasher.update(project.to_string_lossy().as_bytes());
        let key = format!("{:x}", hasher.finalize());
        Some(self.data_dir.join(DIR).join(format!("{key}.json")))
    }

    /// Write the conversation for the project as it now stands.
    ///
    /// An empty conversation deletes the file rather than writing an empty one,
    /// so clearing a conversation and closing the app is the same as never having
    /// had one.
    pub fn save_conversation(&self) -> Result<(), String> {
        let Some(path) = self.conversation_path() else {
            return Ok(());
        };
        let project = self
            .project_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        if self.transcript.is_empty() && self.assistant.history.is_empty() {
            match std::fs::remove_file(&path) {
                Ok(()) => return Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(err) => return Err(format!("{}: {err}", path.display())),
            }
        }
        let stored = StoredConversation {
            version: VERSION,
            project,
            entries: self
                .transcript
                .iter()
                .map(|entry| StoredEntry {
                    id: if entry.id.is_empty() {
                        None
                    } else {
                        Some(entry.id.to_string())
                    },
                    kind: entry.kind.key().to_string(),
                    content: entry.content.to_string(),
                })
                .collect(),
            history: self
                .assistant
                .history
                .iter()
                .map(StoredMessage::from_message)
                .collect(),
        };
        let text =
            serde_json::to_string_pretty(&stored).map_err(|err| format!("serialize: {err}"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        std::fs::write(&path, text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// Save the conversation, reporting a failure to the log only.
    ///
    /// Used where saving is a side effect of something else the operator asked
    /// for (finishing a turn, clearing the conversation). Losing a conversation
    /// is not worth interrupting that with a dialog, and the caller has no
    /// better answer than the log does.
    pub fn persist_conversation(&self) {
        if let Err(err) = self.save_conversation() {
            tracing::warn!(error = %err, "could not save the conversation");
        }
    }

    /// Read the conversation for the current project, replacing whatever is in
    /// memory. Missing, unreadable, or from a version this build does not know:
    /// the panel starts empty and says nothing, because a conversation is not the
    /// document and its loss is not an error worth a dialog.
    pub fn load_conversation(&mut self) {
        std::sync::Arc::make_mut(&mut self.transcript).clear();
        self.assistant.history.clear();
        self.assistant.plan = std::sync::Arc::default();
        self.assistant.skills_off.clear();
        let Some(path) = self.conversation_path() else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        let Ok(stored) = serde_json::from_str::<StoredConversation>(&text) else {
            tracing::warn!(path = %path.display(), "could not read the saved conversation");
            return;
        };
        if stored.version != VERSION {
            tracing::warn!(
                version = stored.version,
                "ignoring a conversation from another version"
            );
            return;
        }
        let rows = std::sync::Arc::make_mut(&mut self.transcript);
        for entry in stored.entries {
            let kind = match entry.kind.as_str() {
                // The first persisted transcript shape used the provider-facing
                // role name. Keep those entries when assigning their new IDs.
                "user" => AgentEntryKind::You,
                key => {
                    let Some(kind) = AgentEntryKind::from_key(key) else {
                        continue;
                    };
                    kind
                }
            };
            let id = match entry.id {
                Some(id) if !id.is_empty() => gpui::SharedString::from(id),
                _ => compact_id(rows.len() + 1),
            };
            rows.push(UiAgentEntry {
                id,
                kind,
                content: entry.content.into(),
                timestamp: 0.0,
            });
        }
        self.assistant.history = stored
            .history
            .into_iter()
            .map(StoredMessage::into_message)
            .collect();
    }
}
/// Pre-formatted static strings for warm per-entry message IDs (0..256),
/// returning static string slices without any heap or format allocation.
const STATIC_IDS: [&str; 256] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
    "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31", "32",
    "33", "34", "35", "36", "37", "38", "39", "40", "41", "42", "43", "44", "45", "46", "47", "48",
    "49", "50", "51", "52", "53", "54", "55", "56", "57", "58", "59", "60", "61", "62", "63", "64",
    "65", "66", "67", "68", "69", "70", "71", "72", "73", "74", "75", "76", "77", "78", "79", "80",
    "81", "82", "83", "84", "85", "86", "87", "88", "89", "90", "91", "92", "93", "94", "95", "96",
    "97", "98", "99", "100", "101", "102", "103", "104", "105", "106", "107", "108", "109", "110",
    "111", "112", "113", "114", "115", "116", "117", "118", "119", "120", "121", "122", "123",
    "124", "125", "126", "127", "128", "129", "130", "131", "132", "133", "134", "135", "136",
    "137", "138", "139", "140", "141", "142", "143", "144", "145", "146", "147", "148", "149",
    "150", "151", "152", "153", "154", "155", "156", "157", "158", "159", "160", "161", "162",
    "163", "164", "165", "166", "167", "168", "169", "170", "171", "172", "173", "174", "175",
    "176", "177", "178", "179", "180", "181", "182", "183", "184", "185", "186", "187", "188",
    "189", "190", "191", "192", "193", "194", "195", "196", "197", "198", "199", "200", "201",
    "202", "203", "204", "205", "206", "207", "208", "209", "210", "211", "212", "213", "214",
    "215", "216", "217", "218", "219", "220", "221", "222", "223", "224", "225", "226", "227",
    "228", "229", "230", "231", "232", "233", "234", "235", "236", "237", "238", "239", "240",
    "241", "242", "243", "244", "245", "246", "247", "248", "249", "250", "251", "252", "253",
    "254", "255",
];

/// Formats an integer index into a compact `SharedString` without allocating format string intermediates.
/// Warm indices (0..256) return pre-allocated static slices.
pub(crate) fn compact_id(n: usize) -> gpui::SharedString {
    if let Some(&s) = STATIC_IDS.get(n) {
        gpui::SharedString::new_static(s)
    } else {
        let mut buf = [0u8; 20];
        let mut val = n;
        let mut i = buf.len();
        if val == 0 {
            i -= 1;
            buf[i] = b'0';
        } else {
            while val > 0 {
                i -= 1;
                buf[i] = b'0' + (val % 10) as u8;
                val /= 10;
            }
        }
        std::str::from_utf8(&buf[i..]).unwrap_or("0").into()
    }
}
