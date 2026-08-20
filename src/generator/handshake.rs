use std::collections::BTreeMap;

use crate::ir::Schema;
use crate::model::screaming_snake_case;
use crate::schema::hex64;

pub const FILE_NAME: &str = "handshake.rs";

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
            out.push_str(&format!(
                "/// One fingerprint per prefix of `{}`: entry `k-1` covers its first `k`\n\
                 /// fields. The last entry is `{}_FINGERPRINT`. Never sent whole - a peer\n\
                 /// sends its field count and its last entry, and the two sides compare at\n\
                 /// `min` of the two counts (RFC-0002 §9.1).\n",
                message.name,
                message_constant(&model.name, &message.codec)
            ));
            out.push_str(&format!(
                "pub const {}_PREFIXES: &[u64] = &[\n",
                message_constant(&model.name, &message.codec)
            ));
            for prefix in &message.prefixes {
                out.push_str(&format!("    {},\n", hex64(prefix.u64())));
            }
            out.push_str("];\n");
        }
        out.push('\n');
    }

    out.push_str(TYPES);

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
             fingerprint: {constant}_FINGERPRINT, prefixes: {constant}_PREFIXES }},\n",
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

fn message_constant(model: &str, codec: &str) -> String {
    format!(
        "{}_{}",
        screaming_snake_case(model),
        screaming_snake_case(codec)
    )
}

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
    /// One entry per field: entry `k-1` covers the first `k` fields. The last
    /// entry is `fingerprint`. Stays local; only its length and its last entry
    /// ever go on the wire.
    pub prefixes: &'static [u64],
}

";

const HANDSHAKE: &str = r####"
/// What a peer's fingerprints mean for this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CycloneHandshake {
    /// The same schema, exactly.
    Current,
    /// A different schema, but every message both ends know agrees on the
    /// fields both ends carry. Safe to proceed.
    Outdated,
    /// Both ends put different fields at an index both of them carry. There is
    /// nothing to negotiate: disconnect.
    Reject,
    /// Not decidable from the peer's table alone - at least one message needs
    /// the extra exchange described on `CycloneMessageCheck::NeedPrefix`.
    NeedMore,
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

/// This schema's fingerprint for the first `field_count` fields of a message,
/// or `None` if it does not declare that message or does not have that many
/// fields. `field_count` counts from 1; 0 is the empty prefix and has no
/// fingerprint because it always matches.
#[allow(dead_code)]
pub fn cyclone_prefix(id: u32, field_count: u32) -> ::core::option::Option<u64> {
    let message = cyclone_message(id)?;
    if field_count == 0 {
        return ::core::option::Option::None;
    }
    message.prefixes.get(field_count as usize - 1).copied()
}

/// What one of the peer's messages means for this schema's message of the same
/// id. See [`cyclone_message_check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CycloneMessageCheck {
    /// Either this schema does not declare the message at all, or the fields
    /// both ends carry agree. Nothing to do.
    Match,
    /// Both ends put different fields at an index both of them carry.
    Reject,
    /// Undecidable from what the peer sent: the peer has more fields than this
    /// schema, so the answer lives at an index only the peer can produce. Ask
    /// it for its prefix fingerprint at this field count, then feed the reply
    /// to `cyclone_prefix` for the same id and compare.
    NeedPrefix(u32),
}

/// Compares one of the peer's messages against this schema's.
///
/// `peer_field_count` and `peer_fingerprint` are what the peer declares for
/// this id - its field count and the fingerprint of its whole message. This is
/// RFC-0002 §9.1's prefix test: the two are compatible when the shorter field
/// list is an exact prefix of the longer one, so the comparison happens at
/// `min(peer_field_count, local field count)`.
#[allow(dead_code)]
pub fn cyclone_message_check(
    id: u32,
    peer_field_count: u32,
    peer_fingerprint: u64,
) -> CycloneMessageCheck {
    let ::core::option::Option::Some(known) = cyclone_message(id) else {
        // Not a message this schema declares, so it is never exchanged.
        return CycloneMessageCheck::Match;
    };
    let local_field_count = known.prefixes.len() as u32;

    if peer_fingerprint == known.fingerprint {
        return CycloneMessageCheck::Match;
    }
    if peer_field_count == 0 || local_field_count == 0 {
        // The empty field list is a prefix of everything.
        return CycloneMessageCheck::Match;
    }
    if peer_field_count == local_field_count {
        // Same length, different content - a prefix of equal length would have
        // to be equality, and it is not.
        return CycloneMessageCheck::Reject;
    }
    if peer_field_count < local_field_count {
        // The peer's own fingerprint already is the value at the shared index.
        return match known.prefixes.get(peer_field_count as usize - 1) {
            ::core::option::Option::Some(local) if *local == peer_fingerprint => {
                CycloneMessageCheck::Match
            }
            _ => CycloneMessageCheck::Reject,
        };
    }
    CycloneMessageCheck::NeedPrefix(local_field_count)
}

/// Compares a peer's whole message table against this schema's.
///
/// `peer_messages` is the peer's `(id, field count, fingerprint)` table - what
/// `CYCLONE_MESSAGES` is on its side. A `NeedMore` result means at least one
/// message needs the extra round described on
/// [`CycloneMessageCheck::NeedPrefix`]; walk the table with
/// `cyclone_message_check` to find which ones.
#[allow(dead_code)]
pub fn cyclone_handshake(
    peer_schema_fingerprint: u64,
    peer_messages: &[(u32, u32, u64)],
) -> CycloneHandshake {
    if peer_schema_fingerprint == CYCLONE_SCHEMA_FINGERPRINT {
        return CycloneHandshake::Current;
    }

    let mut need_more = false;
    for (id, field_count, fingerprint) in peer_messages {
        match cyclone_message_check(*id, *field_count, *fingerprint) {
            // One mismatch decides the whole session. Every other message
            // could agree and it would still be unsafe to speak.
            CycloneMessageCheck::Reject => return CycloneHandshake::Reject,
            CycloneMessageCheck::NeedPrefix(_) => need_more = true,
            CycloneMessageCheck::Match => {}
        }
    }

    if need_more {
        return CycloneHandshake::NeedMore;
    }
    CycloneHandshake::Outdated
}
"####;

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
