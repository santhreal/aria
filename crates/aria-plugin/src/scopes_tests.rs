//! Tests for plugin scopes.
//!
//! Closes the class "a scope draws the wrong size, keeps a previous frame's
//! drawing, outlives its plugin, or takes the preview worker down": every case
//! runs through `register_scope` and `measure_scope`, the two entry points
//! every scope passes whatever tier registered it. It does not cover the scope
//! strip's layout; `aria-ui`'s preview tests own that.

use super::*;
use crate::plugin::Plugin;

type Mount = Box<dyn Fn(&PluginContext) -> Result<(), PluginError> + Send + Sync>;

/// A plugin whose mount runs `mount`.
struct WithScope {
    manifest: PluginManifest,
    mount: Mount,
}

impl Plugin for WithScope {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        (self.mount)(ctx)
    }
}

fn plugin(
    id: &'static str,
    mount: impl Fn(&PluginContext) -> Result<(), PluginError> + Send + Sync + 'static,
) -> WithScope {
    WithScope {
        manifest: PluginManifest::new(id, "Meters", "0.1.0", &[], &[]),
        mount: Box::new(mount),
    }
}

fn spec(id: &str, width: u32, height: u32) -> ScopeSpec {
    ScopeSpec {
        id: id.to_owned(),
        name: "Mean".to_owned(),
        width,
        height,
    }
}

/// Fills column 0 of `image` up to the frame's mean red value, scaled to the
/// image height, and reads the mean and the registering plugin back.
fn mean_red(
    ctx: &PluginContext,
    frame: &ScopeFrame<'_>,
    image: &mut ScopeImage,
) -> Result<(), String> {
    let pixels = frame.pixels();
    let count = pixels.len() / 4;
    let sum: usize = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| usize::from(px[0]))
        .sum();
    let mean = sum / count;
    let rows = (mean * image.height() as usize / 255) as u32;
    for y in 0..rows {
        image.put(0, image.height() - 1 - y, [255, 255, 255, 255]);
    }
    let scope = ctx
        .scope()
        .map_or_else(|| "host".to_owned(), |manifest| manifest.id.to_string());
    image.set_readout(format!("mean {mean} from {scope}"));
    Ok(())
}

/// A 2 by 1 frame with red values `a` and `b`.
fn frame_bytes(a: u8, b: u8) -> Vec<u8> {
    vec![a, 0, 0, 255, b, 0, 0, 255]
}

#[test]
fn a_scope_draws_at_its_size_through_its_plugins_view_and_records_cost() {
    let ctx = PluginContext::new();
    ctx.mount(&plugin("test.mean", |ctx| {
        ctx.register_scope(spec("test.mean.red", 2, 4), mean_red)?;
        Ok(())
    }))
    .expect("mounts");

    let scopes = ctx.scopes();
    assert_eq!(scopes.len(), 1);
    assert_eq!(&*scopes[0].plugin_id, "test.mean");
    assert_eq!(&*scopes[0].plugin_name, "Meters");
    assert_eq!(scopes[0].spec.id, "test.mean.red");

    let bytes = frame_bytes(255, 255);
    let frame = ScopeFrame::new(2, 1, &bytes).expect("a 2x1 frame");
    let mut image = ScopeImage::default();
    ctx.measure_scope("test.mean.red", &frame, &mut image)
        .expect("measures");

    assert_eq!((image.width(), image.height()), (2, 4));
    assert_eq!(image.pixels().len(), 2 * 4 * 4);
    let column_0: Vec<u8> = (0..4).map(|y| image.pixels()[(y * 2) * 4 + 3]).collect();
    assert_eq!(
        column_0,
        [255, 255, 255, 255],
        "a full red frame fills the column"
    );
    assert!(
        image.pixels()[4..8].iter().all(|b| *b == 0),
        "column 1 is untouched"
    );
    assert_eq!(image.readout(), "mean 255 from test.mean");

    let cost = ctx.cost_of("test.mean").expect("mounted");
    assert_eq!(cost.frame.calls, 1, "{cost:?}");
}

#[test]
fn each_measurement_starts_from_a_transparent_image_with_no_readout() {
    let ctx = PluginContext::new();
    ctx.mount(&plugin("test.mean", |ctx| {
        ctx.register_scope(spec("test.mean.red", 1, 4), mean_red)?;
        Ok(())
    }))
    .expect("mounts");

    let mut image = ScopeImage::new(9, 9);
    image.set_readout("left over");
    image.pixels_mut().fill(7);

    let bright = frame_bytes(255, 255);
    ctx.measure_scope(
        "test.mean.red",
        &ScopeFrame::new(2, 1, &bright).expect("frame"),
        &mut image,
    )
    .expect("measures");
    assert_eq!((image.width(), image.height()), (1, 4));
    assert!(image.pixels().iter().all(|b| *b == 255));

    let dark = frame_bytes(0, 0);
    ctx.measure_scope(
        "test.mean.red",
        &ScopeFrame::new(2, 1, &dark).expect("frame"),
        &mut image,
    )
    .expect("measures");
    assert!(
        image.pixels().iter().all(|b| *b == 0),
        "a dark frame draws nothing, and nothing of the bright frame is left"
    );
    assert_eq!(image.readout(), "mean 0 from test.mean");
}

#[test]
fn a_scope_error_is_returned_as_written_and_the_plugin_keeps_running() {
    let ctx = PluginContext::new();
    ctx.mount(&plugin("test.picky", |ctx| {
        ctx.register_scope(
            spec("test.picky", 4, 4),
            |_: &PluginContext, frame: &ScopeFrame<'_>, _: &mut ScopeImage| -> Result<(), String> {
                Err(format!(
                    "needs a square frame, not {}x{}",
                    frame.width(),
                    frame.height()
                ))
            },
        )?;
        Ok(())
    }))
    .expect("mounts");

    let bytes = frame_bytes(1, 2);
    let err = ctx
        .measure_scope(
            "test.picky",
            &ScopeFrame::new(2, 1, &bytes).expect("frame"),
            &mut ScopeImage::default(),
        )
        .expect_err("fails");
    assert_eq!(
        err,
        ScopeError::Failed("needs a square frame, not 2x1".into())
    );
    assert!(ctx.is_mounted("test.picky"));
}

#[test]
fn a_panicking_scope_fails_its_plugin_and_leaves_the_strip() {
    let ctx = PluginContext::new();
    ctx.mount(&plugin("test.boom", |ctx| {
        ctx.register_scope(
            spec("test.boom", 4, 4),
            |_: &PluginContext, _: &ScopeFrame<'_>, _: &mut ScopeImage| -> Result<(), String> {
                panic!("boom")
            },
        )?;
        Ok(())
    }))
    .expect("mounts");

    let bytes = frame_bytes(1, 2);
    let err = ctx
        .measure_scope(
            "test.boom",
            &ScopeFrame::new(2, 1, &bytes).expect("frame"),
            &mut ScopeImage::default(),
        )
        .expect_err("panics");
    assert_eq!(err, ScopeError::Panicked("boom".into()));
    assert!(!ctx.is_mounted("test.boom"));
    assert!(ctx.scopes().is_empty());
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].reason, "scope test.boom panicked: boom");
    assert_eq!(
        ctx.measure_scope(
            "test.boom",
            &ScopeFrame::new(2, 1, &bytes).expect("frame"),
            &mut ScopeImage::default(),
        ),
        Err(ScopeError::Unknown("test.boom".into()))
    );
}

#[test]
fn a_readout_over_one_line_or_the_byte_cap_fails_and_clears_the_image() {
    for readout in ["two\nlines".to_owned(), "x".repeat(MAX_SCOPE_READOUT + 1)] {
        let ctx = PluginContext::new();
        let text = readout.clone();
        ctx.mount(&plugin("test.chatty", move |ctx| {
            let text = text.clone();
            ctx.register_scope(
                spec("test.chatty", 1, 1),
                move |_: &PluginContext,
                      _: &ScopeFrame<'_>,
                      image: &mut ScopeImage|
                      -> Result<(), String> {
                    image.put(0, 0, [9, 9, 9, 9]);
                    image.set_readout(text.clone());
                    Ok(())
                },
            )?;
            Ok(())
        }))
        .expect("mounts");
        let bytes = frame_bytes(1, 2);
        let mut image = ScopeImage::default();
        let err = ctx
            .measure_scope(
                "test.chatty",
                &ScopeFrame::new(2, 1, &bytes).expect("frame"),
                &mut image,
            )
            .expect_err("refused");
        assert!(
            matches!(&err, ScopeError::Failed(reason) if reason.contains("readout")),
            "{err:?}"
        );
        assert_eq!(image.pixels(), [0, 0, 0, 0]);
        assert_eq!(image.readout(), "");
    }

    let ctx = PluginContext::new();
    let fits = "y".repeat(MAX_SCOPE_READOUT);
    let text = fits.clone();
    ctx.mount(&plugin("test.terse", move |ctx| {
        let text = text.clone();
        ctx.register_scope(
            spec("test.terse", 1, 1),
            move |_: &PluginContext,
                  _: &ScopeFrame<'_>,
                  image: &mut ScopeImage|
                  -> Result<(), String> {
                image.set_readout(text.clone());
                Ok(())
            },
        )?;
        Ok(())
    }))
    .expect("mounts");
    let bytes = frame_bytes(1, 2);
    let mut image = ScopeImage::default();
    ctx.measure_scope(
        "test.terse",
        &ScopeFrame::new(2, 1, &bytes).expect("frame"),
        &mut image,
    )
    .expect("a readout at the cap is accepted");
    assert_eq!(image.readout(), fits);
}

#[test]
fn a_scope_leaves_with_its_plugin_and_its_id_is_released() {
    let ctx = PluginContext::new();
    ctx.mount(&plugin("test.leaving", |ctx| {
        ctx.register_scope(spec("shared.id", 2, 2), mean_red)?;
        Ok(())
    }))
    .expect("mounts");
    let revision = ctx.scopes_revision();
    ctx.unmount("test.leaving").expect("unmounts");
    assert!(ctx.scopes().is_empty());
    assert!(ctx.scopes_revision() > revision);

    ctx.mount(&plugin("test.arriving", |ctx| {
        ctx.register_scope(spec("shared.id", 2, 2), mean_red)?;
        Ok(())
    }))
    .expect("the released id is free");
    assert_eq!(&*ctx.scopes()[0].plugin_id, "test.arriving");
}

#[test]
fn malformed_and_duplicate_scopes_are_refused() {
    let ctx = PluginContext::new();
    let _held = ctx
        .register_scope(spec("held.id", 2, 2), mean_red)
        .expect("the host registers");
    let cases = [
        (spec("", 2, 2), "must be 1 to"),
        (spec("has space", 2, 2), "must be 1 to"),
        (spec(&"a".repeat(129), 2, 2), "must be 1 to"),
        (
            ScopeSpec {
                name: "  ".into(),
                ..spec("no.name", 2, 2)
            },
            "needs a name",
        ),
        (spec("zero.wide", 0, 2), "not 0x2"),
        (spec("zero.tall", 2, 0), "not 2x0"),
        (spec("too.wide", MAX_SCOPE_SIDE + 1, 2), "pixels on a side"),
        (spec("too.tall", 2, MAX_SCOPE_SIDE + 1), "pixels on a side"),
        (spec("held.id", 2, 2), "a scope is registered as held.id"),
    ];
    for (spec, expected) in cases {
        let id = spec.id.clone();
        match ctx.register_scope(spec, mean_red) {
            Err(PluginError::ScopeRefused(reason)) => {
                assert!(reason.contains(expected), "{id:?}: {reason}");
            }
            other => panic!("{id:?} was not refused: {:?}", other.map(drop)),
        }
    }

    // A second plugin cannot take an id another plugin holds.
    let refused = ctx.mount(&plugin("test.clash", |ctx| {
        ctx.register_scope(spec("held.id", 2, 2), mean_red)?;
        Ok(())
    }));
    assert!(refused.is_err(), "a duplicate id across plugins mounted");
    assert_eq!(ctx.scopes().len(), 1);

    ctx.register_scope(spec("max.side", MAX_SCOPE_SIDE, MAX_SCOPE_SIDE), mean_red)
        .expect("a scope at the side cap registers");
}

#[test]
fn a_frame_must_match_its_dimensions() {
    let bytes = frame_bytes(1, 2);
    assert!(ScopeFrame::new(2, 1, &bytes).is_some());
    assert!(ScopeFrame::new(1, 1, &bytes).is_none(), "too many bytes");
    assert!(ScopeFrame::new(3, 1, &bytes).is_none(), "too few bytes");
    assert!(ScopeFrame::new(0, 0, &[]).is_none(), "zero sized");
    assert!(ScopeFrame::new(u32::MAX, u32::MAX, &bytes).is_none());

    let frame = ScopeFrame::new(2, 1, &bytes).expect("frame");
    assert_eq!(frame.pixel(1, 0), Some([2, 0, 0, 255]));
    assert_eq!(frame.pixel(2, 0), None);
    assert_eq!(frame.pixel(0, 1), None);

    let mut image = ScopeImage::new(2, 2);
    image.put(1, 1, [1, 2, 3, 4]);
    image.put(2, 0, [9, 9, 9, 9]);
    image.put(0, 2, [9, 9, 9, 9]);
    assert_eq!(&image.pixels()[12..16], [1, 2, 3, 4]);
    assert_eq!(image.pixels().iter().filter(|b| **b == 9).count(), 0);
}
