//! A WASM clip type that declares its fields and draws itself:
//! `define-clip-type`.

use std::sync::Arc;

use aria_plugin::{ClipTypeDefinition, ClipTypeRender, PluginContext};

use super::{
    WasmCpuEffect, host_clip_type_spec, host_graph_pass, host_group_spec, host_param_spec,
};
use crate::bindings;
use crate::host::WasmPluginShared;

pub(crate) fn define_clip_type(
    ctx: &PluginContext,
    shared: Arc<WasmPluginShared>,
    spec: bindings::ClipTypeSpec,
    schema: bindings::ClipTypeSchema,
    render: bindings::ClipTypeRender,
) -> Result<(), String> {
    let spec = host_clip_type_spec(spec);
    let render = match render {
        bindings::ClipTypeRender::Cpu => {
            if !shared.has_parity_exports() {
                return Err("component does not export cpu-effect-process".to_owned());
            }
            ClipTypeRender::Cpu(Arc::new(WasmCpuEffect::new(shared, ctx, spec.id.clone())))
        }
        bindings::ClipTypeRender::Passes(passes) => {
            ClipTypeRender::Passes(passes.into_iter().map(host_graph_pass).collect())
        }
    };
    ctx.define_clip_type(ClipTypeDefinition {
        spec,
        fields: schema.fields.into_iter().map(host_param_spec).collect(),
        groups: schema.groups.into_iter().map(host_group_spec).collect(),
        time_dependent: schema.time_dependent,
        render,
    })
    .map(drop)
    .map_err(|err| err.to_string())
}
