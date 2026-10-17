//! A C ABI clip type that declares its fields and draws itself: the `schema`
//! object of a `register_clip_type` spec.

use std::sync::Arc;

use aria_models::{ClipTypeSpec, EffectGroupSpec, EffectParamSpec};
use serde::Deserialize;

use super::{HostCallContext, NativeCpuEffect, RawPass};
use crate::clip_types::{ClipTypeDefinition, ClipTypeRender};

/// The fields and renderer of a clip type that draws itself.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawClipTypeSchema {
    #[serde(default)]
    fields: Vec<EffectParamSpec>,
    #[serde(default)]
    groups: Vec<EffectGroupSpec>,
    #[serde(default)]
    time_dependent: bool,
    render: RawClipRender,
}

/// `"cpu"`, drawn by `cpu_effect_process` called with the type's id, or
/// `{"passes": [...]}`, a pass list on the terms of `register_generator`.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawClipRender {
    Cpu,
    Passes(Vec<RawPass>),
}

/// Define `spec` with the fields and renderer `schema` declares.
pub(super) fn define(
    ctx: &HostCallContext<'_>,
    spec: ClipTypeSpec,
    schema: RawClipTypeSchema,
) -> Result<(), String> {
    let render = match schema.render {
        RawClipRender::Cpu => {
            if !ctx.state.table.has_contributions() {
                return Err(
                    "plugin table does not include the cpu_effect_process callback".to_owned(),
                );
            }
            ClipTypeRender::Cpu(Arc::new(NativeCpuEffect {
                id: spec.id.clone(),
                state: Arc::clone(ctx.state),
                scoped_ctx: ctx.scoped_ctx.clone(),
            }))
        }
        RawClipRender::Passes(passes) => {
            ClipTypeRender::Passes(passes.into_iter().map(RawPass::into_graph).collect())
        }
    };
    ctx.scoped_ctx
        .define_clip_type(ClipTypeDefinition {
            spec,
            fields: schema.fields,
            groups: schema.groups,
            time_dependent: schema.time_dependent,
            render,
        })
        .map(drop)
        .map_err(|err| err.to_string())
}
