//! A complete external plugin, built as a `cdylib` over `aria-sdk` and loaded from
//! a package directory (`plugin.toml` + `lib/<os>-<arch>.<ext>`).
//!
//! Build it and place the package in `<data dir>/plugins/example.notes`:
//!
//! ```text
//! cargo build -p aria-notes-plugin --release
//! mkdir -p ~/.aria/plugins/example.notes/lib
//! cp examples/notes-plugin/plugin.toml ~/.aria/plugins/example.notes/
//! cp target/release/libaria_notes_plugin.so ~/.aria/plugins/example.notes/lib/linux-x86_64.so
//! ```

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use aria_sdk::{
    DropAction, EventResponse, Host, LogLevel, Panel, PanelEvent, PanelLocation, PanelSpec, Plugin,
    UiNode,
};
use serde_json::Value;

/// Identifier of the sidebar panel registered by [`NotesPlugin`].
pub const NOTES_PANEL_ID: &str = "notes.panel";
/// Counts the commands this plugin saw.
static SEEN: AtomicUsize = AtomicUsize::new(0);

/// Number of `command-proposed` events observed by this plugin in this process.
pub fn seen_commands() -> usize {
    SEEN.load(Ordering::Relaxed)
}

struct NotesPanel {
    count: AtomicUsize,
    claim_srt: AtomicBool,
    mode: AtomicUsize,
}

impl NotesPanel {
    const fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
            claim_srt: AtomicBool::new(true),
            mode: AtomicUsize::new(0),
        }
    }
}

impl Panel for NotesPanel {
    fn view(&self, host: &Host) -> UiNode {
        match self.mode.swap(0, Ordering::AcqRel) {
            1 => panic!("notes panel view panic"),
            2 => {
                return UiNode::column([
                    UiNode::button("dup", "First"),
                    UiNode::button("dup", "Second"),
                ]);
            }
            _ => {}
        }
        let count = self.count.load(Ordering::Acquire);
        let claim_srt = self.claim_srt.load(Ordering::Acquire);
        let draft = host
            .storage_get("draft")
            .ok()
            .flatten()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_else(|| "ready".to_owned());
        UiNode::column([
            UiNode::heading(format!("Notes ({count})")),
            UiNode::button("add", format!("Add note ({count})")),
            UiNode::toggle("claim_srt", "Claim .srt files", claim_srt),
            UiNode::input("draft", draft, "Write a note"),
        ])
    }

    fn event(&self, host: &Host, event: &PanelEvent) -> Result<(), String> {
        match event {
            PanelEvent::Pressed { id } if id.as_ref() == "add" => {
                self.count.fetch_add(1, Ordering::AcqRel);
                Ok(())
            }
            PanelEvent::Toggled { id, on } if id.as_ref() == "claim_srt" => {
                self.claim_srt.store(*on, Ordering::Release);
                Ok(())
            }
            PanelEvent::Submitted { id, text } if id.as_ref() == "draft" => match text.as_ref() {
                "PANIC_VIEW" => {
                    self.mode.store(1, Ordering::Release);
                    Ok(())
                }
                "REFUSE_VIEW" => {
                    self.mode.store(2, Ordering::Release);
                    Ok(())
                }
                "PANIC_EVENT" => panic!("notes panel event panic"),
                "ERROR_EVENT" => Err("note rejected".to_owned()),
                _ => host.storage_set("draft", text.as_bytes()),
            },
            other => Err(format!("unexpected control event on {}", other.id())),
        }
    }
}

/// External plugin that claims `.srt` dropped paths and refuses edits touching
/// more than eight tracks at once.
#[derive(Debug, Default)]
pub struct NotesPlugin;

impl Plugin for NotesPlugin {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.subscribe("resolve-dropped")?;
        host.subscribe("command-proposed")?;
        host.register_command("notes.count", "Record seen command count")?;
        host.register_panel(
            PanelSpec::new(NOTES_PANEL_ID, "Notes", PanelLocation::Sidebar),
            NotesPanel::new(),
        )?;
        let _ = host.setting_get("notes.directory")?;
        host.log(LogLevel::Info, "mounted example.notes");
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args_json: &str) -> Result<(), String> {
        match id {
            "notes.count" => {
                let seen = SEEN.load(Ordering::Relaxed).to_string();
                host.storage_set("seen_commands", seen.as_bytes())?;
                let _ = host.refresh_panel(NOTES_PANEL_ID);
                Ok(())
            }
            other => Err(format!("unknown command: {other}")),
        }
    }

    fn event(&self, _host: &Host, kind: &str, event_json: &str) -> Result<EventResponse, String> {
        match kind {
            "resolve-dropped" => {
                let parsed: Value =
                    serde_json::from_str(event_json).map_err(|err| err.to_string())?;
                let path_str = parsed.get("path").and_then(Value::as_str);
                let is_srt = path_str
                    .and_then(|path| Path::new(path).extension())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("srt"));
                if is_srt {
                    Ok(EventResponse::ClaimDrop(DropAction::ImportMedia))
                } else {
                    Ok(EventResponse::Pass)
                }
            }
            "command-proposed" => {
                SEEN.fetch_add(1, Ordering::Relaxed);
                let parsed: Value =
                    serde_json::from_str(event_json).map_err(|err| err.to_string())?;
                let track_count = parsed
                    .get("target_tracks")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                if track_count > 8 {
                    Ok(EventResponse::Refuse(
                        "Notes will not touch more than eight tracks at once.".to_owned(),
                    ))
                } else {
                    Ok(EventResponse::Pass)
                }
            }
            _ => Ok(EventResponse::Pass),
        }
    }
}

aria_sdk::export!(NotesPlugin);
