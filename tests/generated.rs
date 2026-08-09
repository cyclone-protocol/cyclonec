//! End-to-end: the generated file is compiled and run.
//!
//! A generator that emits plausible-looking source proves nothing. This includes
//! the committed `tests/fixtures/cyclone.codec.rs` into a real crate and runs
//! it, so everything asserted below is code `rustc` accepted.
//!
//! There is no stub and no import. The generated file carries the runtime -
//! `Writer`, `Reader`, `DecodeError`, `Limits` - so the bytes checked here are
//! the bytes a user would put on the wire, compared against RFC-0002.

// The generated file defines the runtime as `pub`, and these tests do not call
// every method of it.
#![allow(dead_code)]

// The models a user would write, and the one file `cyclonec` wrote for them.
include!("fixtures/device_state.rs");
include!("fixtures/cyclone.codec.rs");

fn sample() -> DeviceState {
    DeviceState {
        id: 42,
        temperature: 21.5,
        display_name: "sensor-1".to_owned(),
        unrouted: 7,
        cache: "local".to_owned(),
    }
}

fn encode<T>(value: &T, encode: fn(&mut Writer, &T)) -> Vec<u8> {
    let mut writer = Writer::new();
    encode(&mut writer, value);
    writer.into_bytes()
}

// ================================================== §15 - one model, two codecs

/// h.md §15 - `EdgeCodec` carries `id` and `temperature`, `UnityCodec` carries
/// `id` and `display_name`, each in declaration order. These are the bytes.
#[test]
fn each_codec_writes_the_fields_that_named_it() {
    assert_eq!(
        encode(&sample(), DeviceStateEdgeCodec::encode),
        [
            0x2A, 0x00, 0x00, 0x00, // id = 42, u32 Little Endian
            0x00, 0x00, 0xAC, 0x41, // temperature = 21.5, raw IEEE 754 bits
        ]
    );

    assert_eq!(
        encode(&sample(), DeviceStateUnityCodec::encode),
        [
            0x2A, 0x00, 0x00, 0x00, // id = 42
            0x08, 0x00, 0x00, 0x00, // "sensor-1" - a length in bytes
            0x73, 0x65, 0x6E, 0x73, 0x6F, 0x72, 0x2D, 0x31,
        ]
    );
}

/// Each codec round-trips the fields it carries.
#[test]
fn each_codec_round_trips() {
    let bytes = encode(&sample(), DeviceStateEdgeCodec::encode);
    let mut value = DeviceState::default();
    let mut reader = Reader::new(&bytes);

    DeviceStateEdgeCodec::decode(&mut reader, &mut value).expect("decode");

    assert_eq!(value.id, 42);
    assert_eq!(value.temperature, 21.5);
    assert!(reader.is_empty(), "the cursor lands exactly at the end");
}

/// A codec leaves the fields it does not carry exactly as they were, which is
/// what lets one model be split across several of them.
#[test]
fn decode_leaves_fields_it_does_not_carry_alone() {
    let bytes = encode(&sample(), DeviceStateEdgeCodec::encode);
    let mut value = DeviceState { id: 0, temperature: 0.0, ..sample() };

    DeviceStateEdgeCodec::decode(&mut Reader::new(&bytes), &mut value).expect("decode");

    // `edge` carries these two.
    assert_eq!(value.id, 42);
    assert_eq!(value.temperature, 21.5);

    // It carries none of these, so none of them moved.
    assert_eq!(value.display_name, "sensor-1");
    assert_eq!(value.unrouted, 7);
    assert_eq!(value.cache, "local");
}

/// Both codecs applied in turn rebuild every routed field, and only those.
#[test]
fn two_codecs_together_cover_the_routed_fields() {
    let edge = encode(&sample(), DeviceStateEdgeCodec::encode);
    let unity = encode(&sample(), DeviceStateUnityCodec::encode);

    let mut value = DeviceState::default();
    DeviceStateEdgeCodec::decode(&mut Reader::new(&edge), &mut value).expect("decode");
    DeviceStateUnityCodec::decode(&mut Reader::new(&unity), &mut value).expect("decode");

    assert_eq!(value.id, 42);
    assert_eq!(value.temperature, 21.5);
    assert_eq!(value.display_name, "sensor-1");

    // A field in no codec, and a field with no `#[network]`, are on no wire.
    assert_eq!(value.unrouted, 0);
    assert_eq!(value.cache, "");
}

// ========================================================== §16 - codec names

/// h.md §16 - every identifier is a codec name, and the four types exist.
#[test]
fn unknown_codec_names_become_generated_types() {
    let value = Telemetry { sequence: 9 };
    let expected = [0x09, 0, 0, 0, 0, 0, 0, 0];

    assert_eq!(encode(&value, TelemetryEdgeCodec::encode), expected);
    assert_eq!(encode(&value, TelemetryOrangePiCodec::encode), expected);
    assert_eq!(encode(&value, TelemetryUnityCodec::encode), expected);
    assert_eq!(encode(&value, TelemetryCustomACodec::encode), expected);
}

// ======================================================= §8 - composite model

/// h.md §8 - a model-typed field becomes a call to that model's codec, inlined:
/// no length, no delimiter, no header.
#[test]
fn a_model_field_is_inlined() {
    let value = Player { hp: 100, speed: 1.5, info: PlayerInfo { level: 3 } };

    let bytes = encode(&value, PlayerEdgeCodec::encode);
    assert_eq!(
        bytes,
        [
            0x64, 0x00, 0x00, 0x00, // hp = 100
            0x00, 0x00, 0xC0, 0x3F, // speed = 1.5
            0x03, 0x00, 0x00, 0x00, // info.level = 3, inlined
        ]
    );

    let mut value = Player::default();
    let mut reader = Reader::new(&bytes);
    PlayerEdgeCodec::decode(&mut reader, &mut value).expect("decode");

    assert_eq!(value.hp, 100);
    assert_eq!(value.info.level, 3);
    assert!(reader.is_empty());
}

// ========================================================== §6 - Array<T>

/// §6 - `Array<T>` is a `UInt32` count followed by that many elements, no
/// per-element length prefix. Covers a scalar, a string, and a nested model as
/// element types in one codec. This exact byte sequence is also asserted, from
/// their own generated code, in `tests/csharp/GeneratedTests.cs`,
/// `tests/fixtures/cyclone_generated_test.go`, and by a real Godot run over
/// `tests/fixtures/gdscript/team.gd` - four languages, one wire format.
fn team_golden_bytes() -> [u8; 48] {
    [
        0x03, 0x00, 0x00, 0x00, // scores.len() = 3
        0x0A, 0x00, 0x00, 0x00, // scores[0] = 10
        0x14, 0x00, 0x00, 0x00, // scores[1] = 20
        0x1E, 0x00, 0x00, 0x00, // scores[2] = 30
        0x02, 0x00, 0x00, 0x00, // names.len() = 2
        0x05, 0x00, 0x00, 0x00, b'a', b'l', b'i', b'c', b'e', // names[0]
        0x03, 0x00, 0x00, 0x00, b'b', b'o', b'b', // names[1]
        0x02, 0x00, 0x00, 0x00, // players.len() = 2
        0x03, 0x00, 0x00, 0x00, // players[0].level = 3, inlined
        0x07, 0x00, 0x00, 0x00, // players[1].level = 7, inlined
    ]
}

fn team_sample() -> Team {
    Team {
        scores: vec![10, 20, 30],
        names: vec!["alice".to_owned(), "bob".to_owned()],
        players: vec![PlayerInfo { level: 3 }, PlayerInfo { level: 7 }],
    }
}

#[test]
fn array_of_scalar_string_and_model_matches_the_golden_bytes() {
    assert_eq!(encode(&team_sample(), TeamEdgeCodec::encode), team_golden_bytes());
}

#[test]
fn array_round_trips_including_nested_model_elements() {
    let bytes = encode(&team_sample(), TeamEdgeCodec::encode);
    let mut value = Team::default();
    let mut reader = Reader::new(&bytes);

    TeamEdgeCodec::decode(&mut reader, &mut value).expect("decode");

    assert_eq!(value, team_sample());
    assert!(reader.is_empty(), "the cursor lands exactly at the end");
}

/// An empty `Array<T>` is just its `UInt32` count of zero - no elements, and
/// decoding it leaves the field an empty `Vec`, not untouched.
#[test]
fn an_empty_array_is_just_its_zero_count() {
    let bytes = encode(&Team::default(), TeamEdgeCodec::encode);
    assert_eq!(bytes, [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);

    let mut value = Team { scores: vec![1], ..Team::default() };
    TeamEdgeCodec::decode(&mut Reader::new(&bytes), &mut value).expect("decode");
    assert!(value.scores.is_empty());
}

/// `Limits::max_array_count` is checked before an element is allocated, the
/// same guard `max_string_length` and `max_bytes_length` already give scalar
/// fields - a forged huge count cannot force an unbounded allocation.
#[test]
fn an_array_count_over_the_limit_is_rejected_before_allocating() {
    let mut writer = Writer::new();
    writer.write_array_count(5);

    let limits = Limits { max_array_count: 2, ..Limits::UNLIMITED };
    let bytes = writer.into_bytes();
    let mut reader = Reader::with_limits(bytes.as_slice(), limits);

    assert!(reader.read_array_count().is_err());
}

// ============================================================ §4 - primitives

/// h.md §4 - each network type maps to the runtime method RFC-0002 defines, and
/// these are the bytes that method writes.
#[test]
fn every_primitive_matches_the_specification() {
    let value = EveryPrimitive {
        flag: true,
        a: -1,
        b: 255,
        c: -1,
        d: 300,
        e: -1,
        f: 0x1234_5678,
        g: -1,
        h: 1,
        i: 1.5,
        j: 1.0,
        k: "中".to_owned(),
        l: vec![0xFF, 0xFE],
    };

    assert_eq!(
        encode(&value, EveryPrimitiveAllCodec::encode),
        [
            0x01, // bool
            0xFF, // i8 -1
            0xFF, // u8 255
            0xFF, 0xFF, // i16 -1
            0x2C, 0x01, // u16 300
            0xFF, 0xFF, 0xFF, 0xFF, // i32 -1
            0x78, 0x56, 0x34, 0x12, // u32 0x12345678 - the endianness vector
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // i64 -1
            0x01, 0, 0, 0, 0, 0, 0, 0, // u64 1
            0x00, 0x00, 0xC0, 0x3F, // f32 1.5
            0, 0, 0, 0, 0, 0, 0xF0, 0x3F, // f64 1.0
            0x03, 0, 0, 0, 0xE4, 0xB8, 0xAD, // string "中" - 3 bytes, not 1 char
            0x02, 0, 0, 0, 0xFF, 0xFE, // bytes
        ]
    );
}

// =============================================== the runtime the file carries

/// The generated file carries a conforming decoder, not a permissive one.
#[test]
fn the_embedded_runtime_rejects_malformed_input() {
    let mut value = DeviceState::default();

    // A bool is 0x00 or 0x01 and nothing else - "non-zero means true" is not
    // permitted (RFC-0002 §3).
    assert_eq!(Reader::new(&[0x02]).read_bool(), Err(DecodeError::InvalidBool(0x02)));

    // Fewer bytes than the value requires.
    assert_eq!(
        DeviceStateEdgeCodec::decode(&mut Reader::new(&[0x2A, 0x00, 0x00]), &mut value),
        Err(DecodeError::UnexpectedEof { needed: 4, remaining: 3 })
    );

    // A string region that is not valid UTF-8.
    let bytes = [0x2A, 0, 0, 0, 0x02, 0, 0, 0, 0xFF, 0xFE];
    assert_eq!(
        DeviceStateUnityCodec::decode(&mut Reader::new(&bytes), &mut value),
        Err(DecodeError::InvalidUtf8)
    );
}

/// `Limits` reaches the generated code unchanged, so a caller can bound what an
/// untrusted stream may allocate.
#[test]
fn the_embedded_limits_apply_to_generated_decode() {
    let mut bytes = vec![0x2A, 0, 0, 0, 0x10, 0, 0, 0];
    bytes.extend_from_slice(b"0123456789abcdef");

    let limits = Limits { max_string_len: 8, ..Limits::UNLIMITED };
    let mut value = DeviceState::default();

    assert_eq!(
        DeviceStateUnityCodec::decode(&mut Reader::with_limits(&bytes, limits), &mut value),
        Err(DecodeError::LengthOverflow { length: 16, limit: 8 })
    );

    // The default is permissive: the same bytes decode fine.
    let mut value = DeviceState::default();
    DeviceStateUnityCodec::decode(&mut Reader::new(&bytes), &mut value).expect("decode");
    assert_eq!(value.display_name, "0123456789abcdef");
}

/// Floats are written as raw bits: `-0.0` stays distinct from `0.0`, and nothing
/// is canonicalized.
#[test]
fn floats_keep_their_bits() {
    let negative = encode(
        &DeviceState { temperature: -0.0, ..DeviceState::default() },
        DeviceStateEdgeCodec::encode,
    );
    let positive = encode(
        &DeviceState { temperature: 0.0, ..DeviceState::default() },
        DeviceStateEdgeCodec::encode,
    );

    assert_ne!(negative, positive);

    let mut value = DeviceState::default();
    DeviceStateEdgeCodec::decode(&mut Reader::new(&negative), &mut value).expect("decode");
    assert!(value.temperature.is_sign_negative());
}

// ================================================================ edge cases

/// §15 - a declared codec is generated even when no field joined it.
#[test]
fn a_codec_no_field_joined_is_still_generated() {
    let bytes = encode(&NoFieldsJoined { id: 1 }, NoFieldsJoinedLonelyCodec::encode);
    assert!(bytes.is_empty(), "a model with no routed field occupies zero bytes");

    let mut value = NoFieldsJoined::default();
    NoFieldsJoinedLonelyCodec::decode(&mut Reader::new(&[]), &mut value).expect("decode");
}

/// A struct nothing marks is not a model, and its neighbours are unaffected.
#[test]
fn an_unmarked_struct_does_not_disturb_the_next_model() {
    let _ = NotAModel { whatever: 1 };

    assert_eq!(
        encode(&AfterTheUnmarkedStruct { value: 5 }, AfterTheUnmarkedStructEdgeCodec::encode),
        [0x05, 0x00, 0x00, 0x00]
    );
}

/// Encoding the same value twice produces the same bytes.
#[test]
fn encoding_is_deterministic() {
    assert_eq!(
        encode(&sample(), DeviceStateEdgeCodec::encode),
        encode(&sample(), DeviceStateEdgeCodec::encode)
    );
}
