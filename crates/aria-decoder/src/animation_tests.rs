//! Animated-image decode tests.

use super::*;

// The bundled sticker pack doubles as real-world fixtures: a Pillow GIF
// (full-frame patches, disposal=Background) and APNG, both 12 frames at
// 80 ms.
const STAR_SPIN_GIF: &[u8] = include_bytes!("../../../assets/stickers/star_spin.gif");
const HEART_BEAT_APNG: &[u8] = include_bytes!("../../../assets/stickers/heart_beat.png");
const HEART_PNG: &[u8] = include_bytes!("../../../assets/stickers/heart.png");

fn pixel(image: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * image.width + x) * 4) as usize;
    image.pixels[i..i + 4].try_into().unwrap()
}

#[track_caller]
fn assert_twelve_frames_at_80ms(frames: &[AnimationFrame]) {
    assert_eq!(frames.len(), 12);
    for frame in frames {
        assert_eq!((frame.image.width, frame.image.height), (128, 128));
        assert_eq!(frame.delay_ms, 80);
    }
    // Every frame draws something in the middle over a transparent edge.
    for frame in frames {
        assert_eq!(pixel(&frame.image, 0, 0)[3], 0, "corner should be clear");
        assert_ne!(pixel(&frame.image, 64, 64)[3], 0, "center should be art");
    }
}

#[test]
fn decodes_the_bundled_gif_animation() {
    assert_twelve_frames_at_80ms(&decode_animation(STAR_SPIN_GIF).unwrap());
}

#[test]
fn decodes_the_bundled_apng_animation() {
    assert_twelve_frames_at_80ms(&decode_animation(HEART_BEAT_APNG).unwrap());
}

#[test]
fn portable_gif_matches_the_contract() {
    assert_twelve_frames_at_80ms(&gif_decode(STAR_SPIN_GIF).unwrap());
}

#[test]
fn portable_apng_matches_the_contract() {
    assert_twelve_frames_at_80ms(&apng_decode(HEART_BEAT_APNG).unwrap());
}

#[test]
fn static_png_decodes_as_a_single_frame() {
    let frames = decode_animation(HEART_PNG).unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!((frames[0].image.width, frames[0].image.height), (256, 256));
}

#[test]
fn unrecognized_bytes_error() {
    assert!(decode_animation(b"plain text, not an image").is_err());
}

#[test]
fn compositor_applies_patch_blend_and_dispose() {
    let mut c = Compositor::new(4, 4).unwrap();
    let red = [255, 0, 0, 255];
    let full: Vec<u8> = red.repeat(16);
    // Frame 1: full red canvas, keep it around.
    let f1 = c.frame((0, 0, 4, 4), &full, Blend::Source, Dispose::Keep);
    assert_eq!(pixel(&f1, 3, 3), red);

    // Frame 2: 2x2 transparent patch at (1,1) blended Over changes
    // nothing (alpha 0 keeps the underlying pixel)...
    let clear: Vec<u8> = [0u8; 4].repeat(4);
    let f2 = c.frame((1, 1, 2, 2), &clear, Blend::Over, Dispose::Keep);
    assert_eq!(pixel(&f2, 1, 1), red);
    // ...while Source replaces alpha included, then Background disposal
    // clears the patched region for the following frame.
    let green = [0, 255, 0, 255];
    let patch: Vec<u8> = green.repeat(4);
    let f3 = c.frame((1, 1, 2, 2), &patch, Blend::Source, Dispose::Background);
    assert_eq!(pixel(&f3, 1, 1), green);
    assert_eq!(pixel(&f3, 0, 0), red);
    let f4 = c.frame((0, 0, 1, 1), &[red].concat(), Blend::Source, Dispose::Keep);
    assert_eq!(pixel(&f4, 1, 1), [0, 0, 0, 0], "background-disposed");
    assert_eq!(pixel(&f4, 3, 3), red, "outside the disposed region");
}

#[test]
fn compositor_previous_disposal_restores_the_canvas() {
    let mut c = Compositor::new(2, 2).unwrap();
    let blue = [0, 0, 255, 255];
    c.frame((0, 0, 2, 2), &blue.repeat(4), Blend::Source, Dispose::Keep);
    let white = [255; 4];
    let shown = c.frame(
        (0, 0, 2, 2),
        &white.repeat(4),
        Blend::Source,
        Dispose::Previous,
    );
    assert_eq!(pixel(&shown, 0, 0), white);
    let after = c.frame((0, 0, 1, 1), &blue, Blend::Over, Dispose::Keep);
    assert_eq!(pixel(&after, 1, 1), blue, "previous frame restored");
}

/// WHY: the previous-disposal scratch is retained across frames, so a later
/// frame must restore its own region rather than whatever the earlier frame
/// saved, and the buffer must not grow with the frame count. Covers a shrinking
/// region after a larger one, which is where a stale tail would survive.
#[test]
fn previous_disposal_reuses_one_scratch_buffer_across_frames() {
    let mut c = Compositor::new(2, 2).unwrap();
    let blue = [0, 0, 255, 255];
    c.frame((0, 0, 2, 2), &blue.repeat(4), Blend::Source, Dispose::Keep);
    let white = [255; 4];
    c.frame(
        (0, 0, 2, 2),
        &white.repeat(4),
        Blend::Source,
        Dispose::Previous,
    );
    let full_capacity = c.saved.capacity();
    assert_eq!(c.saved.len(), 16, "four pixels of the restored region");

    let red = [255, 0, 0, 255];
    let shown = c.frame((1, 1, 1, 1), &red, Blend::Source, Dispose::Previous);
    assert_eq!(pixel(&shown, 1, 1), red);
    assert_eq!(c.saved.len(), 4, "one pixel, not the earlier four");
    assert_eq!(
        c.saved.capacity(),
        full_capacity,
        "the smaller region reused the allocation"
    );

    // A fully transparent patch under `Over` leaves the canvas alone, so the
    // snapshot reports what disposal actually restored.
    let transparent = [0u8, 0, 0, 0];
    let after = c.frame(
        (0, 0, 2, 2),
        &transparent.repeat(4),
        Blend::Over,
        Dispose::Keep,
    );
    assert_eq!(
        pixel(&after, 1, 1),
        blue,
        "the one-pixel region was restored to blue, not left red"
    );

    for _ in 0..32 {
        c.frame(
            (0, 0, 2, 2),
            &white.repeat(4),
            Blend::Source,
            Dispose::Previous,
        );
    }
    assert_eq!(
        c.saved.capacity(),
        full_capacity,
        "thirty-two more disposals allocated nothing further"
    );
}

#[test]
fn over_blend_composites_straight_alpha() {
    // 50% white over opaque black = mid gray, still opaque.
    let mut dst = [0u8, 0, 0, 255];
    over(&mut dst, &[255, 255, 255, 128]);
    assert_eq!(dst[3], 255);
    assert!((dst[0] as i32 - 128).abs() <= 1, "{dst:?}");
    // Anything over fully transparent is just the source.
    let mut dst = [0u8; 4];
    over(&mut dst, &[10, 20, 30, 40]);
    assert_eq!(dst, [10, 20, 30, 40]);
}

#[test]
fn oversized_animations_are_rejected() {
    assert!(Compositor::new(MAX_ANIMATION_DIMENSION + 1, 2).is_err());
    assert!(Compositor::new(0, 2).is_err());
}
