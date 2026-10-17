//! Driver languages: plugins that compile property driver code.
//!
//! A registered language joins [`aria_render`]'s process-wide list under its
//! id, the id a clip's driver code names, so preview, export, and the audio
//! mix evaluate drivers written in it. A failure or panic in compiling or
//! evaluating one driver faults that driver alone, read back with
//! [`aria_render::driver_fault`]; the plugin keeps running. Time spent
//! evaluating drivers is recorded as the plugin's frame cost.

use std::sync::Arc;
use std::time::Instant;

use aria_models::ParamValue;
use aria_render::{DriverInput, DriverLanguage, DriverProgram, DriverScope};
use serde::{Deserialize, Serialize};

use crate::context::{PluginContext, WeakContext};
use crate::error::PluginError;
use crate::listed::{Listed, owner};
use crate::registration::{Contribution, ContributionKind, Registration};

/// What a driver language is called.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverLanguageSpec {
    /// The id driver code names: ASCII letters, digits, `.`, `-` and `_`,
    /// unique among the host's plugins.
    pub id: String,
    /// The name the inspector shows when choosing a language.
    pub name: String,
}

/// One registered driver language, as the Plugins list and the inspector read
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDriverLanguage {
    /// The registering plugin's id, empty for the host.
    pub plugin_id: Arc<str>,
    pub spec: Arc<DriverLanguageSpec>,
}

pub(crate) type DriverLanguages = Listed<PluginDriverLanguage>;

fn refused(reason: String) -> PluginError {
    PluginError::DriverLanguageRefused(reason)
}

impl PluginContext {
    /// Add a language that compiles property driver code naming `spec.id`.
    ///
    /// Held on the terms of [`register_effect`](Self::register_effect).
    /// Refused when the id is not letters, digits, `.`, `-` and `_`, the name
    /// is empty, or any plugin already registered a language under the id.
    pub fn register_driver_language(
        &self,
        spec: DriverLanguageSpec,
        language: impl DriverLanguage + 'static,
    ) -> Result<Registration, PluginError> {
        if spec.name.trim().is_empty() {
            return Err(refused(format!(
                "{}: a driver language needs a name",
                spec.id
            )));
        }
        let (plugin_id, _) = owner(self);
        let spec = Arc::new(spec);
        let id = spec.id.clone();
        let listed = self
            .driver_language_registry()
            .insert(
                &plugin_id,
                PluginDriverLanguage {
                    plugin_id: Arc::clone(&plugin_id),
                    spec: Arc::clone(&spec),
                },
                |held| held.spec.id == id,
            )
            .ok_or_else(|| refused(format!("a driver language is registered as {id}")))?;
        let timed: Timed<dyn DriverLanguage> = Timed {
            plugin: (!plugin_id.is_empty()).then_some(plugin_id),
            ctx: self.downgrade(),
            inner: Arc::new(language),
        };
        let handle =
            aria_render::register_driver_language(id.clone(), Arc::new(timed)).map_err(refused)?;
        let contribution = Contribution::new(ContributionKind::DriverLanguage, id);
        Ok(self.retain_if_mounting(Registration::holding(contribution, (handle, listed))))
    }

    /// Every registered driver language, in registration order.
    pub fn driver_languages(&self) -> Vec<PluginDriverLanguage> {
        self.driver_language_registry().items()
    }

    /// Counts driver language registrations and removals.
    pub fn driver_languages_revision(&self) -> u64 {
        self.driver_language_registry().revision()
    }
}

/// Records the time spent in a plugin's driver code as its frame cost.
struct Timed<T: ?Sized> {
    plugin: Option<Arc<str>>,
    ctx: WeakContext,
    inner: Arc<T>,
}

impl<T: ?Sized> Timed<T> {
    fn record(&self, started: Instant) {
        if let (Some(plugin), Some(ctx)) = (self.plugin.as_deref(), self.ctx.upgrade()) {
            ctx.record_frame_cost(plugin, started.elapsed());
        }
    }
}

impl DriverLanguage for Timed<dyn DriverLanguage> {
    fn compile(&self, source: &str) -> Result<Arc<dyn DriverProgram>, String> {
        let started = Instant::now();
        let compiled = self.inner.compile(source);
        self.record(started);
        let inner = compiled?;
        Ok(Arc::new(Timed {
            plugin: self.plugin.clone(),
            ctx: self.ctx.clone(),
            inner,
        }))
    }

    fn reference(&self, input: &DriverInput) -> Option<String> {
        self.inner.reference(input)
    }

    fn template(&self) -> String {
        self.inner.template()
    }
}

impl DriverProgram for Timed<dyn DriverProgram> {
    fn inputs(&self) -> &[DriverInput] {
        self.inner.inputs()
    }

    fn evaluate(&self, scope: &mut dyn DriverScope) -> Result<ParamValue, String> {
        let started = Instant::now();
        let value = self.inner.evaluate(scope);
        self.record(started);
        value
    }
}
