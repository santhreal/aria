//! The block a processor receives: parameter signals in both forms, and the
//! lengths a block built by a caller must agree on.
//!
//! Registration is process-wide, so it is proved in `tests/audio_inserts.rs`,
//! its own binary, never here.

use super::*;

#[test]
fn a_signal_reads_per_frame_and_holds_its_last_value_past_the_end() {
    let ramp = [0.0, 0.5, 1.0];
    let signal = ParamSignal::PerFrame(&ramp);
    let read: Vec<f32> = (0..5).map(|frame| signal.at(frame)).collect();
    assert_eq!(read, [0.0, 0.5, 1.0, 1.0, 1.0]);
    assert_eq!(signal.constant(), None);
    assert_eq!(ParamSignal::PerFrame(&[]).at(0), 0.0);

    let level = ParamSignal::Constant(0.25);
    assert_eq!(level.at(0), 0.25);
    assert_eq!(level.at(10_000), 0.25);
    assert_eq!(level.constant(), Some(0.25));
}

#[test]
fn a_caller_built_block_hands_back_its_channels_signals_and_key() {
    let mut left = [1.0, 2.0];
    let mut right = [3.0, 4.0];
    let ramp = [0.0, 1.0];
    let params = [ParamSignal::Constant(0.5), ParamSignal::PerFrame(&ramp)];
    let key = ([0.1, 0.2], [0.3, 0.4]);
    let mut block = AudioBlock::new(
        &mut left,
        &mut right,
        96,
        48_000,
        &params,
        Some((&key.0[..], &key.1[..])),
    );
    assert_eq!(block.frames(), 2);
    assert_eq!(block.position(), 96);
    assert_eq!(block.sample_rate(), 48_000);
    assert_eq!(block.param(0), Some(ParamSignal::Constant(0.5)));
    assert_eq!(block.param(1), Some(ParamSignal::PerFrame(&ramp)));
    assert_eq!(block.param(2), None);
    assert_eq!(block.sidechain(), Some((&key.0[..], &key.1[..])));
    let (l, r) = block.channels();
    l[0] = -1.0;
    r[1] = -4.0;
    assert_eq!((left, right), ([-1.0, 2.0], [3.0, -4.0]));
}

/// The mixer's table: a constant slot reads as a constant, an automated slot
/// as its own run of `frames` values, never a neighbour's.
#[test]
fn the_mixer_table_reads_each_slot_from_its_own_run() {
    let mut left = [0.0; 3];
    let mut right = [0.0; 3];
    let constants = [None, Some(0.75), None];
    let automated = [1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 7.0, 8.0, 9.0];
    let block = AudioBlock::from_table(
        &mut left, &mut right, 0, 48_000, &constants, &automated, None,
    );
    assert_eq!(
        block.param(0),
        Some(ParamSignal::PerFrame(&[1.0, 2.0, 3.0]))
    );
    assert_eq!(block.param(1), Some(ParamSignal::Constant(0.75)));
    assert_eq!(
        block.param(2),
        Some(ParamSignal::PerFrame(&[7.0, 8.0, 9.0]))
    );
    assert_eq!(block.param(3), None);
    assert_eq!(block.sidechain(), None);
}

#[test]
#[should_panic(expected = "planar channels differ in length")]
fn channels_of_different_lengths_are_refused() {
    let mut left = [0.0; 2];
    let mut right = [0.0; 3];
    let _ = AudioBlock::new(&mut left, &mut right, 0, 48_000, &[], None);
}

#[test]
#[should_panic(expected = "a per-frame parameter covers the block")]
fn a_short_per_frame_parameter_is_refused() {
    let mut left = [0.0; 2];
    let mut right = [0.0; 2];
    let params = [ParamSignal::PerFrame(&[1.0])];
    let _ = AudioBlock::new(&mut left, &mut right, 0, 48_000, &params, None);
}

#[test]
#[should_panic(expected = "a sidechain key is as long as the block")]
fn a_short_key_is_refused() {
    let mut left = [0.0; 2];
    let mut right = [0.0; 2];
    let key = [0.0; 1];
    let _ = AudioBlock::new(
        &mut left,
        &mut right,
        0,
        48_000,
        &[],
        Some((&key[..], &key[..])),
    );
}

#[test]
fn a_panic_payload_reads_as_its_text() {
    let text = std::panic::catch_unwind(|| panic!("static text")).unwrap_err();
    assert_eq!(panic_message(text.as_ref()), "static text");
    let formatted = std::panic::catch_unwind(|| panic!("formatted {}", 7)).unwrap_err();
    assert_eq!(panic_message(formatted.as_ref()), "formatted 7");
    let other = std::panic::catch_unwind(|| std::panic::panic_any(7_u8)).unwrap_err();
    assert_eq!(panic_message(other.as_ref()), "a panic with no message");
}
