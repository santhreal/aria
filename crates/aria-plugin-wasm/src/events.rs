//! Event subscriptions for a WASM plugin: [`aria_plugin::event_wire`],
//! delivering to the guest's `event` export.

use std::sync::Arc;

use aria_plugin::{Event, PluginContext, event_wire};

use crate::host::WasmPluginShared;

/// Private sentinel event type used to invoke guest `unmount` when the plugin's registrations drop.
pub(crate) struct UnmountSentinel;

impl Event for UnmountSentinel {}

/// Subscribe the WASM plugin to `event_kind` on `ctx` during `mount`.
pub(crate) fn subscribe_event(
    ctx: &PluginContext,
    shared: &Arc<WasmPluginShared>,
    event_kind: &str,
) -> Result<(), String> {
    let shared = Arc::clone(shared);
    event_wire::subscribe(ctx, event_kind, move |scoped, kind, payload| {
        shared.call_event(scoped, kind, payload).ok().flatten()
    })
}
