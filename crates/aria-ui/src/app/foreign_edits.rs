//! Work the event bus brings to the window: edits made away from it, and
//! notification handlers that run on its thread.
//!
//! The project host posts each committed command as `CommandApplied`, and the
//! bus delivers it on its delivery thread. The window re-projects when one
//! arrives from any origin but the editor, whose edits re-project where they
//! are made. A handler a plugin registers with `on_event_main` is queued by the
//! delivery thread and run here.

use std::sync::Arc;

use aria_plugin::{CommandApplied, Origin};
use gpui::Context;

use super::AppState;

impl AppState {
    /// Install the one subscriber that brings edits from other origins onto the
    /// window. The handler runs on the event bus's delivery thread and only
    /// sends a wake; a task on the window's thread re-projects.
    pub fn watch_edits(&mut self, cx: &mut Context<Self>) {
        if self.edits_watch.is_some() {
            return;
        }
        let (wake, wakes) = crate::handoff::wakes::<()>();
        self.edits_watch = Some(self.ctx.on_event::<CommandApplied, _>(move |applied| {
            if applied.origin != Origin::Editor {
                wake.wake(());
            }
        }));
        cx.spawn(async move |this, cx| {
            // `None` is the subscription dropped with the state: shutdown.
            while let Some(()) = wakes.next_from_a_thread(cx.background_executor()).await {
                // A committed turn announces each of its commands; one
                // projection covers all of them.
                while wakes.try_next().is_some() {}
                let updated = this.update(cx, |state, cx| {
                    state.sync_from_engine();
                    state.republish_audio();
                    cx.notify();
                });
                if updated.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    /// Run main-lane notification handlers on the window's thread. The bus
    /// calls the waker from its delivery thread when the queue fills; a task
    /// on the window's thread drains it. Calling this again replaces the
    /// waker, and the earlier task ends with it.
    pub fn watch_main_lane(&mut self, cx: &mut Context<Self>) {
        let (wake, wakes) = crate::handoff::wakes::<()>();
        self.ctx.events().set_main_waker(move || {
            wake.wake(());
        });
        let bus = Arc::downgrade(self.ctx.event_bus());
        cx.spawn(async move |this, cx| {
            while let Some(()) = wakes.next_from_a_thread(cx.background_executor()).await {
                while wakes.try_next().is_some() {}
                let Some(bus) = bus.upgrade() else {
                    return;
                };
                bus.run_main();
                if this.update(cx, |_, _| ()).is_err() {
                    return;
                }
            }
        })
        .detach();
    }
}
