use super::*;

#[test]
fn normalizes_tokens_segments_and_probabilities() {
    let transcript = normalize_transcript(vec![
        RawSegment {
            start_cs: -10,
            end_cs: 50,
            text: "  Hello   world!  ".into(),
            tokens: vec![
                token("<|startoftranscript|>", 0, 0, 1.0),
                token(" Hel", -5, 20, f32::NAN),
                token("lo", 10, 25, f32::INFINITY),
                token(" world", 15, 45, -2.0),
                token("!", 44, 60, 0.8),
                token("[EOT]", 50, 50, 1.0),
            ],
        },
        RawSegment {
            start_cs: 40,
            end_cs: 30,
            text: "  Again  ".into(),
            tokens: vec![token(" Again", 35, 35, 0.5)],
        },
    ]);

    assert_eq!(transcript.text(), "Hello world! Again");
    assert_eq!(transcript.segments().len(), 2);

    let first = &transcript.segments()[0];
    assert_eq!((first.start_cs, first.end_cs), (0, 50));
    assert_eq!(first.words.len(), 2);
    assert_eq!(first.words[0].text(), "Hello");
    assert_eq!(
        (
            first.words[0].start_centiseconds(),
            first.words[0].end_centiseconds()
        ),
        (0, 25)
    );
    assert_eq!(first.words[0].probability(), 0.5);
    assert_eq!(first.words[1].text(), "world!");
    assert_eq!(
        (
            first.words[1].start_centiseconds(),
            first.words[1].end_centiseconds()
        ),
        (25, 50)
    );
    assert!((first.words[1].probability() - 0.4).abs() < f32::EPSILON);

    let second = &transcript.segments()[1];
    assert_eq!((second.start_cs, second.end_cs), (50, 50));
    assert_eq!(
        (
            second.words[0].start_centiseconds(),
            second.words[0].end_centiseconds()
        ),
        (50, 50)
    );
    assert!(second.start_seconds().is_finite());
    assert!(second.end_seconds().is_finite());
}

#[test]
fn skips_missing_timestamps_and_uses_word_text_as_fallback() {
    let transcript = normalize_transcript(vec![RawSegment {
        start_cs: 10,
        end_cs: 20,
        text: " [BLANK_AUDIO] ".into(),
        tokens: vec![
            RawToken {
                text: " ignored".into(),
                timestamps_cs: None,
                probability: 1.0,
            },
            token(" useful", 10, 20, 0.9),
        ],
    }]);

    assert_eq!(transcript.text(), "useful");
    assert_eq!(transcript.segments()[0].text(), "useful");
    assert_eq!(transcript.segments()[0].words()[0].text(), "useful");
}

#[test]
fn does_not_treat_lowercase_bracketed_speech_as_special() {
    assert!(!is_special_whisper_token("[hello]"));
    assert!(is_special_whisper_token("[MUSIC]"));
    assert!(is_special_whisper_token("<|endoftext|>"));
}

fn token(text: &str, start_cs: i64, end_cs: i64, probability: f32) -> RawToken {
    RawToken {
        text: text.into(),
        timestamps_cs: Some((start_cs, end_cs)),
        probability,
    }
}
