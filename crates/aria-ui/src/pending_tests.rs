//! Tests for [`super`]: the identity of a row's controls.
//!
//! The class closed here is two live surfaces drawing one job. The jobs board
//! opens over the panel that started the work, so both are mounted at once, and
//! GPUI keys hover and press state by element id: one id shared between them is
//! a pointer over the board lighting a row underneath it, and a press landing on
//! whichever was built first. Nothing about that is visible in a screenshot of
//! either surface on its own.

use super::*;

/// Every surface that can be mounted beside another, and the ids they hand out
/// for the same job. Named here rather than derived, because the point of the
/// test is that the callers pass different ones.
const SURFACES: [&str; 3] = ["panel", "board", "export"];

#[test]
fn one_job_gets_a_different_stop_control_per_surface() {
    let ids: Vec<_> = SURFACES.iter().map(|surface| stop_id(surface, 7)).collect();
    for (i, left) in ids.iter().enumerate() {
        for right in &ids[i + 1..] {
            assert_ne!(left, right, "two surfaces share one stop control");
        }
    }
}

#[test]
fn two_jobs_on_one_surface_get_different_stop_controls() {
    assert_ne!(stop_id("panel", 1), stop_id("panel", 2));
    assert_eq!(stop_id("panel", 1), stop_id("panel", 1));
}
