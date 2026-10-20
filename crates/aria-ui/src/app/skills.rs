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

use super::AppState;

/// One installed skill as the panel needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiSkill {
    pub id: String,
    pub name: String,
    pub description: String,
}

impl AppState {
    /// Where rules, skills, and slash commands live for this session.
    ///
    /// Derived from `data_dir` like every other path the app owns, so a test with its
    /// own scratch directory cannot install into the real agent directory.
    pub fn agent_dir(&self) -> PathBuf {
        self.data_dir.join("agent")
    }

    /// Every installed skill, by id.
    ///
    /// Read from disk on demand rather than cached: skills are also installed by hand
    /// and by the agent's own tooling, and a cache would show a stale list until
    /// restart.
    pub fn installed_skills(&self) -> Vec<UiSkill> {
        aria_ai::extend::load_agent_dir(&self.agent_dir())
            .skills
            .into_iter()
            .map(|skill| UiSkill {
                id: skill.id,
                name: skill.name,
                description: skill.description,
            })
            .collect()
    }

    /// Install a downloaded skill file, validating it first.
    pub(crate) fn install_skill(
        &mut self,
        path: &Path,
        card: &crate::cloud::CloudCard,
    ) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", card.name))?;
        let id = skill_id(&card.id);
        if id.is_empty() {
            return Err(format!("{}: the catalog gave it no usable id", card.name));
        }
        let skill = aria_ai::extend::parse_skill(&id, &text)
            .map_err(|err| format!("{}: {err}", card.name))?;
        let dir = self.agent_dir().join("skills").join(&id);
        std::fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        let dest = dir.join("SKILL.md");
        std::fs::write(&dest, &text).map_err(|err| format!("{}: {err}", dest.display()))?;
        self.status_message = Some(format!("Installed skill {}", skill.name));
        Ok(())
    }

    /// Install a `SKILL.md` dropped on the app.
    ///
    /// The id comes from the file's directory (`skills/my-skill/SKILL.md`), or from
    /// the file stem when it was renamed, because a skill is addressed by its
    /// directory name once installed.
    pub fn install_skill_file(&mut self, path: &Path) -> Result<(), String> {
        let text =
            std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
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
        let dir = self.agent_dir().join("skills").join(&id);
        std::fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        let dest = dir.join("SKILL.md");
        std::fs::write(&dest, &text).map_err(|err| format!("{}: {err}", dest.display()))?;
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
