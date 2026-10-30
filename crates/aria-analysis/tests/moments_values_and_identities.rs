//! `moments` tests covering values and identities. A `#[path]` child of the
//! `moments` test binary; the tests reach their fixtures through `super::*`.

use super::*;

#[test]
fn content_key_binary_and_hex_forms_round_trip_canonically() {
    let bytes = std::array::from_fn(|index| index as u8);
    let key = MediaContentKey::from_slice(&bytes).expect("32 bytes form a key");
    let expected = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    assert_eq!(key.as_bytes(), &bytes);
    assert_eq!(key.into_bytes(), bytes);
    assert_eq!(key.to_string(), expected);
    assert_eq!(key.encode_hex().as_slice(), expected.as_bytes());
    assert_eq!(expected.len(), MEDIA_CONTENT_KEY_HEX_BYTES);
    assert_eq!(MediaContentKey::from_str(expected), Ok(key));
    assert_eq!(
        MediaContentKey::from_str(&expected.to_ascii_uppercase()),
        Ok(key)
    );
}

#[test]
fn content_key_reports_exact_length_and_invalid_hex_errors() {
    assert_eq!(
        MediaContentKey::from_slice(&[0; MEDIA_CONTENT_KEY_BYTES - 1]),
        Err(MediaContentKeyError::InvalidByteLength {
            actual: MEDIA_CONTENT_KEY_BYTES - 1,
        })
    );
    assert_eq!(
        MediaContentKey::from_slice(&[0; MEDIA_CONTENT_KEY_BYTES + 1]),
        Err(MediaContentKeyError::InvalidByteLength {
            actual: MEDIA_CONTENT_KEY_BYTES + 1,
        })
    );
    assert_eq!(
        "0".repeat(MEDIA_CONTENT_KEY_HEX_BYTES - 1)
            .parse::<MediaContentKey>(),
        Err(MediaContentKeyError::InvalidHexLength {
            actual: MEDIA_CONTENT_KEY_HEX_BYTES - 1,
        })
    );
    assert_eq!(
        "0".repeat(MEDIA_CONTENT_KEY_HEX_BYTES + 1)
            .parse::<MediaContentKey>(),
        Err(MediaContentKeyError::InvalidHexLength {
            actual: MEDIA_CONTENT_KEY_HEX_BYTES + 1,
        })
    );

    let mut invalid = "0".repeat(MEDIA_CONTENT_KEY_HEX_BYTES);
    invalid.replace_range(17..18, "g");
    assert_eq!(
        invalid.parse::<MediaContentKey>(),
        Err(MediaContentKeyError::InvalidHexCharacter {
            index: 17,
            byte: b'g',
        })
    );
}

#[test]
fn built_in_and_custom_moment_kinds_are_stable_and_bounded() {
    let built_ins = [
        (MomentKind::SHOT_BOUNDARY, "shot_boundary"),
        (MomentKind::SILENCE, "silence"),
        (MomentKind::BEAT, "beat"),
        (MomentKind::TRANSCRIPT_SEGMENT, "transcript_segment"),
        (MomentKind::TRANSCRIPT_WORD, "transcript_word"),
        (MomentKind::VISION_MOMENT, "vision_moment"),
    ];

    for (kind, expected) in built_ins {
        assert_eq!(kind.as_str(), expected);
        assert!(kind.is_builtin());
        assert_eq!(MomentKind::new(expected), Ok(kind));
    }

    let custom = MomentKind::new("gameplay.kill-confirmed").expect("custom kind is valid");
    assert_eq!(custom.as_str(), "gameplay.kill-confirmed");
    assert!(!custom.is_builtin());

    let maximum = format!("a{}", "x".repeat(MAX_MOMENT_KIND_BYTES - 1));
    assert_eq!(
        MomentKind::new(&maximum)
            .expect("maximum kind length is valid")
            .as_str(),
        maximum
    );
    assert_eq!(MomentKind::new(""), Err(MomentKindError::Empty));
    assert_eq!(
        MomentKind::new(&format!("a{}", "x".repeat(MAX_MOMENT_KIND_BYTES))),
        Err(MomentKindError::TooLong {
            actual: MAX_MOMENT_KIND_BYTES + 1,
        })
    );
    assert_eq!(
        MomentKind::new("Shot"),
        Err(MomentKindError::InvalidStart { byte: b'S' })
    );
    assert_eq!(
        MomentKind::new("custom/kind"),
        Err(MomentKindError::InvalidCharacter {
            index: 6,
            byte: b'/',
        })
    );
}

#[test]
fn analyzer_identities_are_namespaced_bounded_ascii_values() {
    let identity =
        AnalyzerIdentity::new("aria/audio/silence-v2").expect("namespaced identity is valid");
    assert_eq!(identity.as_str(), "aria/audio/silence-v2");

    let maximum = format!("a{}", "x".repeat(MAX_ANALYZER_IDENTITY_BYTES - 1));
    assert_eq!(
        AnalyzerIdentity::new(&maximum)
            .expect("maximum identity length is valid")
            .as_str(),
        maximum
    );
    assert_eq!(AnalyzerIdentity::new(""), Err(AnalyzerIdentityError::Empty));
    assert_eq!(
        AnalyzerIdentity::new(&format!("a{}", "x".repeat(MAX_ANALYZER_IDENTITY_BYTES))),
        Err(AnalyzerIdentityError::TooLong {
            actual: MAX_ANALYZER_IDENTITY_BYTES + 1,
        })
    );
    assert_eq!(
        AnalyzerIdentity::new("Aria"),
        Err(AnalyzerIdentityError::InvalidStart { byte: b'A' })
    );
    assert_eq!(
        AnalyzerIdentity::new("aria:shots"),
        Err(AnalyzerIdentityError::InvalidCharacter {
            index: 4,
            byte: b':',
        })
    );
}

#[test]
fn payloads_are_bounded_text_without_opaque_parsing() {
    let text = "speaker: Héllo\tworld\n{\"opaque\":true}";
    let payload = MomentPayload::new(text).expect("ordinary UTF-8 text is valid");
    assert_eq!(payload.as_str(), text);
    assert_eq!(payload.len_bytes(), text.len());

    let maximum = "x".repeat(MAX_MOMENT_PAYLOAD_BYTES);
    assert_eq!(
        MomentPayload::new(&maximum)
            .expect("maximum payload length is valid")
            .len_bytes(),
        MAX_MOMENT_PAYLOAD_BYTES
    );
    assert_eq!(MomentPayload::new(""), Err(MomentPayloadError::Empty));
    assert_eq!(
        MomentPayload::new(&"x".repeat(MAX_MOMENT_PAYLOAD_BYTES + 1)),
        Err(MomentPayloadError::TooLong {
            actual: MAX_MOMENT_PAYLOAD_BYTES + 1,
        })
    );
    assert_eq!(
        MomentPayload::new("ok\0bad"),
        Err(MomentPayloadError::DisallowedControl {
            index: 2,
            character: '\0',
        })
    );
}

#[test]
fn confidence_rejects_nonfinite_and_out_of_range_values() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            MomentConfidence::new(value),
            Err(MomentConfidenceError::NonFinite)
        );
    }
    for value in [-0.001, 1.001] {
        assert_eq!(
            MomentConfidence::new(value),
            Err(MomentConfidenceError::OutOfRange)
        );
    }

    assert_eq!(MomentConfidence::new(0.0), Ok(MomentConfidence::ZERO));
    assert_eq!(MomentConfidence::new(1.0), Ok(MomentConfidence::CERTAIN));
    assert_eq!(
        MomentConfidence::new(-0.0)
            .expect("negative zero is in range")
            .get()
            .to_bits(),
        0.0_f32.to_bits()
    );
    assert_eq!(AnalyzerVersion::new(0), Err(AnalyzerVersionError::Zero));
    assert_eq!(
        AnalyzerVersion::new(u32::MAX).map(AnalyzerVersion::get),
        Ok(u32::MAX)
    );
}
