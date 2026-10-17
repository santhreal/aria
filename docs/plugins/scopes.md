# Scopes

A scope measures the Viewer frame and draws what it measured: a false-colour
map, a zebra count, a per-channel parade, a loudness-style number. The scope
strip shows every plugin scope below the built-in waveform, vectorscope, and
histogram, with its caption, the registering plugin's name, the drawn image,
and one line of readout.

```rust
use aria_plugin::{PluginContext, PluginError, ScopeFrame, ScopeImage, ScopeSpec};

/// Counts pixels at or above 235 in any channel and marks their columns.
fn clipping(
    _: &PluginContext,
    frame: &ScopeFrame<'_>,
    image: &mut ScopeImage,
) -> Result<(), String> {
    let mut clipped = 0usize;
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            let [r, g, b, _] = frame.pixel(x, y).unwrap_or_default();
            if r.max(g).max(b) >= 235 {
                clipped += 1;
                let column = x * image.width() / frame.width();
                image.put(column, 0, [255, 64, 64, 255]);
            }
        }
    }
    image.set_readout(format!("{clipped} px clipped"));
    Ok(())
}

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_scope(
        ScopeSpec {
            id: "example.clipping".into(),
            name: "Clipping".into(),
            width: 256,
            height: 1,
        },
        clipping,
    )?;
    Ok(())
}
```

## The frame

`ScopeFrame` is the frame on screen in the Viewer: RGBA8, sRGB encoded,
straight alpha, top row first, `width() * height() * 4` bytes. It is the
same frame the built-in scopes read, at the size the Viewer rendered it,
which can be smaller than the sequence. `pixel(x, y)` returns `None` outside
the frame.

## The image

`ScopeImage` is `width` by `height` from the spec, transparent black on entry
to every call. `put(x, y, rgba)` sets one pixel and ignores a position outside
the image; `pixels_mut()` exposes the whole buffer. `set_readout(text)` sets
the line under the image. The host keeps one image per scope and clears it in
place, so a measurement allocates nothing on the host side.

The strip draws the image 192 pixels wide and as tall as the spec's aspect
ratio makes it, up to 384 pixels; the Scopes panel draws it 192 pixels wide
beside the built-in scopes. Draw at the resolution the reading needs; the
image is scaled, not cropped.

## When it runs

Scopes run on the preview worker, never on the window's thread, and only while
the scope strip is open or the Scopes panel is the front tab of its frame. Each
Viewer frame is measured where it is rendered, and showing either measures the
frame already on screen. During playback at most 30 frames a second are
measured, and the strip holds the last reading between them; outside playback a
reading is shown only under the frame it was measured from. While a plugin
scope is registered, Viewer frames are read back to the CPU for it; otherwise
the built-in scopes count a frame on the GPU.

The time a measurement takes is recorded as the plugin's frame cost and shown
in the Plugins list. A slow scope delays the next preview frame, so keep the
pass to one sweep over the pixels.

## Failure

| Outcome | The strip shows |
| --- | --- |
| `Ok(())` | The image and the readout |
| `Err(reason)` | `reason` in place of the image |
| A readout over `MAX_SCOPE_READOUT` (160) bytes or holding a line break | The refusal in place of the image |
| A panic | The panic message for that frame; the plugin is failed for the session and its scopes leave the strip |

`register_scope` returns `PluginError::ScopeRefused(message)` when the id is
empty, over 128 bytes, or holds a byte other than an ASCII letter, digit, `.`,
`-` or `_`; when the name is blank; when a side is outside 1 to
`MAX_SCOPE_SIDE` (512); and when any plugin already registered the id.
The returned `Registration` is held on the terms of `register_effect`. A scope
whose registration is removed leaves the strip on the next frame.

## WebAssembly and native C ABI plugins

A WebAssembly component registers a scope during `mount` with
`register-scope(scope-spec { id, name, width, height })` and measures in
`scope-measure(id, scope-frame { width, height, pixels }) -> result<scope-drawing, string>`.
`scope-drawing { pixels, readout }` holds the whole image, `width * height * 4`
bytes, or an empty list to leave it transparent. Any other length fails the
measurement. The frame is copied into the component on every call, so each
measurement runs under `Limits::fuel_per_call` and `Limits::call_deadline`; a
trap fails the plugin. The example plugin draws a luma histogram,
`example.luma-histogram`.

A native `cdylib` plugin built with `aria-sdk` registers with
`host.register_scope(&ScopeSpec { .. })` during `Plugin::mount` and measures in
`Plugin::scope_measure`, which returns the readout:

```rust
impl aria_sdk::Plugin for Meter {
    fn mount(&self, host: &aria_sdk::Host) -> Result<(), String> {
        host.register_scope(&aria_sdk::ScopeSpec {
            id: "meter.red".into(),
            name: "Red".into(),
            width: 1,
            height: 64,
        })
    }

    fn scope_measure(
        &self,
        _host: &aria_sdk::Host,
        _id: &str,
        frame: &aria_sdk::ScopeFrame<'_>,
        image: &mut aria_sdk::ScopeImage<'_>,
    ) -> Result<String, String> {
        let pixels = frame.pixels.len() / 4;
        let sum: usize = frame.pixels.chunks_exact(4).map(|px| usize::from(px[0])).sum();
        let mean = sum / pixels.max(1);
        let rows = (mean * image.height as usize / 255) as u32;
        for y in 0..rows {
            image.put(0, image.height - 1 - y, [255, 80, 80, 255]);
        }
        Ok(format!("mean red {mean}"))
    }
}
```

The runtime checks that the frame and image buffers match their dimensions
before calling `scope_measure`. A host table older than the scope prefix fails
`register_scope` with `host table does not support scopes`.
