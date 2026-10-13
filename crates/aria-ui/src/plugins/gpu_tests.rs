//! The effect and transition services a plugin renders images with.
//!
//! Closes the class "a service draws but returns the wrong alpha convention or
//! accepts a request it cannot honour": output is straight alpha like the
//! input, and an unknown id, a size mismatch, and a missing device are each an
//! error instead of an image. The drawing tests mount the plugins a session
//! mounts, so the device is the one `aria.render` brings up. Uses shipped ids
//! only; registering from a lib test would change the catalog other tests read.

use super::*;

const SIZE: u32 = 32;

fn solid(rgba: [u8; 4]) -> RgbaImage {
    let pixels = rgba.repeat((SIZE * SIZE) as usize);
    RgbaImage::new(SIZE, SIZE, pixels)
}

/// The plugins the render services depend on, mounted as a session mounts
/// them, or `None` on a host with no GPU.
fn mounted() -> Option<PluginContext> {
    let ctx = PluginContext::new();
    let plugins: [&dyn Plugin; 5] = [
        &crate::plugins::DecoderPlugin,
        &CompositorPlugin,
        &crate::plugins::RenderPlugin,
        &EffectPlugin,
        &TransitionPlugin,
    ];
    for plugin in plugins {
        ctx.mount(plugin).expect("mounts");
    }
    if ctx.try_get::<GpuContext>().is_none() {
        eprintln!("skipping: no GPU device mounted");
        return None;
    }
    Some(ctx)
}

fn without_device() -> OneShot {
    OneShot {
        gpu: None,
        compositor: Mutex::new(None),
    }
}

fn assert_near(actual: [u8; 4], expected: [u8; 4], what: &str) {
    for (a, e) in actual.iter().zip(expected) {
        assert!(
            a.abs_diff(e) <= 3,
            "{what}: got {actual:?}, expected ~{expected:?}"
        );
    }
}

fn centre(image: &RgbaImage) -> [u8; 4] {
    image.pixel(SIZE / 2, SIZE / 2)
}

/// A half-transparent input keeps its colour: the readback is converted back
/// from the compositor's premultiplied form.
#[test]
fn an_effect_returns_straight_alpha() {
    let Some(ctx) = mounted() else {
        return;
    };
    let service = ctx
        .try_get::<dyn EffectService>()
        .expect("aria.effects registers the service");
    let input = solid([200, 100, 50, 128]);
    // Pixelate over one colour changes nothing, so the output is the input.
    let output = service
        .render_effect("pixelate", &[8.0], &input)
        .expect("a shipped effect renders");
    assert_eq!((output.width, output.height), (SIZE, SIZE));
    assert_near(centre(&output), [200, 100, 50, 128], "pixelated solid");
}

#[test]
fn an_unknown_effect_is_an_error() {
    let service = EffectCatalog(without_device());
    let err = service
        .render_effect("test.gpu.nothing", &[], &solid([0, 0, 0, 255]))
        .expect_err("no pass renders it");
    assert!(
        err.to_string()
            .contains("no pass renders effect `test.gpu.nothing`"),
        "{err}"
    );
}

#[test]
fn without_a_device_an_effect_is_an_error() {
    let service = EffectCatalog(without_device());
    let err = service
        .render_effect("pixelate", &[8.0], &solid([0, 0, 0, 255]))
        .expect_err("nothing to draw on");
    assert!(err.to_string().contains("no GPU device"), "{err}");
}

/// The blend runs on premultiplied colour and the result is returned
/// straight: half of opaque red over nothing is red at half alpha.
#[test]
fn a_transition_blends_and_returns_straight_alpha() {
    let Some(ctx) = mounted() else {
        return;
    };
    let service = ctx
        .try_get::<dyn TransitionService>()
        .expect("aria.transitions registers the service");
    let red = solid([255, 0, 0, 255]);
    let blue = solid([0, 0, 255, 255]);
    let opaque = service
        .render_transition("crossfade", &red, &blue, 0.5)
        .expect("a shipped transition renders");
    assert_near(centre(&opaque), [128, 0, 128, 255], "red to blue at 0.5");

    let clear = solid([0, 0, 0, 0]);
    let fading = service
        .render_transition("crossfade", &red, &clear, 0.5)
        .expect("a shipped transition renders");
    assert_near(centre(&fading), [255, 0, 0, 128], "red to nothing at 0.5");

    // Progress outside 0..1 holds at the end.
    let past = service
        .render_transition("crossfade", &red, &blue, 3.0)
        .expect("renders");
    assert_near(centre(&past), [0, 0, 255, 255], "progress past the end");
}

#[test]
fn an_unknown_transition_is_an_error() {
    let service = TransitionCatalog(without_device());
    let image = solid([0, 0, 0, 255]);
    let err = service
        .render_transition("test.gpu.nothing", &image, &image, 0.5)
        .expect_err("no pass renders it");
    assert!(
        err.to_string()
            .contains("no pass renders transition `test.gpu.nothing`"),
        "{err}"
    );
}

#[test]
fn a_transition_between_sizes_is_an_error() {
    let service = TransitionCatalog(without_device());
    let small = RgbaImage::new(2, 2, vec![0; 16]);
    let err = service
        .render_transition("crossfade", &small, &solid([0, 0, 0, 255]), 0.5)
        .expect_err("the sizes differ");
    assert!(
        err.to_string()
            .contains("blends images of one size; got 2x2 and 32x32"),
        "{err}"
    );
}

#[test]
fn without_a_device_a_transition_is_an_error() {
    let service = TransitionCatalog(without_device());
    let image = solid([0, 0, 0, 255]);
    let err = service
        .render_transition("crossfade", &image, &image, 0.5)
        .expect_err("nothing to draw on");
    assert!(err.to_string().contains("no GPU device"), "{err}");
}
