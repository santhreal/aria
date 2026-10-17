//! Scopes a component registers through `register-scope`: the host side of
//! the world's `scope-measure` export.

use std::sync::Arc;

use aria_plugin::{PluginContext, Scope, ScopeFrame, ScopeImage};
use parking_lot::Mutex;

use crate::bindings;
use crate::host::WasmPluginShared;

pub(crate) struct WasmScope {
    shared: Arc<WasmPluginShared>,
    id: String,
    /// The frame record handed to the component, refilled on each call so a
    /// frame copies into the store without a new allocation.
    frame: Mutex<bindings::ScopeFrame>,
}

impl WasmScope {
    pub(crate) fn new(shared: Arc<WasmPluginShared>, id: String) -> Self {
        Self {
            shared,
            id,
            frame: Mutex::new(bindings::ScopeFrame {
                width: 0,
                height: 0,
                pixels: Vec::new(),
            }),
        }
    }
}

impl Scope for WasmScope {
    fn measure(
        &self,
        ctx: &PluginContext,
        frame: &ScopeFrame<'_>,
        image: &mut ScopeImage,
    ) -> Result<(), String> {
        let drawing = {
            let mut held = self.frame.lock();
            held.width = frame.width();
            held.height = frame.height();
            held.pixels.clear();
            held.pixels.extend_from_slice(frame.pixels());
            self.shared.call_scope_measure(ctx, &self.id, &held)?
        };
        let (width, height) = (image.width(), image.height());
        let target = image.pixels_mut();
        if !drawing.pixels.is_empty() {
            if drawing.pixels.len() != target.len() {
                return Err(format!(
                    "scope {} drew {} bytes; its {width}x{height} image is {} bytes",
                    self.id,
                    drawing.pixels.len(),
                    target.len()
                ));
            }
            target.copy_from_slice(&drawing.pixels);
        }
        image.set_readout(drawing.readout);
        Ok(())
    }
}
