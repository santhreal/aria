//! Named effect presets (BACKLOG 58).
//!
//! An effect preset captures an ordered stack of [`EffectInstance`]s along with
//! their parameter values / keyframes. It is saved from a clip's current stack,
//! named, validated, and applied to other clips as a single unit.

use serde::{Deserialize, Serialize};

use crate::effects::EffectInstance;
use crate::error::ModelError;

/// A named preset capturing an effect stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectPreset {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<EffectInstance>,
}

impl EffectPreset {
    /// Create a new preset with `name` and `effects`.
    pub fn new(name: impl Into<String>, effects: Vec<EffectInstance>) -> Self {
        Self {
            name: name.into(),
            effects,
        }
    }

    /// Validate the preset.
    ///
    /// The name must not be empty or whitespace-only, and every effect instance
    /// in the stack must be valid according to the effect catalog and parameter
    /// constraints.
    pub fn validate(&self) -> Result<(), ModelError> {
        let trimmed = self.name.trim();
        if trimmed.is_empty() {
            return Err(ModelError::InvalidParam(
                "effect preset name cannot be empty".to_string(),
            ));
        }
        for effect in &self.effects {
            effect.validate()?;
        }
        Ok(())
    }
}

/// Validate a list of presets for internal consistency and name uniqueness.
///
/// Ensures each preset is individually valid and that no two presets share
/// the same trimmed name.
pub fn validate_preset_list(presets: &[EffectPreset]) -> Result<(), ModelError> {
    let mut seen = std::collections::HashSet::new();
    for preset in presets {
        preset.validate()?;
        let normalized = preset.name.trim().to_lowercase();
        if !seen.insert(normalized) {
            return Err(ModelError::InvalidParam(format!(
                "duplicate effect preset name '{}'",
                preset.name.trim()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_preset_passes_validation() {
        let fx = EffectInstance::new("gaussian_blur");
        let preset = EffectPreset::new("My Blur", vec![fx]);
        assert!(preset.validate().is_ok());
    }

    #[test]
    fn empty_name_is_rejected() {
        let preset = EffectPreset::new("", vec![EffectInstance::new("gaussian_blur")]);
        let err = preset.validate().unwrap_err();
        assert!(err.to_string().contains("name cannot be empty"));

        let preset_spaces = EffectPreset::new("   ", vec![EffectInstance::new("gaussian_blur")]);
        assert!(preset_spaces.validate().is_err());
    }

    #[test]
    fn unknown_effect_in_preset_is_rejected() {
        let preset = EffectPreset::new("Bad Effect", vec![EffectInstance::new("no_such_effect")]);
        assert!(preset.validate().is_err());
    }

    #[test]
    fn duplicate_names_in_list_are_rejected() {
        let p1 = EffectPreset::new("Vignette Blur", vec![EffectInstance::new("vignette")]);
        let p2 = EffectPreset::new("vignette blur", vec![EffectInstance::new("gaussian_blur")]);
        let list = vec![p1, p2];
        let err = validate_preset_list(&list).unwrap_err();
        assert!(err.to_string().contains("duplicate effect preset name"));
    }

    #[test]
    fn preset_serde_roundtrip() {
        let mut fx = EffectInstance::new("gaussian_blur");
        fx.set_param_constant(0, 12.0).unwrap();
        let preset = EffectPreset::new("Heavy Blur", vec![fx]);
        let json = serde_json::to_string(&preset).unwrap();
        let loaded: EffectPreset = serde_json::from_str(&json).unwrap();
        assert_eq!(preset, loaded);
        assert!(loaded.validate().is_ok());
    }
}
