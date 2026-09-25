use serde::{Deserialize, Serialize};

/// User-facing metadata attached to a [`Project`](crate::Project).
///
/// Kept separate from timeline/media state so saves and agent edits can update
/// notes without touching the edit graph.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMetadata {
    /// Free-form description or notes about this edit.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Optional creator / author label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Per-project AI agent rules, injected into the assistant's system
    /// prompt alongside the user's `~/.aria/agent/rules`. Prompt-level
    /// only — rules can shape proposals but never bypass command
    /// validation. Travels with an exported `.aria`, so the desktop UI
    /// must show (never silently apply) rules arriving with an imported
    /// project.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent_rules: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_metadata_serde_omission() {
        let default_meta = ProjectMetadata::default();
        let json = serde_json::to_string(&default_meta).unwrap();
        assert_eq!(json, "{}");

        let populated = ProjectMetadata {
            description: "Notes".into(),
            author: Some("Author".into()),
            agent_rules: "Rule 1".into(),
        };
        let json2 = serde_json::to_string(&populated).unwrap();
        let loaded: ProjectMetadata = serde_json::from_str(&json2).unwrap();
        assert_eq!(populated, loaded);
    }
}
