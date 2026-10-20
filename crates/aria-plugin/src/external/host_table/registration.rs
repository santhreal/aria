use std::ffi::c_void;
use std::sync::Arc;

use aria_plugin_abi::{AriaBytes, AriaResult, AriaStr};

use super::{call_ctx, read_str};
use crate::external::{NativePluginState, ai_tools, contributions, scopes};
use crate::{PluginCommand, PluginContext, event_wire};

pub(super) extern "C" fn host_register_command(
    host_ctx: *mut c_void,
    id: AriaStr,
    title: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx`, `id`, and `title` are valid for the duration of `host_register_command`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            let id = read_str(id, "command id")?;
            let title = read_str(title, "command title")?;
            register_native_command(ctx.scoped_ctx, ctx.state, id.to_owned(), title.to_owned())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(crate) fn register_native_command(
    scoped_ctx: &PluginContext,
    state: &Arc<NativePluginState>,
    id: String,
    title: String,
) -> Result<(), String> {
    let command = NativePluginCommand {
        state: Arc::clone(state),
        id: id.clone(),
    };
    scoped_ctx
        .register_command(state.package.command_spec(id, title), command)
        .map(drop)
        .map_err(|err| err.to_string())
}

pub(super) extern "C" fn host_subscribe(host_ctx: *mut c_void, event_kind: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `event_kind` are valid for the duration of `host_subscribe`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            let kind = read_str(event_kind, "event kind")?;
            let state = Arc::clone(ctx.state);
            event_wire::subscribe(ctx.scoped_ctx, kind, move |scoped, kind, payload| {
                state.call_event(scoped, kind, payload).ok()
            })
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_ai_tool(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_ai_tool`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            if !ctx.state.table.has_ai_tools() {
                return Err("plugin table does not include the ai_tool_call callback".to_owned());
            }
            ai_tools::register(ctx, read_str(spec_json, "AI tool spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_scope(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_scope`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            if !ctx.state.table.has_scopes() {
                return Err("plugin table does not include the scope_measure callback".to_owned());
            }
            scopes::register(ctx, read_str(spec_json, "scope spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_effect(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
    wgsl: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx`, `spec_json`, and `wgsl` are valid for the duration of `host_register_effect`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            contributions::register_effect(
                ctx,
                read_str(spec_json, "effect spec")?,
                read_str(wgsl, "effect WGSL")?,
            )
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_cpu_effect(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_cpu_effect`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            contributions::register_cpu_effect(ctx, read_str(spec_json, "CPU effect spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_transition(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
    wgsl: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx`, `spec_json`, and `wgsl` are valid for the duration of `host_register_transition`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            contributions::register_transition(
                ctx,
                read_str(spec_json, "transition spec")?,
                read_str(wgsl, "transition WGSL")?,
            )
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_generator(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
    wgsl: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx`, `spec_json`, and `wgsl` are valid for the duration of `host_register_generator`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            contributions::register_generator(
                ctx,
                read_str(spec_json, "generator spec")?,
                read_str(wgsl, "generator WGSL")?,
            )
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_clip_type(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_clip_type`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            contributions::register_clip_type(ctx, read_str(spec_json, "clip type spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_driver_language(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_driver_language`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            contributions::register_driver_language(
                ctx,
                read_str(spec_json, "driver language spec")?,
            )
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_preset(
    host_ctx: *mut c_void,
    preset_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `preset_json` are valid for the duration of `host_register_preset`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            contributions::register_preset(ctx, read_str(preset_json, "preset JSON")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_theme(
    host_ctx: *mut c_void,
    id: AriaStr,
    toml: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx`, `id`, and `toml` are valid for the duration of `host_register_theme`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            contributions::register_theme(
                ctx,
                read_str(id, "theme id")?,
                read_str(toml, "theme TOML")?,
            )
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_texture(
    host_ctx: *mut c_void,
    id: AriaStr,
    width: u32,
    height: u32,
    pixels: AriaBytes,
) -> AriaResult {
    // SAFETY: `host_ctx`, `id`, and `pixels` are valid for the duration of `host_register_texture`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            contributions::register_texture(
                ctx,
                read_str(id, "texture id")?,
                width,
                height,
                pixels.as_slice(),
            )
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_workspace(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_workspace`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            contributions::register_workspace(ctx, read_str(spec_json, "workspace spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_analyzer(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_analyzer`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            contributions::register_analyzer(ctx, read_str(spec_json, "analyzer spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

struct NativePluginCommand {
    state: Arc<NativePluginState>,
    id: String,
}

impl PluginCommand for NativePluginCommand {
    fn run(&self, ctx: &PluginContext) -> Result<(), String> {
        self.state.call_command_run(ctx, &self.id, "{}")
    }
}
