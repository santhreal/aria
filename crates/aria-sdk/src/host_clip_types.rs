//! Registering a clip type that declares its fields and draws itself: a
//! `register_clip_type` spec with a `schema` object.

use std::sync::Arc;

use aria_plugin_abi::AriaStr;
use serde::Serialize;

use super::Host;
use crate::{ClipTypeSchema, ClipTypeSpec, CpuEffect, GraphPass};

/// What `register_clip_type` receives for a type that draws itself.
#[derive(Serialize)]
struct Defined<'a> {
    #[serde(flatten)]
    spec: &'a ClipTypeSpec,
    schema: Schema<'a>,
}

#[derive(Serialize)]
struct Schema<'a> {
    #[serde(flatten)]
    fields: &'a ClipTypeSchema,
    render: Render<'a>,
}

/// `"cpu"` or `{"passes": [...]}`.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Render<'a> {
    Cpu,
    Passes(&'a [GraphPass]),
}

impl Host {
    /// Register during `mount` a clip type whose clips store `schema.fields`
    /// and that `passes` draw, on the terms of
    /// [`register_effect_graph`](Self::register_effect_graph) for a generator
    /// reading the fields as `aria.params`. `spec.generator` is empty or the
    /// type's id. The Generators tab and the palette list the type, not its
    /// generator.
    pub fn define_clip_type(
        &self,
        spec: &ClipTypeSpec,
        schema: &ClipTypeSchema,
        passes: &[GraphPass],
    ) -> Result<(), String> {
        self.send_clip_type(spec, schema, Render::Passes(passes))
    }

    /// Register during `mount` a clip type whose clips store `schema.fields`
    /// and that `render` draws on the CPU, handed a transparent frame of the
    /// clip's size whose [`CpuFrameIn`](crate::CpuFrameIn) reads the fields
    /// by name. Otherwise on the terms of
    /// [`define_clip_type`](Self::define_clip_type).
    pub fn define_cpu_clip_type(
        &self,
        spec: &ClipTypeSpec,
        schema: &ClipTypeSchema,
        render: impl CpuEffect,
    ) -> Result<(), String> {
        let previous = self
            .cpu_effects
            .as_ref()
            .and_then(|reg| reg.lock().insert(spec.id.clone(), Arc::new(render)));
        let res = self.define_cpu_clip_type_spec(spec, schema);
        if res.is_err()
            && let Some(reg) = &self.cpu_effects
        {
            let mut held = reg.lock();
            match previous {
                Some(prev) => held.insert(spec.id.clone(), prev),
                None => held.remove(&spec.id),
            };
        }
        res
    }

    /// Register during `mount` a clip type drawn on the CPU by
    /// [`Plugin::cpu_effect_process`](crate::Plugin::cpu_effect_process),
    /// called with the type's id. Otherwise on the terms of
    /// [`define_cpu_clip_type`](Self::define_cpu_clip_type).
    pub fn define_cpu_clip_type_spec(
        &self,
        spec: &ClipTypeSpec,
        schema: &ClipTypeSchema,
    ) -> Result<(), String> {
        let out = self.send_clip_type(spec, schema, Render::Cpu);
        if out.is_ok()
            && let Some(specs) = &self.cpu_effect_specs
        {
            specs
                .lock()
                .insert(spec.id.clone(), Arc::new(schema.generator_spec(spec)));
        }
        out
    }

    #[allow(unsafe_code)]
    fn send_clip_type(
        &self,
        spec: &ClipTypeSpec,
        schema: &ClipTypeSchema,
        render: Render<'_>,
    ) -> Result<(), String> {
        if !self.table.has_contributions() {
            return Err("host table does not support clip type registration".to_owned());
        }
        let defined = Defined {
            spec,
            schema: Schema {
                fields: schema,
                render,
            },
        };
        let json = serde_json::to_string(&defined).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_contributions()` confirmed `register_clip_type` is present in `self.table`.
        let res = unsafe { (self.table.register_clip_type)(self.ctx, AriaStr::from_str(&json)) };
        self.decode_unit(res)
    }
}
