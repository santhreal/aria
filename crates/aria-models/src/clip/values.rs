//! The values a clip holds, checked as the edit that stores each one checks
//! it.
//!
//! An edit validates a transform, a crop, a look or a level before storing it.
//! A file reaches the model through the deserializer instead, so the loader
//! reads [`Clip::check_values`] for every clip and rejects a file holding a
//! value no edit stores: an opacity of -1, a scale of 0, a shadow blur that
//! saves as `null` and then does not open.
//!
//! Effects, audio effects, and the filter and LUT names are not checked here:
//! their rules come from catalogs, and a file naming an entry this build lacks
//! opens with that entry reported missing. The filter and LUT strengths are
//! checked like every other level.

use crate::error::ModelError;

use super::clip_param::ClipParam;
use super::param_value::ParamValue;
use super::retiming::validate_speed_curve;
use super::{Clip, ClipSource};

impl Clip {
    /// The first value of this clip, or of its baked copy, that the edit
    /// storing it rejects, as that edit's error.
    pub(crate) fn check_values(&self) -> Result<(), ModelError> {
        self.transform.validate()?;
        self.crop.validate_shape()?;
        self.crop.for_each_value(|rect| rect.validate())?;
        validate_speed_curve(&self.speed_curve)?;
        for (param, curve) in [
            (ClipParam::Volume, &self.volume),
            (ClipParam::Pan, &self.pan),
        ] {
            curve.validate_shape()?;
            curve.for_each_value(|value| param.validate_value(ParamValue::Scalar(*value)))?;
        }
        self.motion_blur.validate()?;
        self.styles.validate()?;
        self.adjust.validate()?;
        if let Some(chroma) = &self.chroma_key {
            chroma.validate()?;
        }
        if let Some(matte) = &self.matte {
            matte.validate()?;
        }
        if let Some(filter) = &self.filter {
            filter.validate_intensity()?;
        }
        if let Some(lut) = &self.lut {
            lut.validate_intensity()?;
        }
        self.drivers
            .iter()
            .try_for_each(|driver| driver.code.validate())?;
        self.masks.iter().try_for_each(|mask| mask.validate())?;
        if let ClipSource::Generated(generator) = &self.content {
            generator.validate()?;
        }
        match &self.bake {
            Some(bake) => bake.check_values(),
            None => Ok(()),
        }
    }
}
