//! WHY: the GStreamer backend stated no rotation for any stream, so a video a
//! phone recorded upright probed at its sideways size and drew sideways, while
//! every other backend read the container's display matrix. These tests close
//! that class where both the probe and the decoder read it: for every
//! rotation a matrix can state, the probe reports the upright size, and the
//! decoder states the rotation in its source info and on each frame it
//! decodes. They do not cover a mirrored matrix, which reads as no rotation,
//! or drawing the frame upright, which the compositor tests cover.

use aria_core::{Rotation, VideoDecoder};

use super::*;
use crate::GstDecoder;
use crate::gst_test_media::{ROTATED_SIZE, rotated_h264_mp4};

#[test]
fn a_display_matrix_rotates_the_probe_and_every_decoded_frame() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    for rotation in Rotation::ALL {
        let mp4 = rotated_h264_mp4(dir.path(), rotation);
        let (w, h) = ROTATED_SIZE;
        let upright = if rotation.swaps_axes() {
            (h, w)
        } else {
            (w, h)
        };

        let probed = probe(&mp4).expect("the fixture probes");
        assert_eq!(
            (probed.width, probed.height),
            upright,
            "{rotation:?}: the probe reports the upright size"
        );

        let mut decoder = GstDecoder::open(&mp4, OutputMode::Cpu).expect("the fixture opens");
        assert_eq!(
            decoder.info().rotation,
            rotation,
            "{rotation:?}: source info"
        );
        assert_eq!(decoder.info().display_size_rotated(), upright);
        let mut frames = 0;
        while let Some(frame) = decoder.next_frame().expect("the fixture decodes") {
            assert_eq!(frame.rotation, rotation, "{rotation:?}: frame {frames}");
            assert_eq!(
                frame.coded_size, ROTATED_SIZE,
                "{rotation:?}: frame {frames}"
            );
            frames += 1;
        }
        assert_eq!(frames, 4, "{rotation:?}: every frame decodes");
    }
}
