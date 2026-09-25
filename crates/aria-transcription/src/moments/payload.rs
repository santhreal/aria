//! Compact JSON payload encoding for transcript moments.

use aria_analysis::moments::{MAX_MOMENT_PAYLOAD_BYTES, MomentPayload, MomentPayloadError};

use super::errors::{
    TranscriptMomentConversionError, TranscriptMomentErrorField, TranscriptMomentErrorReason,
    TranscriptMomentRecordIndex,
};

pub(super) fn encode_payload(
    record_index: TranscriptMomentRecordIndex,
    text: &str,
) -> Result<MomentPayload, TranscriptMomentConversionError> {
    let initial_capacity = text.len().saturating_add(80).min(MAX_MOMENT_PAYLOAD_BYTES);
    let mut payload = String::with_capacity(initial_capacity);

    push_payload(&mut payload, "{\"v\":1,\"segment_index\":", record_index)?;
    let segment_index = match record_index {
        TranscriptMomentRecordIndex::Segment { segment_index }
        | TranscriptMomentRecordIndex::Word { segment_index, .. } => segment_index,
    };
    push_payload_index(&mut payload, segment_index, record_index)?;

    if let TranscriptMomentRecordIndex::Word { word_index, .. } = record_index {
        push_payload(&mut payload, ",\"word_index\":", record_index)?;
        push_payload_index(&mut payload, word_index, record_index)?;
    }

    push_payload(&mut payload, ",\"text\":\"", record_index)?;
    push_json_string_contents(&mut payload, text, record_index)?;
    push_payload(&mut payload, "\"}", record_index)?;

    MomentPayload::new(&payload).map_err(|error| {
        let reason = match error {
            MomentPayloadError::TooLong { .. } => TranscriptMomentErrorReason::PayloadExceedsLimit,
            MomentPayloadError::Empty | MomentPayloadError::DisallowedControl { .. } => {
                TranscriptMomentErrorReason::InvalidPayload
            }
        };
        TranscriptMomentConversionError::record(
            record_index,
            TranscriptMomentErrorField::Payload,
            reason,
        )
    })
}

fn push_json_string_contents(
    output: &mut String,
    text: &str,
    record_index: TranscriptMomentRecordIndex,
) -> Result<(), TranscriptMomentConversionError> {
    for character in text.chars() {
        match character {
            '"' => push_payload(output, "\\\"", record_index)?,
            '\\' => push_payload(output, "\\\\", record_index)?,
            character if character.is_control() => {
                push_json_unicode_escape(output, character, record_index)?;
            }
            character => push_payload_character(output, character, record_index)?,
        }
    }
    Ok(())
}

fn push_json_unicode_escape(
    output: &mut String,
    character: char,
    record_index: TranscriptMomentRecordIndex,
) -> Result<(), TranscriptMomentConversionError> {
    let scalar = u32::from(character);
    if let Ok(code_unit) = u16::try_from(scalar) {
        push_json_code_unit(output, code_unit, record_index)
    } else {
        let supplementary = scalar - 0x1_0000;
        let high = 0xd800 | u16::try_from(supplementary >> 10).unwrap_or(0);
        let low = 0xdc00 | u16::try_from(supplementary & 0x03ff).unwrap_or(0);
        push_json_code_unit(output, high, record_index)?;
        push_json_code_unit(output, low, record_index)
    }
}

fn push_json_code_unit(
    output: &mut String,
    code_unit: u16,
    record_index: TranscriptMomentRecordIndex,
) -> Result<(), TranscriptMomentConversionError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    push_payload(output, "\\u", record_index)?;
    for shift in [12, 8, 4, 0] {
        let nibble = usize::from((code_unit >> shift) & 0x000f);
        push_payload_character(output, char::from(HEX[nibble]), record_index)?;
    }
    Ok(())
}

fn push_payload(
    output: &mut String,
    value: &str,
    record_index: TranscriptMomentRecordIndex,
) -> Result<(), TranscriptMomentConversionError> {
    let encoded_len = output.len().checked_add(value.len());
    if encoded_len.is_none_or(|length| length > MAX_MOMENT_PAYLOAD_BYTES) {
        return Err(payload_too_large(record_index));
    }
    output.push_str(value);
    Ok(())
}

fn push_payload_index(
    output: &mut String,
    value: usize,
    record_index: TranscriptMomentRecordIndex,
) -> Result<(), TranscriptMomentConversionError> {
    let mut digits = [0_u8; 20];
    let mut start = digits.len();
    let mut remaining = value;
    loop {
        start -= 1;
        digits[start] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }
    let text = std::str::from_utf8(&digits[start..]).expect("decimal digits are ASCII");
    push_payload(output, text, record_index)
}

fn push_payload_character(
    output: &mut String,
    character: char,
    record_index: TranscriptMomentRecordIndex,
) -> Result<(), TranscriptMomentConversionError> {
    let encoded_len = output.len().checked_add(character.len_utf8());
    if encoded_len.is_none_or(|length| length > MAX_MOMENT_PAYLOAD_BYTES) {
        return Err(payload_too_large(record_index));
    }
    output.push(character);
    Ok(())
}

const fn payload_too_large(
    record_index: TranscriptMomentRecordIndex,
) -> TranscriptMomentConversionError {
    TranscriptMomentConversionError::record(
        record_index,
        TranscriptMomentErrorField::Payload,
        TranscriptMomentErrorReason::PayloadExceedsLimit,
    )
}
