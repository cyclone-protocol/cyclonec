//! End to end: the generated tree is compiled and run.
//!
//! A generator that emits plausible-looking source proves nothing. This
//! includes the committed `tests/fixtures/generated/` into a real crate and
//! runs it, so everything asserted below is code `rustc` accepted and bytes a
//! user would actually put on the wire.
//!
//! There is no stub and no import: the generated tree carries the runtime.
//! Every byte expectation is read off RFC-0002, and the decode-semantics tests
//! are §9.1 line by line.

// The generated runtime is `pub` and these tests do not call all of it.
#![allow(dead_code)]

// The models a user would write, and the tree `cyclonec` wrote for them.
// Mounted the way a user mounts them: the models the user annotated, and the
// tree generated from them. The generated codecs reach the models at
// `crate::models::player::Player` - the same types, not copies of them, at the
// path the fixture's own layout implies.
// Through a macro, for one reason: `cargo fmt` follows a `mod` declaration into
// the file it names, and reformatting the committed generated tree would leave
// it disagreeing with what `cyclonec` writes - a permanent, self-inflicted
// "stale" that no amount of regenerating fixes. rustfmt works on the AST before
// expansion, so it cannot see these; the compiler expands them into two
// ordinary modules and nothing else about the test changes.
macro_rules! mount_the_generated_tree {
    () => {
        #[path = "fixtures/src/models/mod.rs"]
        mod models;

        #[path = "fixtures/src/generated/mod.rs"]
        mod generated;
    };
}
mount_the_generated_tree!();

use generated::*;
use models::device_state::*;
use models::every_primitive::*;
use models::player::*;

fn encode<T>(value: &T, encode: fn(&mut Writer, &T)) -> Vec<u8> {
    let mut writer = Writer::new();
    encode(&mut writer, value);
    writer.into_bytes()
}

fn player() -> Player {
    Player {
        id: 100,
        x: 10.5,
        y: 20.0,
    }
}

// ==================================================== the bytes, against RFC-0002

/// The vector every SDK is checked against: Little Endian, no padding, no
/// metadata, fields in declaration order.
#[test]
fn a_model_is_its_fields_back_to_back_little_endian() {
    assert_eq!(
        encode(&player(), PlayerEdgeCodec::encode),
        [
            0x64, 0x00, 0x00, 0x00, // id = 100, u32 LE
            0x00, 0x00, 0x28, 0x41, // x = 10.5, raw IEEE 754 bits
            0x00, 0x00, 0xA0, 0x41, // y = 20.0
        ]
    );
}

/// One model, two codecs: each writes the fields that named it, and nothing
/// else - including the field that named neither.
#[test]
fn each_codec_writes_the_fields_that_named_it() {
    let state = DeviceState {
        id: 42,
        temperature: 21.5,
        display_name: "sensor-1".to_owned(),
        unrouted: 7,
        cache: "local".to_owned(),
    };

    assert_eq!(
        encode(&state, DeviceStateEdgeCodec::encode),
        [0x2A, 0x00, 0x00, 0x00, 0x00, 0x00, 0xAC, 0x41]
    );
    assert_eq!(
        encode(&state, DeviceStateUnityCodec::encode),
        [
            0x2A, 0x00, 0x00, 0x00, // id = 42
            0x08, 0x00, 0x00, 0x00, // "sensor-1", a length in bytes
            0x73, 0x65, 0x6E, 0x73, 0x6F, 0x72, 0x2D, 0x31,
        ]
    );
}

/// A `string` length counts **bytes**, not characters (RFC-0002 §3).
#[test]
fn a_string_length_counts_bytes() {
    let state = DeviceState {
        display_name: "héllo".to_owned(),
        ..DeviceState::default()
    };
    let bytes = encode(&state, DeviceStateUnityCodec::encode);

    assert_eq!(&bytes[4..8], [0x06, 0x00, 0x00, 0x00]);
    assert_eq!(bytes.len(), 4 + 4 + 6);
}

/// Float bits are written unmodified: `-0.0` keeps its sign and a `NaN`
/// payload survives (RFC-0002 §2.3).
#[test]
fn float_bit_patterns_survive() {
    let negative_zero = Player {
        id: 0,
        x: -0.0,
        y: 0.0,
    };
    let bytes = encode(&negative_zero, PlayerEdgeCodec::encode);
    assert_eq!(&bytes[4..8], [0x00, 0x00, 0x00, 0x80]);

    let mut round_tripped = Player::default();
    PlayerEdgeCodec::decode(&mut Reader::new(&bytes), &mut round_tripped).expect("decode");
    assert!(round_tripped.x.is_sign_negative());
}

/// Every primitive, in one message, at its RFC-0002 size.
#[test]
fn every_primitive_is_its_specified_width() {
    let value = EveryPrimitive {
        flag: true,
        tiny: -2,
        byte: 0xFF,
        small: -300,
        port: 8080,
        offset: -1,
        count: 4_294_967_295,
        delta: -2,
        sequence: 1,
        ratio: 1.0,
        precise: 2.0,
        label: "ok".to_owned(),
        blob: vec![0xDE, 0xAD],
    };

    assert_eq!(
        encode(&value, EveryPrimitiveEdgeCodec::encode),
        [
            0x01, // flag
            0xFE, // tiny = -2
            0xFF, // byte
            0xD4, 0xFE, // small = -300
            0x90, 0x1F, // port = 8080
            0xFF, 0xFF, 0xFF, 0xFF, // offset = -1
            0xFF, 0xFF, 0xFF, 0xFF, // count = u32::MAX
            0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // delta = -2
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // sequence = 1
            0x00, 0x00, 0x80, 0x3F, // ratio = 1.0
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, // precise = 2.0
            0x02, 0x00, 0x00, 0x00, 0x6F, 0x6B, // "ok"
            0x02, 0x00, 0x00, 0x00, 0xDE, 0xAD, // blob
        ]
    );

    let bytes = encode(&value, EveryPrimitiveEdgeCodec::encode);
    let mut back = EveryPrimitive::default();
    EveryPrimitiveEdgeCodec::decode(&mut Reader::new(&bytes), &mut back).expect("decode");
    assert_eq!(back, value);
}

/// A nested model is its fields inline - no header, no length, no tag.
#[test]
fn a_nested_model_is_inline() {
    let team = Team {
        captain: PlayerInfo { level: 9 },
        tags: vec!["a".to_owned()],
        scores: vec![7],
        roster: vec![PlayerInfo { level: 1 }, PlayerInfo { level: 2 }],
    };

    assert_eq!(
        encode(&team, TeamEdgeCodec::encode),
        [
            0x09, 0x00, 0x00, 0x00, // captain.level = 9
            0x01, 0x00, 0x00, 0x00, // tags: 1 element
            0x01, 0x00, 0x00, 0x00, 0x61, // "a"
            0x01, 0x00, 0x00, 0x00, // scores: 1 element
            0x07, 0x00, 0x00, 0x00, // 7
            0x02, 0x00, 0x00, 0x00, // roster: 2 elements
            0x01, 0x00, 0x00, 0x00, // roster[0].level
            0x02, 0x00, 0x00, 0x00, // roster[1].level
        ]
    );
}

#[test]
fn composites_round_trip() {
    let team = Team {
        captain: PlayerInfo { level: 9 },
        tags: vec!["a".to_owned(), "bb".to_owned()],
        scores: vec![1, 2, 3],
        roster: vec![PlayerInfo { level: 4 }],
    };

    let bytes = encode(&team, TeamEdgeCodec::encode);
    let mut back = Team::default();
    TeamEdgeCodec::decode(&mut Reader::new(&bytes), &mut back).expect("decode");
    assert_eq!(back, team);
}

/// A codec leaves the fields it does not carry exactly as they were, which is
/// what lets one model be split across several of them.
#[test]
fn decode_leaves_fields_the_codec_does_not_carry() {
    let bytes = encode(
        &DeviceState {
            id: 42,
            temperature: 21.5,
            ..DeviceState::default()
        },
        DeviceStateEdgeCodec::encode,
    );

    let mut value = DeviceState {
        display_name: "kept".to_owned(),
        unrouted: 7,
        cache: "kept".to_owned(),
        ..DeviceState::default()
    };
    DeviceStateEdgeCodec::decode(&mut Reader::new(&bytes), &mut value).expect("decode");

    assert_eq!(value.id, 42);
    assert_eq!(value.display_name, "kept");
    assert_eq!(value.unrouted, 7);
}

// ================================================ decode semantics - RFC-0002 §9.1

/// **Trailing bytes.** A new writer appended a field; an old reader reads what
/// it knows and ignores the rest. Not an error.
#[test]
fn trailing_bytes_are_ignored() {
    let mut bytes = encode(&player(), PlayerEdgeCodec::encode);
    bytes.extend_from_slice(&[0x2A, 0x00, 0x00, 0x00]); // a `level: u32` we never heard of

    let mut value = Player::default();
    let mut reader = Reader::new(&bytes);
    PlayerEdgeCodec::decode(&mut reader, &mut value).expect("trailing bytes are not an error");

    assert_eq!(value, player());
    assert_eq!(reader.remaining(), 4, "the extra field is left unread");
}

/// **A missing field.** An old writer stopped early; the reader's remaining
/// fields are absent and take their zero value.
#[test]
fn a_field_the_stream_ended_before_is_zero() {
    // Only `id` and `x` were written - `y` never arrived at all.
    let bytes = encode(&player(), PlayerEdgeCodec::encode)[..8].to_vec();

    let mut value = Player {
        id: 1,
        x: 1.0,
        y: 999.0,
    };
    PlayerEdgeCodec::decode(&mut Reader::new(&bytes), &mut value).expect("version skew is valid");

    assert_eq!(value.id, 100);
    assert_eq!(value.x, 10.5);
    assert_eq!(
        value.y, 0.0,
        "an absent field is zero, not what was there before"
    );
}

/// Every field absent - an empty payload is a valid message of zeroes.
#[test]
fn an_empty_payload_zeroes_every_field() {
    let mut value = player();
    PlayerEdgeCodec::decode(&mut Reader::new(&[]), &mut value).expect("all absent");
    assert_eq!(value, Player::default());
}

/// **A partial field.** The stream ended *inside* `y`, which is a truncated
/// packet, not version skew. It must not decode to zero.
#[test]
fn a_field_the_stream_ended_inside_is_an_error() {
    let bytes = encode(&player(), PlayerEdgeCodec::encode)[..10].to_vec(); // 2 of y's 4 bytes

    let mut value = Player::default();
    let error = PlayerEdgeCodec::decode(&mut Reader::new(&bytes), &mut value)
        .expect_err("a truncated field is corruption");

    assert_eq!(
        error,
        DecodeError::UnexpectedEof {
            needed: 4,
            remaining: 2
        }
    );
}

/// The distinction the whole of §9.1 turns on, stated as one test: the same
/// field, absent, and the same field, truncated.
#[test]
fn absent_and_truncated_are_not_the_same_thing() {
    let full = encode(&player(), PlayerEdgeCodec::encode);

    let mut absent = Player::default();
    assert!(PlayerEdgeCodec::decode(&mut Reader::new(&full[..8]), &mut absent).is_ok());

    let mut truncated = Player::default();
    assert!(PlayerEdgeCodec::decode(&mut Reader::new(&full[..9]), &mut truncated).is_err());
}

/// A nested model follows the same rule at its own level.
#[test]
fn a_nested_model_skews_at_its_own_level() {
    let mut value = Team {
        captain: PlayerInfo { level: 5 },
        tags: vec!["stale".to_owned()],
        scores: vec![1],
        roster: vec![PlayerInfo { level: 1 }],
    };

    // Only the captain arrived.
    TeamEdgeCodec::decode(&mut Reader::new(&[0x09, 0x00, 0x00, 0x00]), &mut value)
        .expect("version skew");

    assert_eq!(value.captain.level, 9);
    assert!(value.tags.is_empty(), "an absent array is empty, not stale");
    assert!(value.scores.is_empty());
    assert!(value.roster.is_empty());
}

/// An array element is **not** version skew: the count promised it.
#[test]
fn a_truncated_array_is_an_error_not_an_empty_tail() {
    let bytes = [
        0x00, 0x00, 0x00, 0x00, // captain.level
        0x02, 0x00, 0x00, 0x00, // tags: 2 elements promised
        0x01, 0x00, 0x00, 0x00, 0x61, // "a" - and then nothing
    ];

    let mut value = Team::default();
    assert!(TeamEdgeCodec::decode(&mut Reader::new(&bytes), &mut value).is_err());
}

// ==================================================== invalid streams - RFC-0002 §10

#[test]
fn an_invalid_bool_is_rejected() {
    let mut value = EveryPrimitive::default();
    let error = EveryPrimitiveEdgeCodec::decode(&mut Reader::new(&[0x02]), &mut value)
        .expect_err("0x02 is neither 0x00 nor 0x01");
    assert_eq!(error, DecodeError::InvalidBool(0x02));
}

#[test]
fn invalid_utf8_in_a_string_is_rejected() {
    let bytes = [
        0x2A, 0x00, 0x00, 0x00, // id
        0x01, 0x00, 0x00, 0x00, 0xFF, // a one-byte "string" that is not UTF-8
    ];
    let mut value = DeviceState::default();
    assert_eq!(
        DeviceStateUnityCodec::decode(&mut Reader::new(&bytes), &mut value),
        Err(DecodeError::InvalidUtf8)
    );
}

/// A length longer than the bytes that remain is invalid, whatever the limits
/// say (RFC-0002 §10.1) - and it is checked before anything is allocated.
#[test]
fn a_length_past_the_end_is_rejected_before_allocating() {
    let bytes = [
        0x2A, 0x00, 0x00, 0x00, // id
        0xFF, 0xFF, 0xFF, 0xFF, // a string of 4 GiB
    ];
    let mut value = DeviceState::default();
    assert!(DeviceStateUnityCodec::decode(&mut Reader::new(&bytes), &mut value).is_err());
}

/// Configured limits are additive, and rejecting on one is not an error the
/// wire format defines (RFC-0002 §12).
#[test]
fn a_configured_limit_rejects_a_length_the_stream_could_satisfy() {
    let bytes = encode(
        &DeviceState {
            display_name: "sensor-1".to_owned(),
            ..DeviceState::default()
        },
        DeviceStateUnityCodec::encode,
    );

    let limits = Limits {
        max_string_len: 4,
        ..Limits::UNLIMITED
    };
    let mut value = DeviceState::default();
    assert_eq!(
        DeviceStateUnityCodec::decode(&mut Reader::with_limits(&bytes, limits), &mut value),
        Err(DecodeError::LengthOverflow {
            length: 8,
            limit: 4
        })
    );
}

// ============================================================ the fingerprints

/// The constants on a codec and the constants in `handshake.rs` are the same
/// values, generated once from one schema.
#[test]
fn a_codec_and_the_handshake_agree() {
    assert_eq!(PlayerEdgeCodec::FINGERPRINT, PLAYER_EDGE_FINGERPRINT);
    assert_eq!(PlayerEdgeCodec::MESSAGE_ID, PLAYER_EDGE_MESSAGE_ID);
    assert_eq!(PlayerEdgeCodec::MESSAGE_NAME, "Player.edge");
    assert_eq!(TeamEdgeCodec::FINGERPRINT, TEAM_EDGE_FINGERPRINT);
}

/// Two codecs of one model are two wire contracts, and two fingerprints.
#[test]
fn each_codec_has_its_own_fingerprint() {
    assert_ne!(
        DEVICE_STATE_EDGE_FINGERPRINT,
        DEVICE_STATE_UNITY_FINGERPRINT
    );
    assert_ne!(DEVICE_STATE_EDGE_MESSAGE_ID, DEVICE_STATE_UNITY_MESSAGE_ID);
}

#[test]
fn the_message_table_holds_every_message() {
    assert_eq!(CYCLONE_MESSAGES.len(), 8);
    for message in CYCLONE_MESSAGES {
        assert_eq!(cyclone_message(message.id), Some(message));
    }
    assert_eq!(cyclone_message(0), None);
}

// =============================================================== the handshake

#[test]
fn handshake_current() {
    let peer: Vec<(u32, u32, u64)> = CYCLONE_MESSAGES
        .iter()
        .map(|message| {
            (
                message.id,
                message.prefixes.len() as u32,
                message.fingerprint,
            )
        })
        .collect();

    assert_eq!(
        cyclone_handshake(CYCLONE_SCHEMA_FINGERPRINT, &peer),
        CycloneHandshake::Current
    );
}

#[test]
fn handshake_outdated() {
    let peer = [(PLAYER_EDGE_MESSAGE_ID, 3, PLAYER_EDGE_FINGERPRINT)];

    assert_eq!(
        cyclone_handshake(0xDEAD_BEEF_0000_0000, &peer),
        CycloneHandshake::Outdated
    );
}

#[test]
fn handshake_breaking_is_rejected() {
    let peer = [(PLAYER_EDGE_MESSAGE_ID, 3, PLAYER_EDGE_FINGERPRINT ^ 1)];

    assert_eq!(
        cyclone_handshake(0xDEAD_BEEF_0000_0000, &peer),
        CycloneHandshake::Reject
    );
}

#[test]
fn a_message_only_one_side_knows_does_not_reject() {
    let peer = [
        (PLAYER_EDGE_MESSAGE_ID, 3, PLAYER_EDGE_FINGERPRINT),
        (0x1234_5678, 2, 0xAAAA_BBBB_CCCC_DDDD),
    ];

    assert_eq!(
        cyclone_handshake(0xDEAD_BEEF_0000_0000, &peer),
        CycloneHandshake::Outdated
    );
}

#[test]
fn a_peer_with_fewer_fields_is_accepted_when_the_prefix_agrees() {
    let peer = [(PLAYER_EDGE_MESSAGE_ID, 2, PLAYER_EDGE_PREFIXES[1])];

    assert_eq!(
        cyclone_message_check(PLAYER_EDGE_MESSAGE_ID, 2, PLAYER_EDGE_PREFIXES[1]),
        CycloneMessageCheck::Match
    );
    assert_eq!(
        cyclone_handshake(0xDEAD_BEEF_0000_0000, &peer),
        CycloneHandshake::Outdated
    );
}

#[test]
fn a_peer_with_fewer_fields_is_rejected_when_the_prefix_disagrees() {
    let peer = [(PLAYER_EDGE_MESSAGE_ID, 2, PLAYER_EDGE_PREFIXES[1] ^ 1)];

    assert_eq!(
        cyclone_handshake(0xDEAD_BEEF_0000_0000, &peer),
        CycloneHandshake::Reject
    );
}

#[test]
fn a_peer_with_more_fields_needs_one_more_exchange() {
    let peer = [(PLAYER_EDGE_MESSAGE_ID, 5, 0xAAAA_BBBB_CCCC_DDDD)];

    assert_eq!(
        cyclone_message_check(PLAYER_EDGE_MESSAGE_ID, 5, 0xAAAA_BBBB_CCCC_DDDD),
        CycloneMessageCheck::NeedPrefix(3)
    );
    assert_eq!(
        cyclone_handshake(0xDEAD_BEEF_0000_0000, &peer),
        CycloneHandshake::NeedMore
    );
}

#[test]
fn a_prefix_is_readable_by_field_count() {
    assert_eq!(
        cyclone_prefix(PLAYER_EDGE_MESSAGE_ID, 1),
        Some(PLAYER_EDGE_PREFIXES[0])
    );
    assert_eq!(
        cyclone_prefix(PLAYER_EDGE_MESSAGE_ID, 3),
        Some(PLAYER_EDGE_FINGERPRINT)
    );
    assert_eq!(cyclone_prefix(PLAYER_EDGE_MESSAGE_ID, 0), None);
    assert_eq!(cyclone_prefix(PLAYER_EDGE_MESSAGE_ID, 4), None);
    assert_eq!(cyclone_prefix(0x1234_5678, 1), None);
}

#[test]
fn the_last_prefix_is_the_message_fingerprint() {
    for message in CYCLONE_MESSAGES {
        assert_eq!(
            message.prefixes.last(),
            Some(&message.fingerprint),
            "{}",
            message.name
        );
    }
}

/// The envelope is off by default: no frame carries a fingerprint, and the
/// payload starts at byte zero.
#[test]
fn per_frame_validation_is_off_by_default() {
    const { assert!(!CYCLONE_VALIDATE_MESSAGE_FINGERPRINT) };
    assert_eq!(encode(&player(), PlayerEdgeCodec::encode).len(), 12);
}
