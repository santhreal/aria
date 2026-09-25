use super::ParamRange;

impl ParamRange {
    /// Finite, and nothing more.
    pub const ANY: Self = Self {
        min: None,
        max: None,
    };

    pub const fn between(min: f32, max: f32) -> Self {
        Self {
            min: Some(min),
            max: Some(max),
        }
    }

    /// Bounded below only.
    pub const fn above(min: f32) -> Self {
        Self {
            min: Some(min),
            max: None,
        }
    }

    /// Bounded above only.
    pub const fn below(max: f32) -> Self {
        Self {
            min: None,
            max: Some(max),
        }
    }

    /// Hold a value inside whichever sides are declared.
    pub fn clamp(&self, value: f32) -> f32 {
        let mut value = value;
        if let Some(min) = self.min {
            value = value.max(min);
        }
        if let Some(max) = self.max {
            value = value.min(max);
        }
        value
    }

    /// The distance between the bounds, when both are declared. A stepper with a
    /// span can size its own step; one without needs the caller's.
    pub fn span(&self) -> Option<f32> {
        match (self.min, self.max) {
            (Some(min), Some(max)) => Some(max - min),
            _ => None,
        }
    }
}
