//! The committed test vectors, checked against this implementation.
//!
//! `tests/vectors/cyclone-vectors.json` is the cross-SDK artifact: fixed bytes
//! and fixed digests that a Go, C# or C++ implementation is expected to
//! reproduce from the same schema. This is the Rust side of that promise.
//!
//! Two things are checked, and they fail for two different reasons:
//!
//! - **the fingerprints**, against the schema in `tests/fixtures/`. A failure
//!   here means the canonical form or the digest changed - every SDK's
//!   fingerprints just changed with it, and every deployed peer stops
//!   recognising every other one. That is a deliberate, coordinated, versioned
//!   act (`cyclone-fingerprint/2`), never a refactor.
//! - **the bytes**, through the real generated codecs. A failure here means the
//!   wire format moved.
//!
//! An `accept` vector is decoded and then encoded again: what comes back must
//! be the vector's `reencode` bytes. That is a byte-exact check of both halves
//! at once, and it is what lets the version-skew vectors state their answer -
//! `player-skew-empty` decodes from nothing and re-encodes to twelve zero
//! bytes, which is RFC-0002 §9.1 written as data.

#![allow(dead_code)]

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

use cyclonec::json::Json;

fn vectors() -> Json {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/cyclone-vectors.json"
    );
    let text = std::fs::read_to_string(path).expect("read the vectors");
    cyclonec::json::parse(&text).expect("parse the vectors")
}

fn schema() -> cyclonec::ir::Schema {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/.cyclone/schema.json"
    );
    let text = std::fs::read_to_string(path).expect("read the fixture schema");
    cyclonec::schema::from_json(&text).expect("read the fixture schema")
}

fn hex(text: &str) -> Vec<u8> {
    let digits: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    (0..digits.len() / 2)
        .map(|index| u8::from_str_radix(&digits[index * 2..index * 2 + 2], 16).expect("hex"))
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<String>>()
        .join("")
}

// ============================================================== the digests

/// Every fingerprint in the vectors file is what this generator computes today.
#[test]
fn the_fingerprints_are_unchanged() {
    let vectors = vectors();
    let schema = schema();

    assert_eq!(
        vectors.get("schema_fingerprint").and_then(Json::as_str),
        Some(schema.fingerprint.tagged().as_str()),
        "the schema fingerprint changed"
    );

    let messages = vectors
        .get("messages")
        .and_then(Json::as_object)
        .expect("messages");
    assert_eq!(messages.len(), schema.messages().count());

    for (name, expected) in messages {
        let message = schema
            .message(name)
            .unwrap_or_else(|| panic!("{name} is not in the fixture schema"));

        assert_eq!(
            expected.get("fingerprint").and_then(Json::as_str),
            Some(message.fingerprint.tagged().as_str()),
            "{name}'s fingerprint changed"
        );
        assert_eq!(
            expected.get("id").and_then(Json::as_str),
            Some(format!("0x{:08X}", message.id).as_str()),
            "{name}'s message id changed"
        );
    }
}

/// And the generated constants carry those same values into the build.
#[test]
fn the_generated_constants_carry_the_same_digests() {
    let schema = schema();

    assert_eq!(CYCLONE_SCHEMA_FINGERPRINT, schema.fingerprint.u64());
    for message in schema.messages() {
        let generated = cyclone_message(message.id)
            .unwrap_or_else(|| panic!("{} is missing from handshake.rs", message.name));
        assert_eq!(generated.name, message.name);
        assert_eq!(generated.fingerprint, message.fingerprint.u64());
    }
}

// ================================================================ the bytes

/// Decodes one vector with the named message's codec and encodes it again.
fn round_trip(message: &str, bytes: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let mut writer = Writer::new();
    let mut reader = Reader::new(bytes);

    match message {
        "Player.edge" => {
            let mut value = Player::default();
            PlayerEdgeCodec::decode(&mut reader, &mut value)?;
            PlayerEdgeCodec::encode(&mut writer, &value);
        }
        "PlayerInfo.edge" => {
            let mut value = PlayerInfo::default();
            PlayerInfoEdgeCodec::decode(&mut reader, &mut value)?;
            PlayerInfoEdgeCodec::encode(&mut writer, &value);
        }
        "Team.edge" => {
            let mut value = Team::default();
            TeamEdgeCodec::decode(&mut reader, &mut value)?;
            TeamEdgeCodec::encode(&mut writer, &value);
        }
        "DeviceState.edge" => {
            let mut value = DeviceState::default();
            DeviceStateEdgeCodec::decode(&mut reader, &mut value)?;
            DeviceStateEdgeCodec::encode(&mut writer, &value);
        }
        "DeviceState.unity" => {
            let mut value = DeviceState::default();
            DeviceStateUnityCodec::decode(&mut reader, &mut value)?;
            DeviceStateUnityCodec::encode(&mut writer, &value);
        }
        "EveryPrimitive.edge" => {
            let mut value = EveryPrimitive::default();
            EveryPrimitiveEdgeCodec::decode(&mut reader, &mut value)?;
            EveryPrimitiveEdgeCodec::encode(&mut writer, &value);
        }
        "Telemetry.edge" => {
            let mut value = Telemetry::default();
            TelemetryEdgeCodec::decode(&mut reader, &mut value)?;
            TelemetryEdgeCodec::encode(&mut writer, &value);
        }
        other => panic!("no codec wired up for {other} in this test"),
    }

    Ok(writer.into_bytes())
}

#[test]
fn every_accept_vector_round_trips_to_its_stated_bytes() {
    let vectors = vectors();
    let accept = vectors
        .get("accept")
        .and_then(Json::as_array)
        .expect("accept");
    assert!(!accept.is_empty());

    for vector in accept {
        let name = vector.get("name").and_then(Json::as_str).expect("name");
        let message = vector
            .get("message")
            .and_then(Json::as_str)
            .expect("message");
        let bytes = hex(vector.get("hex").and_then(Json::as_str).expect("hex"));
        let expected = hex(vector
            .get("reencode")
            .and_then(Json::as_str)
            .expect("reencode"));

        let produced = round_trip(message, &bytes)
            .unwrap_or_else(|error| panic!("{name}: {message} failed to decode: {error}"));

        assert_eq!(
            to_hex(&produced),
            to_hex(&expected),
            "{name}: {message} re-encoded to different bytes"
        );
    }
}

#[test]
fn every_reject_vector_is_rejected_for_the_stated_reason() {
    let vectors = vectors();
    let reject = vectors
        .get("reject")
        .and_then(Json::as_array)
        .expect("reject");
    assert!(!reject.is_empty());

    for vector in reject {
        let name = vector.get("name").and_then(Json::as_str).expect("name");
        let message = vector
            .get("message")
            .and_then(Json::as_str)
            .expect("message");
        let bytes = hex(vector.get("hex").and_then(Json::as_str).expect("hex"));
        let expected = vector.get("error").and_then(Json::as_str).expect("error");

        let error = round_trip(message, &bytes)
            .expect_err(&format!("{name}: {message} should not have decoded"));

        let actual = match error {
            DecodeError::UnexpectedEof { .. } => "UnexpectedEof",
            DecodeError::InvalidBool(_) => "InvalidBool",
            DecodeError::InvalidUtf8 => "InvalidUtf8",
            DecodeError::LengthOverflow { .. } => "LengthOverflow",
        };
        assert_eq!(actual, expected, "{name}: wrong error");
    }
}
