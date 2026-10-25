//! Editor panels hosted by the four-panel shell.

pub mod about_dialog;
pub mod ai_panel;
pub mod ai_tools;
pub mod align_panel;
pub mod analysis;
pub mod audio_gain_dialog;
pub mod audio_meters;
pub mod captions;
pub mod cloud;
pub mod color_panel;
pub mod context_menu;
pub mod effects_browser;
pub mod export_dialog;
pub mod generate;
pub mod history_panel;
pub mod home;
pub mod info_panel;
pub mod insert_chain;
pub mod inspector;
pub mod install_dialog;
pub mod jobs_dialog;
pub mod keyframe_icon;
pub mod keyframe_velocity_dialog;
pub mod marker_edit_dialog;
pub mod markers;
pub mod media_support;
pub mod mixer;
pub mod palette;
pub mod plugin_panel;
pub mod preview;
pub mod project_report;
pub mod remove_attributes_dialog;
pub mod rename_bar;
pub mod settings_dialog;
pub mod sidebar;
pub mod speed_duration_dialog;
pub mod text_panel;
pub mod timeline;
pub mod timeline_report;
pub mod unsaved_dialog;
pub mod value_readout;

#[cfg(test)]
#[path = "panel_casing_tests.rs"]
mod panel_casing_tests;
