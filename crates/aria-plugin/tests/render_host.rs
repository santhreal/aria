//! Frames a plugin asks the render host for.
//!
//! Closes the class "a frame request renders something other than what it
//! asked for": the source (the open sequence, the root or a nested one, or one
//! clip alone, including a side of a transition or a track matte), every size,
//! the straight alpha of an image, motion blur, every tick of a range, and a
//! texture target, including the textures it must refuse. Does not cover
//! decoded media, which the renderer's own tests exercise.

use std::ops::ControlFlow;

use aria_compositor::{CompositorError, GpuContext, wgpu};
use aria_models::{
    CanvasAspect, CanvasSettings, ClipId, ClipParam, ClipTransform, Easing, Generator, ModelError,
    MotionBlur, ParamValue, Project, Rational, RationalTime, TimeRange, TrackKind, TrackMatte,
};
use aria_plugin::{FrameRequest, FrameSize, FrameSource, RenderHost};
use aria_render::{RenderError, Renderer};

const FPS_24: Rational = Rational::FPS_24;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, FPS_24)
}

fn host() -> Option<RenderHost> {
    match GpuContext::new_headless_blocking() {
        Ok(gpu) => Some(RenderHost::new(gpu)),
        Err(err) => {
            eprintln!("skipping: no headless GPU available: {err}");
            None
        }
    }
}

fn solid(project: &mut Project, track: aria_models::TrackId, rgba: [u8; 4], start: i64) -> ClipId {
    project
        .add_generated(
            track,
            Generator::SolidColor { rgba },
            TimeRange::at_rate(start, 24, FPS_24),
        )
        .expect("adds a solid")
}

fn assert_near(actual: [u8; 4], expected: [u8; 4], what: &str) {
    for (a, e) in actual.iter().zip(expected) {
        assert!(
            a.abs_diff(e) <= 3,
            "{what}: got {actual:?}, expected ~{expected:?}"
        );
    }
}

/// Opaque red on the lower lane and half-transparent green above it, both at
/// 0..24 on the root sequence.
struct Stack {
    project: Project,
    red: ClipId,
    green: ClipId,
}

fn stack() -> Stack {
    let mut project = Project::new("p", FPS_24);
    let lower = project.add_track(TrackKind::Sticker, "Lower");
    let upper = project.add_track(TrackKind::Sticker, "Upper");
    let red = solid(&mut project, lower, [255, 0, 0, 255], 0);
    let green = solid(&mut project, upper, [0, 255, 0, 128], 0);
    Stack {
        project,
        red,
        green,
    }
}

#[test]
fn a_sequence_frame_is_the_frame_the_renderer_draws() {
    let Some(host) = host() else { return };
    let Stack { project, .. } = stack();
    let request = FrameRequest::open();
    let image = host.frame(&project, rt(1), &request).expect("a frame");
    assert_eq!((image.width, image.height), (1920, 1080));
    assert_eq!(
        host.frame_size(&project, &request).expect("a size"),
        (1920, 1080)
    );
    let mut renderer = Renderer::new_headless().expect("the host found a device");
    let expected = renderer.render_frame(&project, rt(1)).expect("a frame");
    assert!(
        image.pixels == expected.pixels,
        "the host drew another frame"
    );
    // Green at half coverage over red.
    assert_near(image.pixel(960, 540), [127, 128, 0, 255], "sequence center");
}

/// The root sequence and a nested one, each asked for while the other is
/// open, and a clip of each while either is open.
#[test]
fn a_sequence_frame_shows_the_sequence_asked_for_whichever_is_open() {
    let Some(host) = host() else { return };
    let Stack {
        mut project, red, ..
    } = stack();
    let nest = project.create_sequence("Nest", FPS_24);
    project.set_active_sequence(Some(nest)).expect("opens");
    let lane = project.add_track(TrackKind::Sticker, "Nested");
    let blue = solid(&mut project, lane, [0, 0, 255, 255], 0);
    project.set_active_sequence(None).expect("opens the root");

    let centre = |request: FrameRequest, project: &Project| {
        host.frame(project, rt(1), &request)
            .expect("a frame")
            .pixel(960, 540)
    };
    let root = [127, 128, 0, 255];
    let nested = [0, 0, 255, 255];
    assert_near(centre(FrameRequest::open(), &project), root, "open root");
    assert_near(centre(FrameRequest::sequence(None), &project), root, "root");
    assert_near(
        centre(FrameRequest::sequence(Some(nest)), &project),
        nested,
        "nested while the root is open",
    );
    assert_near(
        centre(FrameRequest::clip(blue), &project),
        nested,
        "a nested clip while the root is open",
    );

    project.set_active_sequence(Some(nest)).expect("opens");
    assert_near(centre(FrameRequest::open(), &project), nested, "open nest");
    assert_near(
        centre(FrameRequest::clip(blue), &project),
        nested,
        "a clip of the open nest",
    );
    assert_near(
        centre(FrameRequest::clip(red), &project),
        [255, 0, 0, 255],
        "a root clip while the nest is open",
    );
    assert_near(
        centre(FrameRequest::sequence(None), &project),
        root,
        "root while the nest is open",
    );
    assert_eq!(
        project.active_sequence(),
        Some(nest),
        "the request changed the project"
    );

    let missing = aria_models::SequenceId::from_raw(9_999);
    let err = host
        .frame(&project, rt(1), &FrameRequest::sequence(Some(missing)))
        .expect_err("no such sequence");
    assert!(
        matches!(err, RenderError::Model(ModelError::UnknownSequence(id)) if id == missing),
        "{err}"
    );
}

#[test]
fn a_clip_frame_holds_that_clip_alone_in_straight_alpha() {
    let Some(host) = host() else { return };
    let Stack {
        project,
        red,
        green,
    } = stack();
    let image = host
        .frame(&project, rt(1), &FrameRequest::clip(green))
        .expect("a frame");
    // Premultiplied would read [0, 128, 0, 128]; the red below is not drawn.
    assert_near(image.pixel(960, 540), [0, 255, 0, 128], "green alone");
    let image = host
        .frame(&project, rt(1), &FrameRequest::clip(red))
        .expect("a frame");
    // The green lane above it is not drawn either.
    assert_near(image.pixel(960, 540), [255, 0, 0, 255], "red alone");
}

/// Red cuts to blue at tick 24 under a 24-tick crossfade, so tick 24 is the
/// middle of the blend.
#[test]
fn a_clip_in_a_transition_is_drawn_without_the_other_side() {
    let Some(host) = host() else { return };
    let mut project = Project::new("p", FPS_24);
    let lane = project.add_track(TrackKind::Sticker, "Lane");
    let red = solid(&mut project, lane, [255, 0, 0, 255], 0);
    let blue = solid(&mut project, lane, [0, 0, 255, 255], 24);
    project
        .add_transition(red, "crossfade")
        .expect("a crossfade");
    let centre = |request: FrameRequest| {
        host.frame(&project, rt(24), &request)
            .expect("a frame")
            .pixel(960, 540)
    };
    assert_near(
        centre(FrameRequest::open()),
        [128, 0, 128, 255],
        "the blend",
    );
    assert_near(
        centre(FrameRequest::clip(red)),
        [255, 0, 0, 255],
        "outgoing alone",
    );
    assert_near(
        centre(FrameRequest::clip(blue)),
        [0, 0, 255, 255],
        "incoming alone",
    );
}

/// An opaque stencil above an inverted alpha matte cuts the red clip away
/// entirely, and the stencil itself is never drawn in the sequence.
#[test]
fn a_clip_in_a_track_matte_is_drawn_without_the_other_layer() {
    let Some(host) = host() else { return };
    let mut project = Project::new("p", FPS_24);
    let lower = project.add_track(TrackKind::Sticker, "Lower");
    let upper = project.add_track(TrackKind::Sticker, "Upper");
    let red = solid(&mut project, lower, [255, 0, 0, 255], 0);
    let stencil = solid(&mut project, upper, [0, 0, 255, 255], 0);
    project
        .set_clip_matte(
            red,
            Some(TrackMatte {
                invert: true,
                ..TrackMatte::alpha()
            }),
        )
        .expect("a matte");
    let centre = |request: FrameRequest| {
        host.frame(&project, rt(1), &request)
            .expect("a frame")
            .pixel(960, 540)
    };
    assert_near(centre(FrameRequest::open()), [0, 0, 0, 255], "red cut away");
    assert_near(
        centre(FrameRequest::clip(red)),
        [255, 0, 0, 255],
        "red unmatted",
    );
    assert_near(
        centre(FrameRequest::clip(stencil)),
        [0, 0, 255, 255],
        "the stencil",
    );
}

#[test]
fn a_clip_not_drawn_at_the_time_is_a_transparent_frame() {
    let Some(host) = host() else { return };
    let Stack { project, green, .. } = stack();
    let image = host
        .frame(&project, rt(30), &FrameRequest::clip(green))
        .expect("a frame");
    assert_eq!((image.width, image.height), (1920, 1080));
    assert!(image.pixels.iter().all(|&byte| byte == 0));
}

#[test]
fn a_clip_that_is_not_in_the_project_is_refused() {
    let Some(host) = host() else { return };
    let Stack { project, .. } = stack();
    let missing = ClipId::from_raw(9_999);
    let err = host
        .frame(&project, rt(1), &FrameRequest::clip(missing))
        .expect_err("no such clip");
    assert!(
        matches!(err, RenderError::Model(ModelError::UnknownClip(id)) if id == missing),
        "{err}"
    );
}

#[test]
fn every_size_reports_and_renders_the_same_dimensions() {
    let Some(host) = host() else { return };
    let Stack { project, green, .. } = stack();
    let cases = [
        (FrameSize::Canvas, (1920, 1080)),
        (
            FrameSize::Fit {
                max_width: 480,
                max_height: 480,
            },
            (480, 270),
        ),
        // Fit never enlarges.
        (
            FrameSize::Fit {
                max_width: 4000,
                max_height: 4000,
            },
            (1920, 1080),
        ),
        (
            FrameSize::Exact {
                width: 400,
                height: 400,
            },
            (400, 400),
        ),
    ];
    for source in [
        FrameSource::Open,
        FrameSource::Sequence(None),
        FrameSource::Clip(green),
    ] {
        for (size, expected) in cases {
            let request = FrameRequest {
                source,
                size,
                motion_blur: false,
            };
            let reported = host.frame_size(&project, &request).expect("a size");
            assert_eq!(reported, expected, "{request:?}");
            let image = host.frame(&project, rt(1), &request).expect("a frame");
            assert_eq!((image.width, image.height), expected, "{request:?}");
        }
    }
    // An exact square letterboxes a 16:9 canvas: the top row is background,
    // black for the sequence and transparent for a clip.
    let exact = FrameSize::Exact {
        width: 400,
        height: 400,
    };
    let image = host
        .frame(&project, rt(1), &FrameRequest::open().size(exact))
        .expect("a frame");
    assert_eq!(image.pixel(200, 0), [0, 0, 0, 255]);
    assert_near(image.pixel(200, 200), [127, 128, 0, 255], "sequence center");
    let image = host
        .frame(&project, rt(1), &FrameRequest::clip(green).size(exact))
        .expect("a frame");
    assert_eq!(image.pixel(200, 0), [0, 0, 0, 0]);
    assert_near(image.pixel(200, 200), [0, 255, 0, 128], "clip center");
}

#[test]
fn frames_renders_each_tick_of_the_range_in_order_and_stops_on_break() {
    let Some(host) = host() else { return };
    let Stack { mut project, .. } = stack();
    let top = project.add_track(TrackKind::Sticker, "Top");
    solid(&mut project, top, [0, 0, 255, 255], 2);
    let request = FrameRequest::open().size(FrameSize::Fit {
        max_width: 64,
        max_height: 64,
    });
    let mut seen = Vec::new();
    host.frames(
        &project,
        TimeRange::at_rate(0, 4, FPS_24),
        &request,
        |time, image| {
            seen.push((time.value, image.pixel(32, 18)));
            ControlFlow::Continue(())
        },
    )
    .expect("frames");
    let ticks: Vec<i64> = seen.iter().map(|(tick, _)| *tick).collect();
    assert_eq!(ticks, [0, 1, 2, 3]);
    for (tick, pixel) in &seen {
        let expected = if *tick < 2 {
            [127, 128, 0, 255]
        } else {
            [0, 0, 255, 255]
        };
        assert_near(*pixel, expected, &format!("tick {tick}"));
    }

    let mut seen = Vec::new();
    host.frames(
        &project,
        TimeRange::at_rate(0, 4, FPS_24),
        &request,
        |time, _| {
            seen.push(time.value);
            if seen.len() == 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
    )
    .expect("frames");
    assert_eq!(seen, [0, 1]);
}

#[test]
fn motion_blur_follows_the_request() {
    let Some(host) = host() else { return };
    let mut project = Project::new("p", FPS_24);
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Square1x1,
        background: [0, 0, 0],
        resolution: None,
    });
    let track = project.add_track(TrackKind::Sticker, "S1");
    let clip = solid(&mut project, track, [255, 255, 255, 255], 0);
    project
        .set_transform(
            clip,
            ClipTransform {
                scale: 0.1.into(),
                ..ClipTransform::IDENTITY
            },
            None,
        )
        .expect("a transform");
    for (tick, x) in [(0, -0.35), (2, 0.35)] {
        project
            .set_param_keyframe(
                clip,
                ClipParam::Position,
                rt(tick),
                ParamValue::Vec2([x, 0.0]),
                Easing::Linear,
                None,
            )
            .expect("a keyframe");
    }
    project
        .set_motion_blur(
            clip,
            MotionBlur {
                enabled: true,
                shutter_deg: 360.0,
                samples: 8,
            },
        )
        .expect("motion blur");

    let mut renderer = Renderer::new_headless().expect("the host found a device");
    let sharp = renderer.render_frame(&project, rt(1)).expect("a frame");
    renderer.set_apply_motion_blur(true);
    let blurred = renderer.render_frame(&project, rt(1)).expect("a frame");
    assert!(sharp.pixels != blurred.pixels, "the fixture does not blur");

    let request = FrameRequest::open();
    let image = host.frame(&project, rt(1), &request).expect("a frame");
    assert!(image.pixels == sharp.pixels, "blurred without asking");
    let image = host
        .frame(&project, rt(1), &request.motion_blur(true))
        .expect("a frame");
    assert!(image.pixels == blurred.pixels, "asked for blur, got none");
    // The flag is per request, not left on the shared renderer.
    let image = host.frame(&project, rt(1), &request).expect("a frame");
    assert!(
        image.pixels == sharp.pixels,
        "the last request's blur stuck"
    );
}

fn target(
    gpu: &GpuContext,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test.frame_target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// Copy `texture` back to the CPU as tightly packed RGBA rows.
fn read_back(gpu: &GpuContext, texture: &wgpu::Texture) -> Vec<u8> {
    let size = texture.size();
    let padded = (size.width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test.frame_readback"),
        size: u64::from(padded) * u64::from(size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(size.height),
            },
        },
        size,
    );
    gpu.queue.submit(std::iter::once(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("maps"));
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the device finishes");
    let mapped = slice.get_mapped_range();
    let row = (size.width * 4) as usize;
    mapped
        .chunks(padded as usize)
        .flat_map(|chunk| &chunk[..row])
        .copied()
        .collect()
}

#[test]
fn a_frame_lands_in_a_texture_as_the_image_premultiplied() {
    let Some(host) = host() else { return };
    let Stack { project, green, .. } = stack();
    let gpu = host.gpu();
    let usage = wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC;
    // An opaque frame reads the same either way; the translucent clip tells
    // premultiplied bytes from straight ones.
    for (request, translucent) in [
        (FrameRequest::open(), false),
        (
            FrameRequest::clip(green).size(FrameSize::Fit {
                max_width: 320,
                max_height: 320,
            }),
            true,
        ),
    ] {
        let (width, height) = host.frame_size(&project, &request).expect("a size");
        let texture = target(gpu, width, height, wgpu::TextureFormat::Rgba8Unorm, usage);
        host.frame_to_texture(&project, rt(1), &request, &texture)
            .expect("renders into the texture");
        let expected = host.frame(&project, rt(1), &request).expect("a frame");
        // The texture holds the premultiplied bytes the image was straightened
        // from, so straightening them gives the image byte for byte.
        let mut written = aria_plugin::RgbaImage::new(width, height, read_back(gpu, &texture));
        if translucent {
            assert!(
                written.pixels != expected.pixels,
                "{request:?}: the texture is not premultiplied"
            );
        }
        written.unpremultiply();
        assert!(
            written.pixels == expected.pixels,
            "{request:?}: the texture holds another frame"
        );
    }
}

#[test]
fn a_texture_of_another_size_format_or_usage_is_refused() {
    let Some(host) = host() else { return };
    let Stack { project, .. } = stack();
    let gpu = host.gpu();
    let request = FrameRequest::open().size(FrameSize::Fit {
        max_width: 64,
        max_height: 64,
    });
    let (width, height) = host.frame_size(&project, &request).expect("a size");
    let copy = wgpu::TextureUsages::COPY_DST;
    let rgba = wgpu::TextureFormat::Rgba8Unorm;
    let refused = [
        ("wider", target(gpu, width + 1, height, rgba, copy)),
        ("shorter", target(gpu, width, height - 1, rgba, copy)),
        (
            "bgra",
            target(gpu, width, height, wgpu::TextureFormat::Bgra8Unorm, copy),
        ),
        (
            "not a copy destination",
            target(
                gpu,
                width,
                height,
                rgba,
                wgpu::TextureUsages::TEXTURE_BINDING,
            ),
        ),
    ];
    for (what, texture) in refused {
        let err = host
            .frame_to_texture(&project, rt(1), &request, &texture)
            .expect_err(what);
        assert!(
            matches!(
                &err,
                RenderError::Compositor(CompositorError::FrameTarget {
                    width: w,
                    height: h,
                    ..
                }) if (*w, *h) == (width, height)
            ),
            "{what}: {err}"
        );
    }
}
