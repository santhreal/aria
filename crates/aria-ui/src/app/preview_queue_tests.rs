//! The preview decoder runs one decode at a time and coalesces the rest.
//!
//! The class closed here is "a gesture that asks for a frame per mouse move
//! queues a decode per mouse move". Every decode takes the engine lock for its
//! whole length, so a queue of them is a queue on the lock the render path also
//! needs, and every frame in it except the last is discarded on arrival.
//!
//! `claim_preview` and `release_preview` are the whole decision, so it is
//! asserted here without a window: the async side does nothing but call them in
//! that order.

use super::premiere_tests::with_clip;

#[test]
fn a_second_request_while_decoding_does_not_start_a_second_decode() {
    let (mut state, _) = with_clip();

    assert!(state.claim_preview(), "an idle decoder starts the decode");
    assert!(
        !state.claim_preview(),
        "a request during a decode must not start a second one"
    );
    assert!(!state.claim_preview(), "nor the third");

    // Forty mouse moves collapse to one more decode, not forty.
    assert!(
        state.release_preview(),
        "the decode that landed owes the newest request a frame"
    );
    assert!(
        state.claim_preview(),
        "the decoder is free again once one lands"
    );
    assert!(
        !state.release_preview(),
        "nothing was asked for while that one ran"
    );
}

#[test]
fn a_request_that_arrives_idle_is_decoded_at_once() {
    let (mut state, _) = with_clip();

    for round in 0..3 {
        assert!(state.claim_preview(), "round {round} waited for nothing");
        assert!(
            !state.release_preview(),
            "round {round} invented a request nobody made"
        );
    }
}

#[test]
fn every_request_advances_the_generation_so_a_stale_frame_is_dropped() {
    let (mut state, _) = with_clip();
    let before = state.preview_generation;

    state.claim_preview();
    let started = state.preview_generation;
    assert!(started > before, "the decode took no generation");

    // The frame in flight is for `started`. Two more requests arrive while it
    // decodes; when it lands, the generation has moved past it, which is what
    // tells it not to put a stale picture on screen.
    state.claim_preview();
    state.claim_preview();
    assert!(
        state.preview_generation > started,
        "a coalesced request must still count, or the frame in flight would \
         commit a playhead that has moved on"
    );
    assert!(state.release_preview());
}
