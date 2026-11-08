//! Agent skills: what is installed, and installing one from the cloud catalog.
//!
//! A skill is a `SKILL.md` in `<data dir>/agent/skills/<id>/`, which is exactly where
//! [`aria_ai::extend::load_agent_dir`] looks. Installing one therefore has an effect on
//! the agent's next prompt without this module knowing anything about prompts: the
//! loader lists every installed skill in the system prompt and `read_skill` serves the
//! body on demand.
//!
//! Two rules the panel must not own:
//!
//! - A downloaded pack is parsed before it is installed. A file that is not a skill —
//!   an error page, a renamed archive, a `SKILL.md` with no frontmatter — is refused by
//!   name, because an unparseable skill installed into the agent dir turns into a
//!   warning on every later prompt instead of a failure at the click that caused it.
//! - The id is the directory name, so it is sanitized. A catalog id carrying a slash
//!   would install outside the skills directory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::SharedString;

use super::AppState;

/// One skill offered to the assistant as the panel draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiSkill {
    pub id: SharedString,
    pub name: SharedString,
    pub description: SharedString,
}

/// Read the bundled and installed skills for `data_dir` from disk.
pub(crate) fn load_assistant_skills(data_dir: &Path) -> Vec<UiSkill> {
    aria_ai::merge_skills(aria_ai::extend::load_agent_dir(&data_dir.join("agent")).skills)
        .into_iter()
        .map(|skill| UiSkill {
            id: SharedString::from(skill.id),
            name: SharedString::from(skill.name),
            description: SharedString::from(skill.description),
        })
        .collect()
}

impl AppState {
    /// Where rules, skills, and slash commands live for this session.
    ///
    /// Derived from `data_dir` like every other path the app owns, so a test with its
    /// own scratch directory cannot install into the real agent directory.
    pub fn agent_dir(&self) -> PathBuf {
        self.data_dir.join("agent")
    }

    /// Every skill the assistant is offered: the bundled set and the installed
    /// skills, an installed skill replacing a bundled one of the same id, sorted
    /// by id. The same merge builds the prompt, so the panel lists what the
    /// agent receives.
    ///
    /// Held on [`AppState`] and refreshed when the Assistant panel comes
    /// forward, after a skill is installed from the catalog or a dropped
    /// `SKILL.md`, and when an assistant turn finishes, so a draw of the panel
    /// does not read the disk.
    pub fn assistant_skills(&self) -> Arc<[UiSkill]> {
        Arc::clone(&self.assistant_skills)
    }

    /// Re-read [`Self::assistant_skills`] and [`Self::assistant_slash_commands`]
    /// from `<data_dir>/agent`.
    pub fn refresh_assistant_skills(&mut self) {
        let dir = aria_ai::extend::load_agent_dir(&self.agent_dir());
        self.assistant_skills = aria_ai::merge_skills(dir.skills)
            .into_iter()
            .map(|skill| UiSkill {
                id: SharedString::from(skill.id),
                name: SharedString::from(skill.name),
                description: SharedString::from(skill.description),
            })
            .collect::<Vec<_>>()
            .into();
        self.assistant.slash_commands = dir
            .commands
            .into_iter()
            .map(|command| command.name)
            .collect::<Vec<_>>()
            .into();
    }

    /// Create `<data_dir>/agent` on demand and reveal it in the file manager.
    pub fn reveal_agent_dir(&mut self) {
        let path = self.agent_dir();
        if let Err(err) = std::fs::create_dir_all(&path) {
            self.report_error(format!("{}: {err}", path.display()));
            return;
        }
        self.reveal_path(&path.display().to_string());
    }

    /// Install a downloaded skill file, validating it first.
    pub(crate) fn install_skill(
        &mut self,
        path: &Path,
        card: &crate::cloud::CloudCard,
    ) -> Result<(), String> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("{}: {err}", card.name, err = aria_models::io_reason(&err)))?;
        let id = skill_id(&card.id);
        if id.is_empty() {
            return Err(format!("{}: the catalog gave it no usable id", card.name));
        }
        let skill = aria_ai::extend::parse_skill(&id, &text)
            .map_err(|err| format!("{}: {err}", card.name))?;
        let dest = self.agent_dir().join("skills").join(&id).join("SKILL.md");
        aria_settings::replace_file(&dest, text.as_bytes())?;
        self.refresh_assistant_skills();
        self.status_message = Some(format!("Installed skill {}", skill.name));
        Ok(())
    }

    /// Install a `SKILL.md` dropped on the app.
    ///
    /// The id comes from the file's directory (`skills/my-skill/SKILL.md`), or from
    /// the file stem when it was renamed, because a skill is addressed by its
    /// directory name once installed.
    pub fn install_skill_file(&mut self, path: &Path) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|err| {
            format!(
                "{}: {err}",
                path.display(),
                err = aria_models::io_reason(&err)
            )
        })?;
        let raw = path
            .parent()
            .and_then(|dir| dir.file_name())
            .filter(|name| *name != "skills")
            .or_else(|| path.file_stem())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let id = skill_id(&raw);
        if id.is_empty() {
            return Err(format!("{}: no usable skill id", path.display()));
        }
        let skill = aria_ai::extend::parse_skill(&id, &text)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        let dest = self.agent_dir().join("skills").join(&id).join("SKILL.md");
        aria_settings::replace_file(&dest, text.as_bytes())?;
        self.refresh_assistant_skills();
        self.status_message = Some(format!("Installed skill {}", skill.name));
        Ok(())
    }
}

/// A directory-safe id. The catalog's ids are opaque and have carried slashes, which
/// would place the skill outside the skills directory.
fn skill_id(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}
