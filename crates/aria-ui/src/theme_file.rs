//! Theme files: token values read from `<data dir>/themes/*.toml` and from
//! the themes plugins register.
//!
//! A file holds a `[dark]` table, a `[light]` table, or both, mapping token
//! names to values. A token the file leaves out keeps the shipped value for
//! that appearance, and an appearance with no table is the shipped one, so a
//! file that sets one accent is a whole theme. Colours are `#rgb`, `#rrggbb`,
//! `#rrggbbaa`, or `oklch(l c h)` with an optional `/ alpha`; the font tokens
//! take a family name. A plugin theme is the same text, registered with
//! `PluginContext::register_theme` or shipped in a package's `themes/`.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use gpui::{Hsla, Rgba, SharedString};

use super::{Appearance, Theme, oklch};

/// The directory under the data dir that holds theme files.
pub const THEMES_DIR: &str = "themes";

/// The largest theme file read. A theme is a few dozen lines.
const MAX_THEME_BYTES: u64 = 64 * 1024;

type ColorSlot = fn(&mut Theme) -> &mut Hsla;
type FontSlot = fn(&mut Theme) -> &mut SharedString;

/// `(field name, accessor)` for each listed [`Theme`] field.
macro_rules! slots {
    ($ty:ty; $($name:ident),* $(,)?) => {
        &[$((stringify!($name), {
            fn slot(theme: &mut Theme) -> &mut $ty {
                &mut theme.$name
            }
            slot
        })),*]
    };
}

/// Every colour token a theme file sets, by its field name.
pub(super) const COLOR_TOKENS: &[(&str, ColorSlot)] = slots!(Hsla;
    bg,
    surface,
    surface_header,
    surface_raised,
    surface_card,
    surface_dialog,
    surface_overlay,
    element_hover,
    element_active,
    border,
    border_strong,
    hairline,
    text,
    text_muted,
    text_faint,
    text_dim,
    value,
    solid,
    on_solid,
    accent,
    accent_strong,
    on_accent,
    danger,
    danger_muted,
    warning,
    warning_muted,
    success,
    busy,
    success_muted,
    surface_raised_hover,
    band,
    input_bg,
    selection,
    cursor,
    caret,
    danger_strong,
    track_video,
    track_audio,
    track_text,
    track_generator,
    playhead,
    snap_guide,
    waveform_peak,
    waveform_rms,
    recording_dot,
);

/// Every font token a theme file sets, by its field name.
pub(super) const FONT_TOKENS: &[(&str, FontSlot)] = slots!(SharedString;
    font_sans,
    font_mono,
    font_sans_fallback,
    font_mono_fallback,
);

/// A theme read from a file: the values it sets for each appearance.
#[derive(Debug, Clone)]
pub struct ThemeFile {
    /// The name Settings saves: the file name without `.toml`, or
    /// `<plugin id>/<theme id>` for a plugin theme.
    pub stem: String,
    /// The file's `name`, or the file name or theme id when it gives none.
    pub name: String,
    /// The display name of the plugin that registered the theme.
    pub plugin: Option<String>,
    dark: Overrides,
    light: Overrides,
}

/// The token values one appearance's table sets.
#[derive(Debug, Clone, Default)]
struct Overrides {
    colors: Vec<(ColorSlot, Hsla)>,
    fonts: Vec<(FontSlot, SharedString)>,
}

impl Overrides {
    fn parse(section: &str, value: &toml::Value) -> Result<Self, String> {
        let table = value
            .as_table()
            .ok_or_else(|| format!("`{section}` is not a table: write it as [{section}]"))?;
        let mut overrides = Self::default();
        for (token, value) in table {
            let text = value
                .as_str()
                .ok_or_else(|| format!("[{section}] {token} is not a string"))?;
            if let Some(&(_, slot)) = COLOR_TOKENS.iter().find(|(name, _)| name == token) {
                let color =
                    parse_color(text).map_err(|err| format!("[{section}] {token}: {err}"))?;
                overrides.colors.push((slot, color));
            } else if let Some(&(_, slot)) = FONT_TOKENS.iter().find(|(name, _)| name == token) {
                let family = text.trim();
                if family.is_empty() {
                    return Err(format!("[{section}] {token} names no font family"));
                }
                overrides
                    .fonts
                    .push((slot, SharedString::from(family.to_owned())));
            } else {
                return Err(format!(
                    "[{section}] has no token `{token}`; docs/theme-tokens.md lists the tokens"
                ));
            }
        }
        Ok(overrides)
    }

    fn is_empty(&self) -> bool {
        self.colors.is_empty() && self.fonts.is_empty()
    }
}

impl ThemeFile {
    /// Read a theme file's text. `stem` is the file name without `.toml`.
    pub fn parse(stem: &str, text: &str) -> Result<Self, String> {
        let table: toml::Table = text
            .parse()
            .map_err(|err| format!("not valid TOML: {err}"))?;
        let mut name = None;
        let mut dark = Overrides::default();
        let mut light = Overrides::default();
        for (key, value) in &table {
            match key.as_str() {
                "name" => {
                    let given = value.as_str().ok_or("`name` is not a string")?.trim();
                    if !given.is_empty() {
                        name = Some(given.to_owned());
                    }
                }
                "dark" => dark = Overrides::parse("dark", value)?,
                "light" => light = Overrides::parse("light", value)?,
                other => {
                    return Err(format!(
                        "unknown key `{other}`: a theme file holds `name`, [dark], and [light]"
                    ));
                }
            }
        }
        if dark.is_empty() && light.is_empty() {
            return Err("sets no tokens: add a [dark] or [light] table".to_owned());
        }
        Ok(Self {
            stem: stem.to_owned(),
            name: name.unwrap_or_else(|| stem.to_owned()),
            plugin: None,
            dark,
            light,
        })
    }

    /// Read a theme a plugin registered.
    pub fn from_plugin(theme: &aria_plugin::PluginTheme) -> Result<Self, String> {
        let mut file = Self::parse(&theme.id, &theme.text)?;
        file.stem = theme.key();
        file.plugin = Some(theme.plugin.to_string());
        Ok(file)
    }

    /// The name a picker shows: the theme's name, and the plugin it came from.
    pub fn label(&self) -> String {
        match &self.plugin {
            Some(plugin) => format!("{} from {plugin}", self.name),
            None => self.name.clone(),
        }
    }

    /// Whether the file sets any token for `appearance`.
    pub fn covers(&self, appearance: Appearance) -> bool {
        !self.overrides(appearance).is_empty()
    }

    fn overrides(&self, appearance: Appearance) -> &Overrides {
        match appearance {
            Appearance::Dark => &self.dark,
            Appearance::Light => &self.light,
        }
    }
}

impl Theme {
    /// This theme with the values `file` sets for its appearance.
    pub fn with_file(mut self, file: &ThemeFile) -> Self {
        let overrides = file.overrides(self.appearance);
        for &(slot, color) in &overrides.colors {
            *slot(&mut self) = color;
        }
        for (slot, family) in &overrides.fonts {
            *slot(&mut self) = family.clone();
        }
        self
    }
}

/// A colour value: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, or
/// `oklch(l c h)` / `oklch(l c h / alpha)` with `l` and `alpha` in `0..=1`.
pub(super) fn parse_color(text: &str) -> Result<Hsla, String> {
    let text = text.trim();
    if let Some(digits) = text.strip_prefix('#') {
        // `Rgba` parses each pair with `from_str_radix`, which takes a sign.
        let hex = digits.bytes().all(|byte| byte.is_ascii_hexdigit());
        return Rgba::try_from(text)
            .ok()
            .filter(|_| hex)
            .map(Hsla::from)
            .ok_or_else(|| format!("`{text}` is not #rgb, #rgba, #rrggbb, or #rrggbbaa"));
    }
    let Some(body) = text
        .strip_prefix("oklch(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return Err(format!(
            "`{text}` is not a colour: write #rrggbb, #rrggbbaa, or oklch(l c h)"
        ));
    };
    let (channels, alpha) = match body.split_once('/') {
        Some((channels, alpha)) => (channels, Some(alpha)),
        None => (body, None),
    };
    let number = |part: &str| {
        part.trim()
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("`{part}` in `{text}` is not a number"))
    };
    let numbers = channels
        .split_whitespace()
        .map(number)
        .collect::<Result<Vec<f32>, String>>()?;
    let &[lightness, chroma, hue] = numbers.as_slice() else {
        return Err(format!("`{text}` needs three numbers: oklch(l c h)"));
    };
    let alpha = alpha.map_or(Ok(1.0), number)?;
    if !(0.0..=1.0).contains(&lightness) || chroma < 0.0 || !(0.0..=1.0).contains(&alpha) {
        return Err(format!(
            "`{text}` is out of range: lightness and alpha are 0 to 1, chroma is 0 or more"
        ));
    }
    let mut color = oklch(lightness, chroma, hue);
    color.a = alpha;
    Ok(color)
}

/// The theme files in a data dir, and the files that could not be read.
#[derive(Debug, Default)]
pub struct ThemeFiles {
    /// Readable themes, sorted by name without regard to case.
    pub themes: Vec<Arc<ThemeFile>>,
    /// `(file name, reason)` for each file that could not be read; a plugin
    /// theme is listed as `<plugin id>/<theme id> from <plugin>`.
    pub failed: Vec<(String, String)>,
}

impl ThemeFiles {
    /// The theme saved as `stem`.
    pub fn find(&self, stem: &str) -> Option<&Arc<ThemeFile>> {
        self.themes.iter().find(|theme| theme.stem == stem)
    }
}

/// Read every `*.toml` file in `<data_dir>/themes`, then `plugins`, the themes
/// plugins registered. A missing directory holds no themes.
pub fn scan(data_dir: &Path, plugins: &[aria_plugin::PluginTheme]) -> ThemeFiles {
    let mut found = scan_dir(data_dir);
    for theme in plugins {
        match ThemeFile::from_plugin(theme) {
            Ok(file) => found.themes.push(Arc::new(file)),
            Err(reason) => found
                .failed
                .push((format!("{} from {}", theme.key(), theme.plugin), reason)),
        }
    }
    found
        .themes
        .sort_by_cached_key(|theme| (theme.name.to_lowercase(), theme.stem.clone()));
    found.failed.sort();
    found
}

fn scan_dir(data_dir: &Path) -> ThemeFiles {
    let dir = data_dir.join(THEMES_DIR);
    let mut found = ThemeFiles::default();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return found,
        Err(err) => {
            found
                .failed
                .push((dir.display().to_string(), format!("could not list: {err}")));
            return found;
        }
    };
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(err) => {
                found
                    .failed
                    .push((dir.display().to_string(), format!("could not list: {err}")));
                continue;
            }
        };
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let file = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            found
                .failed
                .push((file, "the file name is not UTF-8: rename it".to_owned()));
            continue;
        };
        match read_capped(&path).and_then(|text| ThemeFile::parse(stem, &text)) {
            Ok(theme) => found.themes.push(Arc::new(theme)),
            Err(reason) => found.failed.push((file, reason)),
        }
    }
    found
}

fn read_capped(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|err| format!("could not open: {err}"))?;
    let mut text = String::new();
    file.take(MAX_THEME_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|err| format!("could not read: {err}"))?;
    if text.len() as u64 > MAX_THEME_BYTES {
        return Err(format!(
            "larger than {} KiB; a theme file holds token values only",
            MAX_THEME_BYTES / 1024
        ));
    }
    Ok(text)
}

#[cfg(test)]
#[path = "theme_file_tests.rs"]
mod tests;
