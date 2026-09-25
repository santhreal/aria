//! Owned, normalized transcription output.

/// A complete transcription.
///
/// Segment text is joined with exactly one ASCII space between non-empty
/// segments. Leading, trailing, and repeated whitespace from Whisper is
/// normalized deterministically.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    text: String,
    segments: Vec<TranscriptSegment>,
}

impl Transcript {
    /// Returns the normalized text for the complete recording.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the normalized, time-ordered segments.
    #[must_use]
    pub fn segments(&self) -> &[TranscriptSegment] {
        &self.segments
    }

    /// Returns whether the transcription contains no spoken text.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Consumes the transcript and returns its text and segments.
    #[must_use]
    pub fn into_parts(self) -> (String, Vec<TranscriptSegment>) {
        (self.text, self.segments)
    }

    #[cfg(test)]
    pub(crate) fn test_fixture(segments: Vec<TranscriptSegment>) -> Self {
        let text = join_segment_text(&segments);
        Self { text, segments }
    }
}

/// One time-ordered transcription segment.
///
/// Timestamps are explicit centiseconds, the native unit returned by
/// `whisper.cpp`. They are always non-negative and `end_centiseconds()` is
/// always greater than or equal to `start_centiseconds()`.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptSegment {
    start_cs: u64,
    end_cs: u64,
    text: String,
    words: Vec<TranscriptWord>,
}

impl TranscriptSegment {
    /// Returns the segment start in centiseconds.
    #[must_use]
    pub fn start_centiseconds(&self) -> u64 {
        self.start_cs
    }

    /// Returns the segment end in centiseconds.
    #[must_use]
    pub fn end_centiseconds(&self) -> u64 {
        self.end_cs
    }

    /// Returns the segment start in finite seconds.
    #[must_use]
    pub fn start_seconds(&self) -> f64 {
        centiseconds_to_seconds(self.start_cs)
    }

    /// Returns the segment end in finite seconds.
    #[must_use]
    pub fn end_seconds(&self) -> f64 {
        centiseconds_to_seconds(self.end_cs)
    }

    /// Returns the normalized segment text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns normalized words when token timestamps were requested.
    ///
    /// The slice is empty when token timestamps were disabled or Whisper did
    /// not provide usable token timestamps.
    #[must_use]
    pub fn words(&self) -> &[TranscriptWord] {
        &self.words
    }

    /// Returns the duration-independent arithmetic mean of this segment's word
    /// probabilities, or `None` when the segment carries no words.
    ///
    /// One owner for "how sure is the model about this segment". The moments
    /// adapter converts the answer into a `MomentConfidence`, and a caption
    /// readout shows it beside the text; both used to be free to average
    /// differently, and a readout that disagreed with the stored confidence is
    /// a defect nobody can see from either side alone.
    ///
    /// Summed in `f64` so a long segment does not accumulate `f32` rounding,
    /// then narrowed once. `None` rather than a zero, because "no words" and
    /// "the model was certain of nothing" are different answers and only the
    /// caller knows which fallback its surface wants.
    #[must_use]
    pub fn mean_word_probability(&self) -> Option<f32> {
        if self.words.is_empty() {
            return None;
        }
        let sum: f64 = self
            .words
            .iter()
            .map(|word| f64::from(word.probability))
            .sum();
        Some((sum / self.words.len() as f64) as f32)
    }

    /// Consumes the segment and returns its timestamps, text, and words.
    #[must_use]
    pub fn into_parts(self) -> (u64, u64, String, Vec<TranscriptWord>) {
        (self.start_cs, self.end_cs, self.text, self.words)
    }

    #[cfg(test)]
    pub(crate) fn test_fixture(
        start_cs: u64,
        end_cs: u64,
        text: impl Into<String>,
        words: Vec<TranscriptWord>,
    ) -> Self {
        Self {
            start_cs,
            end_cs,
            text: text.into(),
            words,
        }
    }
}

/// One word, or one merged group of Whisper subword tokens.
///
/// Word ranges are non-negative, ordered, and contained by their segment.
/// `probability()` is always finite and in the inclusive range `0.0..=1.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptWord {
    start_cs: u64,
    end_cs: u64,
    text: String,
    probability: f32,
}

impl TranscriptWord {
    /// Returns the word start in centiseconds.
    #[must_use]
    pub fn start_centiseconds(&self) -> u64 {
        self.start_cs
    }

    /// Returns the word end in centiseconds.
    #[must_use]
    pub fn end_centiseconds(&self) -> u64 {
        self.end_cs
    }

    /// Returns the word start in finite seconds.
    #[must_use]
    pub fn start_seconds(&self) -> f64 {
        centiseconds_to_seconds(self.start_cs)
    }

    /// Returns the word end in finite seconds.
    #[must_use]
    pub fn end_seconds(&self) -> f64 {
        centiseconds_to_seconds(self.end_cs)
    }

    /// Returns the merged word text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the mean probability of the merged subword tokens.
    #[must_use]
    pub fn probability(&self) -> f32 {
        self.probability
    }

    /// Consumes the word and returns its timestamps, text, and probability.
    #[must_use]
    pub fn into_parts(self) -> (u64, u64, String, f32) {
        (self.start_cs, self.end_cs, self.text, self.probability)
    }

    #[cfg(test)]
    pub(crate) fn test_fixture(
        start_cs: u64,
        end_cs: u64,
        text: impl Into<String>,
        probability: f32,
    ) -> Self {
        Self {
            start_cs,
            end_cs,
            text: text.into(),
            probability,
        }
    }
}

pub(super) struct RawSegment {
    pub(super) start_cs: i64,
    pub(super) end_cs: i64,
    pub(super) text: String,
    pub(super) tokens: Vec<RawToken>,
}

pub(super) struct RawToken {
    pub(super) text: String,
    pub(super) timestamps_cs: Option<(i64, i64)>,
    pub(super) probability: f32,
}

pub(super) fn normalize_transcript(raw_segments: Vec<RawSegment>) -> Transcript {
    let mut previous_segment_end = 0;
    let mut segments = Vec::with_capacity(raw_segments.len());

    for raw in raw_segments {
        let start_cs = nonnegative_centiseconds(raw.start_cs).max(previous_segment_end);
        let end_cs = nonnegative_centiseconds(raw.end_cs).max(start_cs);
        let words = normalize_words(raw.tokens, start_cs, end_cs);
        let mut text = normalize_segment_text(&raw.text);
        if text.is_empty() && !words.is_empty() {
            text = join_word_text(&words);
        }

        segments.push(TranscriptSegment {
            start_cs,
            end_cs,
            text,
            words,
        });
        previous_segment_end = end_cs;
    }

    let text = join_segment_text(&segments);
    Transcript { text, segments }
}

fn normalize_words(
    raw_tokens: Vec<RawToken>,
    segment_start: u64,
    segment_end: u64,
) -> Vec<TranscriptWord> {
    let mut accumulators = Vec::new();
    let mut current: Option<WordAccumulator> = None;

    for token in raw_tokens {
        let trimmed = token.text.trim();
        if trimmed.is_empty() || is_special_whisper_token(trimmed) {
            continue;
        }

        let Some((raw_start, raw_end)) = token.timestamps_cs else {
            continue;
        };
        let starts_word = token.text.chars().next().is_some_and(char::is_whitespace);
        let piece = if starts_word {
            token.text.trim_start()
        } else {
            token.text.as_str()
        };
        if piece.is_empty() {
            continue;
        }

        let start_cs = nonnegative_centiseconds(raw_start);
        let end_cs = nonnegative_centiseconds(raw_end).max(start_cs);
        if starts_word && let Some(word) = current.take() {
            accumulators.push(word);
        }

        if let Some(word) = current.as_mut() {
            word.push_piece(piece, start_cs, end_cs, token.probability);
        } else {
            current = Some(WordAccumulator::new(
                piece,
                start_cs,
                end_cs,
                token.probability,
            ));
        }
    }

    if let Some(word) = current {
        accumulators.push(word);
    }

    let mut words = Vec::with_capacity(accumulators.len());
    let mut previous_word_end = segment_start;
    for word in accumulators {
        let text = normalize_spacing(&word.text);
        if text.is_empty() {
            continue;
        }

        let start_cs = word
            .start_cs
            .clamp(segment_start, segment_end)
            .max(previous_word_end);
        let end_cs = word.end_cs.clamp(start_cs, segment_end);
        let probability = (word.probability_sum / f64::from(word.probability_count)) as f32;
        words.push(TranscriptWord {
            start_cs,
            end_cs,
            text,
            probability: normalize_probability(probability),
        });
        previous_word_end = end_cs;
    }

    words
}

struct WordAccumulator {
    start_cs: u64,
    end_cs: u64,
    text: String,
    probability_sum: f64,
    probability_count: u32,
}

impl WordAccumulator {
    fn new(text: &str, start_cs: u64, end_cs: u64, probability: f32) -> Self {
        Self {
            start_cs,
            end_cs,
            text: text.to_owned(),
            probability_sum: f64::from(normalize_probability(probability)),
            probability_count: 1,
        }
    }

    fn push_piece(&mut self, text: &str, start_cs: u64, end_cs: u64, probability: f32) {
        self.start_cs = self.start_cs.min(start_cs);
        self.end_cs = self.end_cs.max(end_cs);
        self.text.push_str(text);
        self.probability_sum += f64::from(normalize_probability(probability));
        self.probability_count += 1;
    }
}

fn normalize_probability(probability: f32) -> f32 {
    if probability.is_nan() {
        0.0
    } else {
        probability.clamp(0.0, 1.0)
    }
}

fn nonnegative_centiseconds(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn centiseconds_to_seconds(value: u64) -> f64 {
    value as f64 / 100.0
}

/// Joins `pieces` with single spaces, byte-identical to
/// `collect::<Vec<_>>().join(" ")` but without the throwaway `Vec` of
/// borrows that version allocated on every call. `capacity` is an upper
/// bound on the result, so the buffer is allocated exactly once.
fn join_with_spaces<'a>(pieces: impl Iterator<Item = &'a str>, capacity: usize) -> String {
    let mut out = String::with_capacity(capacity);
    for (index, piece) in pieces.enumerate() {
        if index > 0 {
            out.push(' ');
        }
        out.push_str(piece);
    }
    out
}

fn normalize_segment_text(text: &str) -> String {
    let pieces = text
        .split_whitespace()
        .filter(|piece| !is_special_whisper_token(piece));
    join_with_spaces(pieces, text.len())
}

fn normalize_spacing(text: &str) -> String {
    join_with_spaces(text.split_whitespace(), text.len())
}

fn join_segment_text(segments: &[TranscriptSegment]) -> String {
    let capacity = segments
        .iter()
        .map(|segment| segment.text().len() + 1)
        .sum();
    let pieces = segments
        .iter()
        .map(TranscriptSegment::text)
        .filter(|text| !text.is_empty());
    join_with_spaces(pieces, capacity)
}

fn join_word_text(words: &[TranscriptWord]) -> String {
    let capacity = words.iter().map(|word| word.text().len() + 1).sum();
    join_with_spaces(words.iter().map(TranscriptWord::text), capacity)
}

fn is_special_whisper_token(text: &str) -> bool {
    let text = text.trim();
    if text.starts_with("<|") && text.ends_with("|>") {
        return true;
    }

    let Some(inner) = text
        .strip_prefix('[')
        .and_then(|text| text.strip_suffix(']'))
    else {
        return false;
    };
    !inner.is_empty()
        && inner.chars().all(|character| {
            character.is_ascii_uppercase()
                || character.is_ascii_digit()
                || matches!(character, '_' | '-' | ':' | '.')
        })
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
