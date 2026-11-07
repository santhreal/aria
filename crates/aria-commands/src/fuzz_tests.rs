use super::*;

#[test]
fn every_fuzzed_edit_command_covers_every_spec_and_is_deterministic() {
    let project = Project::new("Fuzz", Rational::FPS_24);
    let mut rng_a = CommandRng::new(42);
    let mut rng_b = CommandRng::new(42);
    let list_a = every_fuzzed_edit_command(&mut rng_a, &project);
    let list_b = every_fuzzed_edit_command(&mut rng_b, &project);
    assert_eq!(list_a.len(), edit_specs().len());
    assert_eq!(format!("{list_a:?}"), format!("{list_b:?}"));
    for (cmd, spec) in list_a.iter().zip(edit_specs()) {
        assert_eq!(cmd.spec().id, spec.id);
    }
}

/// Closes the class where a seed's draws depend on the values of the ids it
/// draws. The allocators are process-wide, so those values, and the order a
/// hash map keyed by them hands them out in, follow whatever the tests beside
/// this one allocated and read first: the payload census walked a different
/// project from one run to the next and reached different variants. Every id
/// draw passes through `live_or_absent`: a live id is drawn by its rank, so
/// the order the live ids arrive in changes nothing, and an absent id is 0,
/// below every id an allocator hands out, or one no file or command states.
/// Does not catch a draw that reads some other value off an entity.
#[test]
fn an_id_draw_reads_the_ids_rank_and_an_absent_id_is_never_held() {
    let live: Vec<u64> = (0..12).map(|i| 1_000 + 37 * i).collect();
    let mut reversed = live.clone();
    reversed.reverse();
    let mut rotated = live.clone();
    rotated.rotate_left(5);
    let mut absent = 0;
    for seed in 0..64 {
        let draws = |order: &[u64]| -> Vec<ClipId> {
            let mut rng = CommandRng::new(seed);
            let raw: Vec<ClipId> = order.iter().map(|&r| ClipId::from_raw(r)).collect();
            (0..32)
                .map(|_| rng.live_or_absent(raw.clone(), ClipId::from_raw))
                .collect()
        };
        let drawn = draws(&live);
        assert_eq!(draws(&reversed), drawn, "seed {seed}, live ids reversed");
        assert_eq!(draws(&rotated), drawn, "seed {seed}, live ids rotated");
        for id in drawn.into_iter().filter(|id| !live.contains(&id.raw())) {
            absent += 1;
            assert!(
                id.raw() == 0 || serde_json::from_value::<ClipId>(id.raw().into()).is_err(),
                "seed {seed} drew {id:?}, an id an allocator hands out or a file states"
            );
        }
    }
    assert!(absent > 0, "no seed drew an absent id");
}
