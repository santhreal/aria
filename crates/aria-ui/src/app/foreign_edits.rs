//! Edits made away from the window: a plugin's transaction, or an assistant
//! turn when it commits.
//!
//! The project host announces each committed command as `CommandApplied`, on the
//! thread that applied it. The window re-projects when one arrives from any
//! origin but the editor, whose edits re-project where they are made.

use aria_plugin::{CommandApplied, Origin};
use gpui::Context;

use super::AppState;

impl AppState {
    /// Install the one subscriber that brings edits from other origins onto the
    /// window. The handler runs on the applying thread and only sends a wake; a
    /// task on the window's thread re-projects.
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
}
