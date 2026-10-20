use serde::Deserialize;

use crate::PanelLocation;

#[derive(Deserialize)]
pub(super) struct RawImporterSpec {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) extensions: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct RawExporterSpec {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) extension: String,
}

#[derive(Deserialize)]
pub(super) struct RawEncoderSpec {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) extension: String,
    #[serde(default)]
    pub(super) audio: bool,
}

#[derive(Deserialize)]
pub(super) struct RawAudioParamSpec {
    pub(super) name: String,
    pub(super) label: String,
    #[serde(alias = "default")]
    pub(super) default_value: f32,
    pub(super) min: f32,
    pub(super) max: f32,
}

#[derive(Deserialize)]
pub(super) struct RawAudioEffectSpec {
    pub(super) id: String,
    pub(super) label: String,
    #[serde(default)]
    pub(super) category: String,
    #[serde(default)]
    pub(super) params: Vec<RawAudioParamSpec>,
    #[serde(default)]
    pub(super) latency: u32,
    #[serde(default)]
    pub(super) sidechain: bool,
}

#[derive(Deserialize)]
pub(super) struct RawImportResult {
    #[serde(default)]
    pub(super) commands_json: String,
    #[serde(default)]
    pub(super) skipped: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct RawExportReport {
    #[serde(default)]
    pub(super) skipped: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct RawPanelSpec {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) location: String,
}

#[derive(Deserialize)]
pub(super) struct RawOverlaySpec {
    pub(super) id: String,
    pub(super) title: String,
}

pub(super) fn parse_panel_location(raw: &str) -> Result<PanelLocation, String> {
    match raw {
        "sidebar" | "Sidebar" => Ok(PanelLocation::Sidebar),
        "inspector" | "Inspector" => Ok(PanelLocation::Inspector),
        "timeline" | "Timeline" => Ok(PanelLocation::Timeline),
        "overlay" | "Overlay" => Ok(PanelLocation::Overlay),
        "viewer" | "Viewer" => Ok(PanelLocation::Viewer),
        "home" | "Home" => Ok(PanelLocation::Home),
        other => Err(format!("unknown panel location `{other}`")),
    }
}

pub(super) fn validate_storage_key(key: &str) -> Result<(), String> {
    if key.is_empty()
        || key == "."
        || key == ".."
        || key.contains('/')
        || key.contains('\\')
        || key.contains('\0')
    {
        return Err(format!(
            "invalid storage key `{key}`: key must be a single file name"
        ));
    }
    Ok(())
}
