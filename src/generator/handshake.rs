//! `handshake.rs` - the fingerprints, generated.
//!
//! Nothing here is hand-written or hand-updated. A constant that a human keeps
//! in step with a schema is a constant that is one commit away from being
//! wrong, and a wrong fingerprint is worse than none: it is a handshake that
//! says *current* about two peers that disagree.
//!
//! # What a handshake exchanges
//!
//! The schema fingerprint, and - only if that differs - the per-message
//! fingerprint table. Never a schema. A schema is kilobytes of text that both
//! ends already have compiled in, and sending it would invite a peer to
//! *interpret* it, which is exactly the runtime schema resolution Cyclone
//! exists to not have.
//!
//! ```text
//! peer schema fingerprint == ours            → CURRENT   accept
//! a message both ends know, fingerprints differ → REJECT  disconnect
//! otherwise (each end knows messages the other does not)
//!                                            → OUTDATED  accept
//! ```
//!
//! The middle rule is the whole safety property. Two peers that both speak
//! `Player.edge` and disagree about its bytes cannot be allowed to exchange
//! it, and no amount of fingerprint arithmetic can tell them *how* they
//! disagree - that is [`crate::compat`]'s answer, at build time, from two
//! schemas. At runtime there is nothing to work out and nothing to negotiate.

use std::collections::BTreeMap;

use crate::ir::Schema;
use crate::model::screaming_snake_case;
use crate::schema::hex64;

/// The file name, relative to the output directory.
pub const FILE_NAME: &str = "handshake.rs";

/// Renders `handshake.rs`.
///
/// `validate_message_fingerprint` adds the optional per-frame envelope
/// helpers - see the brief's §11. It is off by default, because a fingerprint
/// in every frame is 12 bytes of overhead per message on a wire format whose
/// entire premise is that there is no metadata on it.
///
/// # Errors
///
/// Two constants that would be spelled the same - a model named `PlayerEdge`
/// beside a `Player` with an `edge` codec. Rare, mechanical, and far better
/// reported here than as a redefinition error in the user's build.
pub fn handshake_file(
    schema: &Schema,
    validate_message_fingerprint: bool,
) -> Result<String, String> {
    check_constant_names(schema)?;

    let mut out = super::Header {
        fingerprint: Some(schema.fingerprint.tagged()),
        note: Some(
            "Every fingerprint this schema publishes, and the handshake that compares\n\
             them. Generated - never edit, and never hand-maintain a copy of these\n\
             values anywhere else.",
        ),
        ..super::Header::default()
    }
    .render();
    out.push_str(super::FILE_ATTRIBUTES);

    // The handshake itself needs nothing from the runtime; the optional frame
    // envelope needs all of it.
    if validate_message_fingerprint {
        out.push_str("use super::runtime::{DecodeError, Reader, Writer};\n\n");
    }

    out.push_str(
        "/// The fingerprint of the whole schema: every message, by name, with its own\n\
         /// fingerprint, hashed together. Two peers that agree on this agree on\n\
         /// everything.\n",
    );
    out.push_str(&format!(
        "pub const CYCLONE_SCHEMA_FINGERPRINT: u64 = {};\n\n",
        hex64(schema.fingerprint.u64())
    ));

    for model in &schema.models {
        out.push_str(&format!(
            "/// `{}`, as declared - every annotated field, whatever codec it joined.\n",
            model.name
        ));
        out.push_str(&format!(
            "pub const {}_FINGERPRINT: u64 = {};\n",
            screaming_snake_case(&model.name),
            hex64(model.fingerprint.u64())
        ));

        for message in &model.messages {
            out.push_str(&format!(
                "/// `{}` - the wire contract `{}` encodes and decodes.\n",
                message.name,
                super::rust::codec_type_name(&model.name, &message.codec)
            ));
            out.push_str(&format!(
                "pub const {}_MESSAGE_ID: u32 = 0x{:08X};\n",
                message_constant(&model.name, &message.codec),
                message.id
            ));
            out.push_str(&format!(
                "pub const {}_FINGERPRINT: u64 = {};\n",
                message_constant(&model.name, &message.codec),
                hex64(message.fingerprint.u64())
            ));
        }
        out.push('\n');
    }

    out.push_str(TYPES);

    // Sorted by id, so a peer's table and ours can be compared without either
    // side sorting first.
    let mut messages: Vec<_> = schema.messages().collect();
    messages.sort_by_key(|message| message.id);

    out.push_str(
        "/// Every message this schema declares, sorted by id.\n\
         #[allow(dead_code)]\n\
         pub const CYCLONE_MESSAGES: &[CycloneMessage] = &[\n",
    );
    for message in &messages {
        let constant = message_constant(&message.model, &message.codec);
        out.push_str(&format!(
            "    CycloneMessage {{ id: {constant}_MESSAGE_ID, name: \"{}\", \
             fingerprint: {constant}_FINGERPRINT }},\n",
            message.name
        ));
    }
    out.push_str("];\n");

    out.push_str(HANDSHAKE);

    out.push_str(&format!(
        "\n/// Whether this schema was generated with `validate_message_fingerprint`.\n\
         #[allow(dead_code)]\n\
         pub const CYCLONE_VALIDATE_MESSAGE_FINGERPRINT: bool = {validate_message_fingerprint};\n"
    ));
    if validate_message_fingerprint {
        out.push_str(ENVELOPE);
    } else {
        out.push_str(ENVELOPE_OFF);
    }

    Ok(out)
}

/// `Player` + `edge` → `PLAYER_EDGE`.
fn message_constant(model: &str, codec: &str) -> String {
    format!(
        "{}_{}",
        screaming_snake_case(model),
        screaming_snake_case(codec)
    )
}

/// Two constants may not be spelled the same.
fn check_constant_names(schema: &Schema) -> Result<(), String> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();

    for model in &schema.models {
        let name = format!("{}_FINGERPRINT", screaming_snake_case(&model.name));
        seen.insert(name, model.name.clone());
    }
    for message in schema.messages() {
        let name = format!(
            "{}_FINGERPRINT",
            message_constant(&message.model, &message.codec)
        );
        if let Some(previous) = seen.insert(name.clone(), message.name.clone()) {
            return Err(format!(
                "'{previous}' and '{}' would both generate `{name}` - rename one of them",
                message.name
            ));
        }
    }

    Ok(())
}

/// The message descriptor, identical in every generated `handshake.rs`.
const TYPES: &str = "\
/// One message: its id, its name, and the fingerprint of its wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub struct CycloneMessage {
    /// Stable across schema changes; derived from the name alone.
    pub id: u32,
    /// `Model.codec`.
    pub name: &'static str,
    /// Changes whenever the message's fields do.
    pub fingerprint: u64,
}

";

/// The handshake itself, identical in every generated `handshake.rs`.
const HANDSHAKE: &str = r####"
/// What a peer's fingerprints mean for this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CycloneHandshake {
    /// The same schema, exactly.
    Current,
    /// A different schema, but no message both ends know disagrees. One side is
    /// older; every message they share is byte-identical.
    Outdated,
    /// A message both ends know has two different shapes. There is nothing to
    /// negotiate: disconnect.
    Reject,
}

/// The message with this id, if this schema declares it.
#[allow(dead_code)]
pub fn cyclone_message(id: u32) -> ::core::option::Option<&'static CycloneMessage> {
    let mut low = 0usize;
    let mut high = CYCLONE_MESSAGES.len();
    while low < high {
        let middle = (low + high) / 2;
        if CYCLONE_MESSAGES[middle].id < id {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    match CYCLONE_MESSAGES.get(low) {
        ::core::option::Option::Some(message) if message.id == id => {
            ::core::option::Option::Some(message)
        }
        _ => ::core::option::Option::None,
    }
}

/// Compares a peer's fingerprints against this schema's.
///
/// `peer_messages` is the peer's `(id, fingerprint)` table - what
/// `CYCLONE_MESSAGES` is on its side. It is only worth sending when the schema
/// fingerprints already differ.
#[allow(dead_code)]
pub fn cyclone_handshake(
    peer_schema_fingerprint: u64,
    peer_messages: &[(u32, u64)],
) -> CycloneHandshake {
    if peer_schema_fingerprint == CYCLONE_SCHEMA_FINGERPRINT {
        return CycloneHandshake::Current;
    }

    for (id, fingerprint) in peer_messages {
        if let ::core::option::Option::Some(known) = cyclone_message(*id) {
            if known.fingerprint != *fingerprint {
                // A message both ends know, with two shapes. Every other
                // message could match and it would still be unsafe to speak.
                return CycloneHandshake::Reject;
            }
        }
    }

    CycloneHandshake::Outdated
}
"####;

/// The optional per-frame envelope, when `validate_message_fingerprint` is on.
const ENVELOPE: &str = r####"
// ==========================================================================
// Per-frame validation - `validate_message_fingerprint = true`.
//
//     [MessageId: u32][MessageFingerprint: u64][Payload]
//
// Twelve bytes in front of every message, so that a peer that got past the
// handshake still cannot decode one message as another. Off by default: the
// wire format's premise is that there is no metadata on it, and the handshake
// already answers this question once per connection instead of once per frame.
// ==========================================================================

/// A frame whose envelope did not describe a message this schema can decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CycloneEnvelopeError {
    /// The envelope itself could not be read.
    Malformed(DecodeError),
    /// An id this schema does not declare.
    UnknownMessage { id: u32 },
    /// The right message, the wrong shape.
    FingerprintMismatch { id: u32, expected: u64, received: u64 },
}

impl ::core::fmt::Display for CycloneEnvelopeError {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        match *self {
            CycloneEnvelopeError::Malformed(error) => write!(f, "malformed envelope: {error}"),
            CycloneEnvelopeError::UnknownMessage { id } => {
                write!(f, "unknown message id 0x{id:08X}")
            }
            CycloneEnvelopeError::FingerprintMismatch { id, expected, received } => write!(
                f,
                "message 0x{id:08X}: peer fingerprint 0x{received:016X}, ours 0x{expected:016X}"
            ),
        }
    }
}

impl ::std::error::Error for CycloneEnvelopeError {}

/// Writes `[MessageId][MessageFingerprint]`, immediately before the payload.
#[allow(dead_code)]
pub fn cyclone_write_envelope(writer: &mut Writer, message: &CycloneMessage) {
    writer.write_u32(message.id);
    writer.write_u64(message.fingerprint);
}

/// Reads an envelope and resolves it against this schema, leaving the reader
/// positioned at the payload.
#[allow(dead_code)]
pub fn cyclone_read_envelope(
    reader: &mut Reader,
) -> ::core::result::Result<&'static CycloneMessage, CycloneEnvelopeError> {
    let id = reader.read_u32().map_err(CycloneEnvelopeError::Malformed)?;
    let fingerprint = reader.read_u64().map_err(CycloneEnvelopeError::Malformed)?;

    let ::core::option::Option::Some(message) = cyclone_message(id) else {
        return ::core::result::Result::Err(CycloneEnvelopeError::UnknownMessage { id });
    };
    if message.fingerprint != fingerprint {
        return ::core::result::Result::Err(CycloneEnvelopeError::FingerprintMismatch {
            id,
            expected: message.fingerprint,
            received: fingerprint,
        });
    }

    ::core::result::Result::Ok(message)
}
"####;

/// What stands in for the envelope when it is off.
const ENVELOPE_OFF: &str = "\
// Per-frame message validation is off, so no envelope is generated and no
// frame carries one. Turn it on in cyclone.toml:
//
//     validate_message_fingerprint = true
//
// and every frame gains [MessageId: u32][MessageFingerprint: u64] in front of
// its payload, with cyclone_write_envelope / cyclone_read_envelope to match.
";

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::handshake_file;
    use crate::ir::Schema;
    use crate::model::{Field, Model};

    fn model(name: &str, codecs: &[&str]) -> Model {
        Model {
            name: name.to_owned(),
            source: PathBuf::from("src/models.rs"),
            line: 1,
            codecs: codecs.iter().map(|codec| (*codec).to_owned()).collect(),
            fields: vec![Field {
                name: "id".to_owned(),
                network_type: "u32".to_owned(),
                codecs: codecs.iter().map(|codec| (*codec).to_owned()).collect(),
                line: 2,
            }],
        }
    }

    fn generated(models: &[Model]) -> String {
        handshake_file(&Schema::build(models).expect("build"), false).expect("render")
    }

    #[test]
    fn every_constant_the_brief_asks_for_is_generated() {
        let text = generated(&[model("Player", &["edge"]), model("Enemy", &["edge"])]);

        assert!(
            text.contains("pub const CYCLONE_SCHEMA_FINGERPRINT: u64 = 0x"),
            "{text}"
        );
        assert!(
            text.contains("pub const PLAYER_FINGERPRINT: u64 = 0x"),
            "{text}"
        );
        assert!(
            text.contains("pub const ENEMY_FINGERPRINT: u64 = 0x"),
            "{text}"
        );
        assert!(
            text.contains("pub const PLAYER_EDGE_FINGERPRINT: u64 = 0x"),
            "{text}"
        );
        assert!(
            text.contains("pub const PLAYER_EDGE_MESSAGE_ID: u32 = 0x"),
            "{text}"
        );
        assert!(
            text.contains("pub const CYCLONE_MESSAGES: &[CycloneMessage]"),
            "{text}"
        );
    }

    #[test]
    fn the_message_table_is_sorted_by_id_so_lookup_can_bisect() {
        let text = generated(&[
            model("Player", &["edge", "unity"]),
            model("Enemy", &["edge"]),
        ]);

        let ids: Vec<&str> = text
            .lines()
            .filter(|line| line.trim_start().starts_with("CycloneMessage {"))
            .collect();
        assert_eq!(ids.len(), 3, "{text}");

        let schema = Schema::build(&[
            model("Player", &["edge", "unity"]),
            model("Enemy", &["edge"]),
        ])
        .expect("build");
        let mut sorted: Vec<u32> = schema.messages().map(|message| message.id).collect();
        sorted.sort_unstable();
        for (line, id) in ids.iter().zip(sorted) {
            let name = schema
                .messages()
                .find(|message| message.id == id)
                .expect("message")
                .name
                .clone();
            assert!(line.contains(&format!("\"{name}\"")), "{line} / {name}");
        }
    }

    #[test]
    fn the_envelope_is_off_unless_asked_for() {
        let off = generated(&[model("Player", &["edge"])]);
        assert!(
            off.contains("CYCLONE_VALIDATE_MESSAGE_FINGERPRINT: bool = false"),
            "{off}"
        );
        assert!(!off.contains("fn cyclone_write_envelope"), "{off}");

        let schema = Schema::build(&[model("Player", &["edge"])]).expect("build");
        let on = handshake_file(&schema, true).expect("render");
        assert!(
            on.contains("CYCLONE_VALIDATE_MESSAGE_FINGERPRINT: bool = true"),
            "{on}"
        );
        assert!(on.contains("fn cyclone_write_envelope"), "{on}");
        assert!(on.contains("fn cyclone_read_envelope"), "{on}");
    }

    #[test]
    fn two_constants_spelled_the_same_are_reported_not_emitted() {
        let schema =
            Schema::build(&[model("Player", &["edge"]), model("PlayerEdge", &[])]).expect("build");
        let error = handshake_file(&schema, false).expect_err("collision");
        assert!(error.contains("PLAYER_EDGE_FINGERPRINT"), "{error}");
    }
}
