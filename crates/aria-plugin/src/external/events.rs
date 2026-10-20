//! Event subscriptions for a native plugin: [`crate::event_wire`], delivering to
//! the plugin's `event` callback.

use std::sync::Arc;

use super::NativePluginState;
use crate::{Event, PluginContext, event_wire};

/// Sentinel event type used only so `EventBus` holds an unmount drop guard inside
/// the plugin's registration list without adding an entry to `MountedPluginInfo::services`.
pub(crate) struct UnmountSentinel;

impl Event for UnmountSentinel {}

pub(crate) fn subscribe_event(
    ctx: &PluginContext,
    state: &Arc<NativePluginState>,
    event_kind: &str,
) -> Result<(), String> {
    let state = Arc::clone(state);
    event_wire::subscribe(ctx, event_kind, move |scoped, kind, payload| {
        state.call_event(scoped, kind, payload).ok()
    })
}
