//! Process bring-up helpers that do not depend on a UI toolkit.

pub(crate) fn setup_tracing() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,aria=info,aria_desktop=info,aria_ui=info,aria_engine=info,aria_render=info,aria_models=info,aria_commands=info,aria_analysis=info,aria_jobs=info,aria_plugin=info,aria_cloud=info,aria_ai=info,aria_transcription=info,aria_dialog=info,aria_shapes=info,aria_text=info")),
        )
        .init();
}
