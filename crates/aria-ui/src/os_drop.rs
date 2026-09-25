//! OS file-drop placement planning.

/// Start ticks that place clips of `durations` end-to-end from `anchor`,
/// sliding right past `spans` — the sorted, non-overlapping `[start, end)`
/// ranges already occupied on the landing lane.
pub fn plan_sequential_starts(spans: &[(i64, i64)], anchor: i64, durations: &[i64]) -> Vec<i64> {
    let mut starts = Vec::with_capacity(durations.len());
    let mut cursor = anchor.max(0);
    for &duration in durations {
        let duration = duration.max(1);
        let mut start = cursor;
        for &(occupied_start, occupied_end) in spans {
            if start < occupied_end && start + duration > occupied_start {
                start = occupied_end;
            }
        }
        starts.push(start);
        cursor = start + duration;
    }
    starts
}

/// Insertion boundary: the start of the first clip whose midpoint lies
/// right of `desired`, else the lane's content end.
pub fn insertion_boundary(spans: &[(i64, i64)], desired: i64) -> i64 {
    for &(start, end) in spans {
        let mid = start + (end - start) / 2;
        if desired < mid {
            return start;
        }
    }
    spans.last().map(|(_, end)| *end).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_lane_places_end_to_end_from_the_anchor() {
        assert_eq!(
            plan_sequential_starts(&[], 100, &[50, 30, 20]),
            vec![100, 150, 180]
        );
    }

    #[test]
    fn negative_anchor_clamps_to_zero() {
        assert_eq!(plan_sequential_starts(&[], -40, &[10, 10]), vec![0, 10]);
    }

    #[test]
    fn blocked_anchor_slides_the_whole_chain_right() {
        assert_eq!(
            plan_sequential_starts(&[(80, 200)], 100, &[50, 25]),
            vec![200, 250]
        );
    }

    #[test]
    fn chain_fits_a_gap_then_slides_past_the_next_clip() {
        assert_eq!(
            plan_sequential_starts(&[(100, 300)], 0, &[60, 60]),
            vec![0, 300]
        );
        assert_eq!(
            plan_sequential_starts(&[(100, 300)], 0, &[100, 60]),
            vec![0, 300]
        );
    }

    #[test]
    fn durations_are_clamped_to_at_least_one_tick() {
        assert_eq!(plan_sequential_starts(&[], 0, &[0, 0]), vec![0, 1]);
    }

    #[test]
    fn insertion_boundary_flips_at_clip_midpoints() {
        let spans = [(0, 100), (100, 250)];
        assert_eq!(insertion_boundary(&spans, 20), 0);
        assert_eq!(insertion_boundary(&spans, 80), 100);
        assert_eq!(insertion_boundary(&spans, 240), 250);
    }

    #[test]
    fn insertion_boundary_on_an_empty_lane_is_zero() {
        assert_eq!(insertion_boundary(&[], 500), 0);
    }
}
