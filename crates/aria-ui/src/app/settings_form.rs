//! The editable half of Settings: the enumerated choices and the four typed fields.
//!
//! `aria-settings` owned an AI section (source, model, base URL, protocol, reasoning
//! summary, key, autonomy), a theme choice, and a cloud base URL, and the dialog showed
//! six of those as read-only lines. Configuring AI meant hand-editing
//! `~/.aria/config.toml` and restarting.
//!
//! Two shapes carry all of it:
//!
//! - **A choice is a chip row generated from the enum's own list.** Adding a source, a
//!   protocol, an autonomy level, or a theme costs no edit here or in the dialog.
//! - **A typed value is one focused field at a time.** [`SettingsField`] is the whole
//!   set, so the shell needs one key-routing block rather than one per box, and the
//!   draft is separate from the saved value: Esc leaves the file as it was.
//!
//! Nothing here contacts a provider. A key is stored and checked for shape; whether the
//! endpoint answers is a question the assistant asks when it runs, and a Settings dialog
//! that blocked on a network round trip would be worse than one that says "not
//! configured".

use aria_plugin::{SettingKind, SettingSpec};
use aria_settings::{
    AiApiProtocol, AiSource, Autonomy, ReasoningSummary, Settings, ThemeChoice, load, save,
};
use gpui::SharedString;

use super::AppState;

/// One typed settings value.
///
/// A plugin-declared field is the same shape as a shipped one, so the dialog draws
/// it with the same row and the shell routes its keys with the same block. The
/// difference is only where the value lives: a shipped field is a member of
/// [`Settings`], and a plugin's is a key in `SettingsService`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsField {
    AiBaseUrl,
    AiModel,
    AiKey,
    AiKeyEnv,
    CloudBaseUrl,
    /// The dialog's own filter. It is a field so it shares the key routing, but it
    /// never touches the config file.
    Filter,
    /// The name the Keys section saves the keys in force under. Not part of the
    /// config file: Enter writes `<data dir>/keymaps/<name>.toml`, and the field
    /// shows the name of the set in force.
    KeymapName,
    /// A field a plugin declared, by its settings key. Its label, placeholder
    /// and kind are read from the schema, so nothing here needs a second table
    /// of plugin settings.
    Plugin(&'static str),
}

/// How one field reads in the dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingLook {
    pub label: SharedString,
    /// What an empty value means.
    pub placeholder: SharedString,
    /// Which row draws it. A shipped field is text, or a secret for a key.
    pub kind: SettingKind,
}

impl SettingsField {
    /// Every shipped field, in dialog order. Plugin fields come from the schema.
    pub const ALL: [Self; 7] = [
        Self::AiBaseUrl,
        Self::AiModel,
        Self::AiKey,
        Self::AiKeyEnv,
        Self::CloudBaseUrl,
        Self::KeymapName,
        Self::Filter,
    ];

    /// The stable element id, also the palette-facing name.
    pub fn id(self) -> &'static str {
        match self {
            Self::AiBaseUrl => "ai-base-url",
            Self::AiModel => "ai-model",
            Self::AiKey => "ai-key",
            Self::AiKeyEnv => "ai-key-env",
            Self::CloudBaseUrl => "cloud-base-url",
            Self::Filter => "settings-filter",
            Self::KeymapName => "keymap-name",
            Self::Plugin(key) => key,
        }
    }

    /// How a shipped field reads: its label and what an empty value means.
    /// `None` for a plugin's field, which [`AppState::setting_look`] reads from
    /// the schema.
    fn shipped(self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            Self::AiBaseUrl => (
                "AI base URL",
                "Not set, for example http://localhost:11434/v1",
            ),
            Self::AiModel => ("AI model", "Not set"),
            Self::AiKey => ("AI key", "No key stored"),
            Self::AiKeyEnv => ("AI key variable", "No variable named"),
            Self::CloudBaseUrl => ("Cloud base URL", "The shipped default"),
            Self::Filter => ("Filter", "Type to narrow the sections"),
            Self::KeymapName => ("Save keys as", "A name, then Enter"),
            Self::Plugin(_) => return None,
        })
    }

    fn label(self) -> &'static str {
        self.shipped().map_or(self.id(), |(label, _)| label)
    }

    /// The saved value, as the row should read it.
    pub fn value(self, settings: &Settings) -> String {
        match self {
            Self::AiBaseUrl => settings.ai.base_url.clone(),
            Self::AiModel => settings.ai.model.clone(),
            Self::AiKey => settings.ai.api_key.clone().unwrap_or_default(),
            Self::AiKeyEnv => settings.ai.api_key_env.clone().unwrap_or_default(),
            Self::CloudBaseUrl => settings.cloud.base_url.clone(),
            // The set in force is not in this struct either:
            // `AppState::setting_draft` reads it.
            Self::Filter | Self::KeymapName => String::new(),
            // A plugin's value is not in this struct: it lives under its own key,
            // and `AppState::plugin_setting` is the one reader.
            Self::Plugin(_) => String::new(),
        }
    }

    /// Write `draft` into `settings`, refusing a value the rest of the app cannot use.
    ///
    /// An empty draft clears the value rather than storing an empty string, because
    /// "unset" and "set to nothing" behave differently: the first falls back to a
    /// default, the second sends an empty header.
    fn apply(self, settings: &mut Settings, draft: &str) -> Result<(), String> {
        let value = draft.trim();
        match self {
            Self::AiBaseUrl => {
                validate_url(value, self.label())?;
                settings.ai.base_url = value.to_string();
            }
            Self::AiModel => settings.ai.model = value.to_string(),
            Self::AiKey => {
                settings.ai.api_key = (!value.is_empty()).then(|| value.to_string());
            }
            Self::AiKeyEnv => {
                if !value.is_empty()
                    && !value
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                {
                    return Err(format!(
                        "{value} is not an environment variable name; use letters, digits, and underscores"
                    ));
                }
                settings.ai.api_key_env = (!value.is_empty()).then(|| value.to_string());
            }
            Self::CloudBaseUrl => {
                validate_url(value, self.label())?;
                settings.cloud.base_url = value.to_string();
            }
            // `commit_setting` writes a named set and never reaches here.
            Self::Filter | Self::KeymapName => {}
            // Not a member of this struct: `commit_setting` writes a plugin field
            // through `SettingsService` and never reaches here.
            Self::Plugin(_) => {}
        }
        Ok(())
    }
}

/// An endpoint has to be an absolute HTTP URL: a bare host silently becomes a relative
/// path at request time, which fails as a 404 rather than as a configuration error.
fn validate_url(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    if !(value.starts_with("http://") || value.starts_with("https://")) {
        return Err(format!(
            "{label} must start with http:// or https://, not {value}"
        ));
    }
    if value
        .split_once("://")
        .is_none_or(|(_, rest)| rest.is_empty())
    {
        return Err(format!("{label} has no host after the scheme"));
    }
    Ok(())
}

/// The dialog's editing state.
#[derive(Debug, Default, Clone)]
pub struct SettingsForm {
    /// Which field typing goes to, if any.
    pub focused: Option<SettingsField>,
    /// The in-progress text for the focused field.
    pub draft: String,
    /// The committed section filter.
    pub filter: String,
}

impl AppState {
    /// Focus one field, seeding the draft from what is saved.
    ///
    /// Seeding rather than starting empty: a focus usually precedes a correction,
    /// and a field that cleared itself on focus would lose the value being fixed.
    pub fn focus_setting(&mut self, field: Option<SettingsField>) {
        self.settings_form.focused = field;
        self.settings_form.draft = match field {
            Some(SettingsField::Filter) => self.settings_form.filter.clone(),
            Some(SettingsField::KeymapName) => self.active_keymap().unwrap_or_default().to_owned(),
            Some(SettingsField::Plugin(key)) => self.plugin_setting(key),
            Some(field) => field.value(&self.cloud_settings()),
            None => String::new(),
        };
    }

    /// How `field` reads in the dialog, or nothing for a plugin field no
    /// mounted plugin declares any more.
    pub fn setting_look(&self, field: SettingsField) -> Option<SettingLook> {
        if let Some((label, placeholder)) = field.shipped() {
            let kind = if field == SettingsField::AiKey {
                SettingKind::Secret
            } else {
                SettingKind::Text
            };
            return Some(SettingLook {
                label: SharedString::new_static(label),
                placeholder: SharedString::new_static(placeholder),
                kind,
            });
        }
        let spec = self.plugin_setting_spec(field.id())?;
        let placeholder = match (&spec.kind, spec.placeholder.is_empty()) {
            (_, false) => spec.placeholder,
            // A manifest number with no `min` or `max` is bounded at the f64
            // extremes, which are not worth reading.
            (SettingKind::Number { min, max, .. }, true) => {
                match (*min > f64::MIN, *max < f64::MAX) {
                    (true, true) => format!("{min} to {max}"),
                    (true, false) => format!("{min} or more"),
                    (false, true) => format!("{max} or less"),
                    (false, false) => "a number".to_owned(),
                }
            }
            (_, true) => "Not set".to_owned(),
        };
        Some(SettingLook {
            label: spec.label.into(),
            placeholder: placeholder.into(),
            kind: spec.kind,
        })
    }

    /// The declared field that owns one plugin settings key.
    pub fn plugin_setting_spec(&self, key: &str) -> Option<SettingSpec> {
        aria_plugin::settings_schema::schema_of(&self.ctx)?
            .field(key)
            .map(|(_, spec)| spec)
    }

    /// Check `text` against the field that owns `key` and save it, or clear the
    /// key when `text` is empty so it reads as its default. Returns the field's
    /// label.
    pub fn set_plugin_setting(&mut self, key: &str, text: &str) -> Result<SharedString, String> {
        let spec = self
            .plugin_setting_spec(key)
            .ok_or_else(|| format!("{key}: no mounted plugin declares this setting"))?;
        let stored = spec
            .kind
            .check(text)
            .map_err(|reason| format!("{}: {reason}", spec.label))?;
        let settings = self
            .ctx
            .try_get::<dyn aria_plugin::SettingsService>()
            .ok_or_else(|| {
                "settings are not available: the settings plugin is not mounted".to_string()
            })?;
        settings.set(
            key,
            if stored.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(stored)
            },
        );
        settings.save().map_err(|err| format!("{key}: {err}"))?;
        let label = SharedString::from(spec.label);
        self.status_message = Some(format!("{label} saved"));
        Ok(label)
    }

    /// The value `SettingsService` reads for one plugin settings key, as text:
    /// the stored value, or the declared default when nothing is stored.
    ///
    /// Through `SettingsService`, because that is the owner of the file. A string is
    /// shown as itself rather than as a quoted JSON string, which is what a row of
    /// text should read as; anything else is rendered as it is stored. Showing a
    /// value does not load a package waiting for its key; saving one does.
    pub fn plugin_setting(&self, key: &str) -> String {
        let Some(settings) = self.ctx.try_get::<dyn aria_plugin::SettingsService>() else {
            return String::new();
        };
        match settings.peek(key) {
            Some(serde_json::Value::String(text)) => text,
            Some(serde_json::Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        }
    }

    /// Every plugin-declared section, in declaration order, with its fields.
    ///
    /// Empty when no schema is mounted: a host with no plugin settings shows no
    /// section rather than an empty one.
    pub fn plugin_settings_sections(&self) -> Vec<std::sync::Arc<aria_plugin::SettingsSection>> {
        aria_plugin::settings_schema::schema_of(&self.ctx)
            .map(|schema| schema.sections())
            .unwrap_or_default()
    }

    pub fn setting_push(&mut self, ch: char) {
        if self.settings_form.focused.is_some() {
            self.settings_form.draft.push(ch);
            self.live_filter();
        }
    }

    pub fn setting_pop(&mut self) {
        if self.settings_form.focused.is_some() {
            self.settings_form.draft.pop();
            self.live_filter();
        }
    }

    /// The filter narrows as it is typed; every other field waits for Enter, because
    /// a half-typed URL saved on each keystroke would be written to disk six times.
    fn live_filter(&mut self) {
        if self.settings_form.focused == Some(SettingsField::Filter) {
            self.settings_form.filter = self.settings_form.draft.clone();
        }
    }

    /// Drop the draft and leave the saved value alone.
    pub fn cancel_setting(&mut self) {
        if self.settings_form.focused == Some(SettingsField::Filter) {
            self.settings_form.filter.clear();
        }
        self.focus_setting(None);
    }

    /// Persist the focused field.
    ///
    /// A refused value keeps the focus and the draft, so the typed text can be fixed
    /// rather than retyped.
    pub fn commit_setting(&mut self) -> Result<(), String> {
        let Some(field) = self.settings_form.focused else {
            return Ok(());
        };
        if field == SettingsField::Filter {
            self.settings_form.filter = self.settings_form.draft.clone();
            self.settings_form.focused = None;
            return Ok(());
        }
        let draft = self.settings_form.draft.clone();
        // A plugin's value is not in `Settings`: it is a key in the service that owns
        // the file, so the write goes there and the same save path persists it.
        if let SettingsField::Plugin(key) = field {
            self.set_plugin_setting(key, &draft)?;
        } else if field == SettingsField::KeymapName {
            self.save_keymap_as(&draft)?;
        } else {
            self.edit_settings(|settings| field.apply(settings, &draft))?;
            self.status_message = Some(format!("{} saved", field.label()));
        }
        self.settings_form.focused = None;
        self.settings_form.draft.clear();
        Ok(())
    }

    /// Load, mutate, and save the config beside this session's data directory.
    ///
    /// One path for every settings write: the file is read fresh each time, so an edit
    /// never persists a value that was stale when the dialog opened.
    fn edit_settings(
        &mut self,
        mutate: impl FnOnce(&mut Settings) -> Result<(), String>,
    ) -> Result<(), String> {
        let path = self.config_path();
        let mut settings = load(&path).map_err(|err| {
            format!(
                "{}: {err}; fix the file before changing a setting",
                path.display()
            )
        })?;
        mutate(&mut settings)?;
        save(&path, &settings).map_err(|err| format!("{}: {err}", path.display()))
    }

    pub fn set_ai_source(&mut self, source: AiSource) -> Result<(), String> {
        self.edit_settings(|settings| {
            settings.ai.source = source;
            Ok(())
        })?;
        self.status_message = Some(format!("AI source is {}", source.key()));
        Ok(())
    }

    pub fn set_ai_protocol(&mut self, protocol: AiApiProtocol) -> Result<(), String> {
        self.edit_settings(|settings| {
            settings.ai.api_protocol = protocol;
            Ok(())
        })?;
        self.status_message = Some(format!("AI protocol is {}", protocol.key()));
        Ok(())
    }

    pub fn set_autonomy(&mut self, autonomy: Autonomy) -> Result<(), String> {
        self.edit_settings(|settings| {
            settings.ai.autonomy = autonomy;
            Ok(())
        })?;
        self.status_message = Some(format!("Assistant autonomy is {}", autonomy.key()));
        Ok(())
    }

    pub fn set_reasoning_summary(&mut self, summary: ReasoningSummary) -> Result<(), String> {
        self.edit_settings(|settings| {
            settings.ai.reasoning_summary = summary;
            Ok(())
        })?;
        self.status_message = Some(format!("Reasoning summary is {}", summary.key()));
        Ok(())
    }

    /// Persist the theme choice and apply it to the running window.
    pub fn set_theme_choice(&mut self, theme: ThemeChoice) -> Result<(), String> {
        self.edit_settings(|settings| {
            settings.appearance.theme = theme;
            Ok(())
        })?;
        self.status_message = Some(format!("Theme is {}", theme.key()));
        Ok(())
    }

    /// The saved theme, for the picker.
    pub fn theme_choice(&self) -> ThemeChoice {
        self.cloud_settings().appearance.theme
    }

    /// Every line of the config file this build could not use, worded for the
    /// operator.
    ///
    /// A dropped value keeps its default, which is right: a hand edit must not
    /// stop the editor opening. Dropping it silently is not, because the file
    /// then claims something the session is not doing. `[storage].root =
    /// "cache"` is the case that made this necessary — a relative path is
    /// refused, the caches stay under the profile directory, and until now
    /// nothing anywhere said so.
    pub fn config_refusals(&self) -> Vec<String> {
        self.cloud_settings()
            .rejected
            .iter()
            .map(aria_settings::Rejected::line)
            .collect()
    }

    /// Whether the AI settings are usable, in one sentence.
    ///
    /// The check is `aria-ai`'s own, so the dialog cannot disagree with the assistant
    /// about what counts as configured.
    pub fn ai_status(&self) -> String {
        let settings = self.cloud_settings();
        match aria_ai::config::validate_ai_settings(&settings.ai) {
            Ok(()) => match aria_ai::config::resolve_api_key(
                settings.ai.api_key.as_deref(),
                settings.ai.api_key_env.as_deref(),
            ) {
                Ok(_) => "configured".to_string(),
                Err(err) => err,
            },
            Err(err) => err,
        }
    }

    /// The assistant's guardrails, read from the AI plugin.
    ///
    /// Not a copy of the defaults: with the plugin disabled the row says so, which
    /// is the honest reading — no run can be held to a cap that is not loaded.
    pub fn agent_limits(&self) -> String {
        match self.ctx.try_get::<crate::plugins::AiHost>() {
            Some(host) => {
                let config = host.agent_config();
                format!(
                    "{} edits, {} host calls, {} turns, {} images per prompt",
                    config.max_tool_calls,
                    config.max_host_calls,
                    config.max_turns,
                    config.max_images
                )
            }
            None => "the AI plugin is not mounted".to_string(),
        }
    }

    /// What the assistant can actually do, counted from the vocabularies
    /// themselves.
    ///
    /// Not a written claim: the edit count is `aria-ai`'s own tool table and the
    /// host count is the desktop's own, so a tool added or removed changes this
    /// line without anyone remembering to edit it. The point of the row is that
    /// "the assistant can edit the timeline" should not have to be taken on
    /// trust.
    pub fn agent_capabilities(&self) -> String {
        let edits = aria_ai::wire::tool_specs().len();
        let host = crate::app::assistant_host::host_tool_specs();
        let senses = host
            .iter()
            .filter(|spec| spec.tier == aria_ai::ToolTier::ReadOnly)
            .count();
        let confirms = host
            .iter()
            .filter(|spec| spec.tier == aria_ai::ToolTier::System)
            .count();
        let plugins = crate::app::assistant_host::plugin_tool_specs(&self.ctx).len();
        format_agent_capabilities(edits, host.len(), senses, confirms, plugins)
    }

    /// Which parts of a turn are switched on, from the settings that decide it.
    pub fn agent_phases(&self) -> String {
        let ai = self.cloud_settings().ai;
        let mut on: Vec<&str> = vec!["edits", "senses"];
        if ai.reasoning_summary == aria_settings::ReasoningSummary::Auto {
            on.push("reasoning");
        }
        match ai.autonomy {
            aria_settings::Autonomy::Ask => on.push("confirmation"),
            aria_settings::Autonomy::Full => on.push("no confirmation"),
        }
        on.join(", ")
    }

    /// Whether a section survives the current filter. An empty filter shows everything.
    pub fn settings_section_visible(&self, title: &str) -> bool {
        let filter = self.settings_form.filter.trim().to_lowercase();
        filter.is_empty() || title.to_lowercase().contains(&filter)
    }

    /// The draft to draw for `field`, and whether it is focused.
    pub fn setting_draft(&self, field: SettingsField) -> (String, bool) {
        let focused = self.settings_form.focused == Some(field);
        if focused {
            return (self.settings_form.draft.clone(), true);
        }
        let saved = match field {
            SettingsField::Filter => self.settings_form.filter.clone(),
            SettingsField::KeymapName => self.active_keymap().unwrap_or_default().to_owned(),
            SettingsField::Plugin(key) => self.plugin_setting(key),
            field => field.value(&self.cloud_settings()),
        };
        (saved, false)
    }
}

/// Format the AI `Can do` capability summary with singular/plural agreement.
pub(crate) fn format_agent_capabilities(
    edits: usize,
    app: usize,
    senses: usize,
    confirms: usize,
    plugins: usize,
) -> String {
    let plural = super::project_reach::plural;
    format!(
        "{edits} edit{}, {app} app tool{} of which {senses} look{} and {confirms} ask{} first, \
         {plugins} plugin tool{}",
        plural(edits),
        plural(app),
        if senses == 1 { "s" } else { "" },
        if confirms == 1 { "s" } else { "" },
        plural(plugins),
    )
}

#[cfg(test)]
#[path = "plugin_settings_tests.rs"]
mod plugin_settings_tests;
