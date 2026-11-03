//! Work the event bus brings to the window: edits made away from it, and
//! notification handlers that run on its thread.
//!
//! The project host posts each committed command as `CommandApplied`, and the
//! bus delivers it on its delivery thread. The window re-projects when one
//! arrives from any origin but the editor, whose edits re-project where they
//! are made. A handler a plugin registers with `on_event_main` is queued by the
//! delivery thread and run here.

use std::sync::Arc;

use aria_plugin::host_wire::{SessionChange, rescale_time, take_session_changes};
use aria_plugin::{CommandApplied, Origin, TransportRequest};
use gpui::Context;

use super::AppState;
use crate::transport::ShuttleSpeed;

impl AppState {
    /// Apply any pending project edits, session changes (`open`/`save`/`save_as`),
    /// and transport requests (`seek`/`play`/`stop`) made by plugins on the shared
    /// [`ProjectHost`](aria_plugin::ProjectHost) and [`EditorView`](aria_plugin::EditorView).
    pub(crate) fn sync_from_plugin_host(&mut self) {
        let session_changes = take_session_changes(&self.host);
        if !session_changes.is_empty() {
            for change in session_changes {
                match change {
                    SessionChange::Opened { path } => {
                        crate::projection::invalidate_media_statuses();
                        self.waveforms.clear();
                        self.clear_recovery();
                        if let Some(p) = &path {
                            self.remember_recent(p);
                        }
                        self.project_path = path;
                        self.load_conversation();
                        self.enter_editor();
                        self.sync_from_engine();
                    }
                    SessionChange::Saved { path } => {
                        self.project_path = Some(path.clone());
                        self.remember_recent(&path);
                        self.clear_recovery();
                        if let Err(err) = self.save_conversation() {
                            tracing::warn!(error = %err, "could not save the conversation");
                        }
                        self.status_message = Some("Saved".into());
                        self.sync_from_engine();
                    }
                }
            }
        } else {
            self.sync_from_engine();
        }

        let transport_requests = self.editor_view.take_transport_requests();
        if !transport_requests.is_empty() {
            for req in transport_requests {
                match req {
                    TransportRequest::Seek { at } => {
                        self.set_playhead(rescale_time(at, self.playhead.rate).value);
                    }
                    TransportRequest::Play { speed } => {
                        self.set_shuttle_speed(ShuttleSpeed::steps(speed));
                    }
                    TransportRequest::Stop => {
                        self.set_shuttle(0);
                    }
                }
            }
            self.publish_view();
        }
    }

    /// Install the one subscriber that brings edits and transport controls from
    /// other origins onto the window. The handler runs on the event bus's
    /// delivery thread and only sends a wake; a task on the window's thread
    /// re-projects.
    pub fn watch_edits(&mut self, cx: &mut Context<Self>) {
        if self.edits_watch.is_some() {
            return;
        }
        let (wake, wakes) = crate::handoff::wakes::<()>();
        let transport_wake = wake.clone();
        self.editor_view.set_transport_waker(move || {
            transport_wake.wake(());
        });
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
                    state.sync_from_plugin_host();
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
