//! Three- and four-point editing: where a Source tab's edit lands and how much
//! of the source it takes.
//!
//! A Source tab holds In and Out on the source; the sequence holds In and Out
//! on the timeline. Any three of the four points define the edit and the
//! fourth comes from the other side's length. With all four set and the two
//! lengths different, the source is retimed to fill the sequence span.

/// The four points of an edit, each in its own clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Points {
    /// Source In and Out, in source ticks; `None` where the source is unmarked.
    pub source_in: Option<i64>,
    pub source_out: Option<i64>,
    /// The source's length, in source ticks.
    pub source_len: i64,
    /// Whether the source ends at `source_len`. A still repeats one frame for
    /// any length, so it has no end to run out of.
    pub bounded: bool,
    /// Sequence In and Out, in timeline ticks.
    pub sequence_in: Option<i64>,
    pub sequence_out: Option<i64>,
    /// Where an edit with no sequence point lands.
    pub playhead: i64,
}

/// What the points resolve to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Placement {
    /// The source range taken, `[start, end)` in source ticks.
    pub source: (i64, i64),
    /// The timeline tick the edit lands at.
    pub at: i64,
    /// The span the source is retimed to fill, in timeline ticks. `None`
    /// places the source at its own length.
    pub fill: Option<i64>,
}

/// Resolve `points` to a placement. `to_timeline` and `to_source` convert a
/// length between the two clocks. The error is the status line that explains
/// why nothing was placed.
pub(crate) fn resolve(
    points: Points,
    to_timeline: impl Fn(i64) -> i64,
    to_source: impl Fn(i64) -> i64,
) -> Result<Placement, &'static str> {
    let source_in = points.source_in.unwrap_or(0);
    let source_out = points.source_out.unwrap_or(points.source_len);
    let marked = match (points.sequence_in, points.sequence_out) {
        (Some(from), Some(to)) if to > from => Some((from, to)),
        _ => None,
    };
    let placement = match (marked, points.sequence_out) {
        // Sequence In and Out: the sequence decides the length.
        (Some((from, to)), _) => {
            let span = to - from;
            match (points.source_in, points.source_out) {
                (Some(start), Some(end)) => {
                    if end <= start {
                        return Err("The source's marked range is empty");
                    }
                    if !points.bounded {
                        // A still has no motion to retime; it takes the span.
                        return Ok(Placement {
                            source: (start, start + to_source(span).max(1)),
                            at: from,
                            fill: None,
                        });
                    }
                    let fill = (to_timeline(end - start).max(1) != span).then_some(span);
                    Placement {
                        source: (start, end),
                        at: from,
                        fill,
                    }
                }
                (None, Some(end)) => {
                    let start = end - to_source(span).max(1);
                    if start < 0 {
                        return Err(
                            "The source has less than the marked sequence span before its Out",
                        );
                    }
                    Placement {
                        source: (start, end),
                        at: from,
                        fill: None,
                    }
                }
                (_, None) => {
                    let end = source_in + to_source(span).max(1);
                    if points.bounded && end > points.source_len {
                        return Err(
                            "The source has less than the marked sequence span after its In",
                        );
                    }
                    Placement {
                        source: (source_in, end),
                        at: from,
                        fill: None,
                    }
                }
            }
        }
        // Sequence Out alone: the source is backtimed to end there.
        (None, Some(to)) if points.sequence_in.is_none() => {
            if source_out <= source_in {
                return Err("The source's marked range is empty");
            }
            let at = to - to_timeline(source_out - source_in).max(1);
            if at < 0 {
                return Err("The source is longer than the sequence before its Out");
            }
            Placement {
                source: (source_in, source_out),
                at,
                fill: None,
            }
        }
        // Sequence In, or no sequence point: the source lands at its own length.
        _ => {
            if source_out <= source_in {
                return Err("The source's marked range is empty");
            }
            Placement {
                source: (source_in, source_out),
                at: points.sequence_in.unwrap_or(points.playhead).max(0),
                fill: None,
            }
        }
    };
    Ok(placement)
}

#[cfg(test)]
mod tests {
    //! Every combination of the four points, on one source whose clock runs
    //! twice as fast as the sequence's, so a resolver that forgot to convert a
    //! length between the two clocks lands on the wrong tick.
    //!
    //! Not covered: the rate conversion itself, which is `resample`'s.

    use super::*;

    /// Two source ticks per timeline tick.
    fn resolve2(points: Points) -> Result<Placement, &'static str> {
        resolve(points, |source| source / 2, |timeline| timeline * 2)
    }

    fn points() -> Points {
        Points {
            source_in: None,
            source_out: None,
            source_len: 200,
            bounded: true,
            sequence_in: None,
            sequence_out: None,
            playhead: 30,
        }
    }

    #[test]
    fn every_combination_of_points_resolves_to_its_edit() {
        let cases: [(&str, [Option<i64>; 4], Placement); 9] = [
            (
                "nothing marked: the whole source at the playhead",
                [None, None, None, None],
                Placement {
                    source: (0, 200),
                    at: 30,
                    fill: None,
                },
            ),
            (
                "source In and Out at the playhead",
                [Some(20), Some(60), None, None],
                Placement {
                    source: (20, 60),
                    at: 30,
                    fill: None,
                },
            ),
            (
                "source range at the sequence In",
                [Some(20), Some(60), Some(5), None],
                Placement {
                    source: (20, 60),
                    at: 5,
                    fill: None,
                },
            ),
            (
                "source range backtimed to the sequence Out",
                [Some(20), Some(60), None, Some(50)],
                Placement {
                    source: (20, 60),
                    at: 30,
                    fill: None,
                },
            ),
            (
                "source In and the sequence span: the span decides the Out",
                [Some(20), None, Some(5), Some(15)],
                Placement {
                    source: (20, 40),
                    at: 5,
                    fill: None,
                },
            ),
            (
                "source Out and the sequence span: the span decides the In",
                [None, Some(60), Some(5), Some(15)],
                Placement {
                    source: (40, 60),
                    at: 5,
                    fill: None,
                },
            ),
            (
                "no source mark and the sequence span: from the source's start",
                [None, None, Some(5), Some(15)],
                Placement {
                    source: (0, 20),
                    at: 5,
                    fill: None,
                },
            ),
            (
                "four points of one length: no retime",
                [Some(20), Some(40), Some(5), Some(15)],
                Placement {
                    source: (20, 40),
                    at: 5,
                    fill: None,
                },
            ),
            (
                "four points of different lengths: fit to fill",
                [Some(20), Some(60), Some(5), Some(15)],
                Placement {
                    source: (20, 60),
                    at: 5,
                    fill: Some(10),
                },
            ),
        ];
        for (case, [source_in, source_out, sequence_in, sequence_out], expected) in cases {
            let resolved = resolve2(Points {
                source_in,
                source_out,
                sequence_in,
                sequence_out,
                ..points()
            });
            assert_eq!(resolved, Ok(expected), "{case}");
        }
    }

    #[test]
    fn a_source_too_short_for_the_span_is_refused() {
        let short_after_in = Points {
            source_in: Some(190),
            sequence_in: Some(0),
            sequence_out: Some(10),
            ..points()
        };
        assert!(resolve2(short_after_in).is_err());
        let still = Points {
            bounded: false,
            ..short_after_in
        };
        assert_eq!(
            resolve2(still).map(|placed| placed.source),
            Ok((190, 210)),
            "a still has no end to run out of"
        );

        let short_before_out = Points {
            source_out: Some(10),
            sequence_in: Some(0),
            sequence_out: Some(10),
            ..points()
        };
        assert!(resolve2(short_before_out).is_err());

        let backtimed_past_zero = Points {
            source_in: Some(0),
            source_out: Some(100),
            sequence_out: Some(10),
            ..points()
        };
        assert!(resolve2(backtimed_past_zero).is_err());
    }

    #[test]
    fn a_still_takes_a_four_point_span_instead_of_being_retimed() {
        let resolved = resolve2(Points {
            source_in: Some(20),
            source_out: Some(60),
            sequence_in: Some(5),
            sequence_out: Some(15),
            bounded: false,
            ..points()
        });
        assert_eq!(
            resolved,
            Ok(Placement {
                source: (20, 40),
                at: 5,
                fill: None
            })
        );
    }

    #[test]
    fn an_empty_or_inverted_mark_is_refused_not_placed() {
        for (source_in, source_out, sequence) in [
            (Some(60), Some(60), (None, None)),
            (Some(60), Some(20), (None, None)),
            (Some(60), Some(20), (Some(5), Some(15))),
            (Some(60), Some(20), (None, Some(50))),
        ] {
            let resolved = resolve2(Points {
                source_in,
                source_out,
                sequence_in: sequence.0,
                sequence_out: sequence.1,
                ..points()
            });
            assert!(
                resolved.is_err(),
                "{source_in:?}..{source_out:?} over {sequence:?}"
            );
        }
    }

    #[test]
    fn a_sequence_out_before_its_in_is_read_as_the_in_alone() {
        let resolved = resolve2(Points {
            source_in: Some(20),
            source_out: Some(60),
            sequence_in: Some(15),
            sequence_out: Some(5),
            ..points()
        });
        assert_eq!(
            resolved,
            Ok(Placement {
                source: (20, 60),
                at: 15,
                fill: None
            })
        );
    }
}
