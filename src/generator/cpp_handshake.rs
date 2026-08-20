use std::collections::BTreeMap;

use crate::ir::Schema;
use crate::model::screaming_snake_case;
use crate::schema::hex64;

pub const FILE_NAME: &str = "handshake.hpp";

pub fn handshake_file(
    schema: &Schema,
    namespace: &str,
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
    out.push_str("#pragma once\n\n");
    out.push_str(
        "#include <cstddef>\n#include <cstdint>\n#include <optional>\n#include <string>\n\
         #include <vector>\n",
    );
    if validate_message_fingerprint {
        out.push_str("\n#include \"runtime.hpp\"\n");
    }
    out.push('\n');

    out.push_str(&format!("namespace {namespace} {{\n\n"));

    out.push_str(
        "/// The fingerprint of the whole schema: every message, by name, with its own\n\
         /// fingerprint, hashed together. Two peers that agree on this agree on\n\
         /// everything.\n",
    );
    out.push_str(&format!(
        "inline constexpr std::uint64_t CYCLONE_SCHEMA_FINGERPRINT = {}ULL;\n\n",
        hex64(schema.fingerprint.u64())
    ));

    for model in &schema.models {
        out.push_str(&format!(
            "/// `{}`, as declared - every annotated field, whatever codec it joined.\n",
            model.name
        ));
        out.push_str(&format!(
            "inline constexpr std::uint64_t {}_FINGERPRINT = {}ULL;\n",
            screaming_snake_case(&model.name),
            hex64(model.fingerprint.u64())
        ));

        for message in &model.messages {
            out.push_str(&format!(
                "/// `{}` - the wire contract `{}` encodes and decodes.\n",
                message.name,
                super::codec_type_name(&model.name, &message.codec)
            ));
            out.push_str(&format!(
                "inline constexpr std::uint32_t {}_MESSAGE_ID = 0x{:08X}u;\n",
                message_constant(&model.name, &message.codec),
                message.id
            ));
            out.push_str(&format!(
                "inline constexpr std::uint64_t {}_FINGERPRINT = {}ULL;\n",
                message_constant(&model.name, &message.codec),
                hex64(message.fingerprint.u64())
            ));
            let constant = message_constant(&model.name, &message.codec);
            out.push_str(&format!(
                "/// One fingerprint per prefix of `{}`: entry `k-1` covers its first `k`\n\
                 /// fields. The last entry is `{constant}_FINGERPRINT`. Never sent whole - a\n\
                 /// peer sends its field count and its last entry, and the two sides compare\n\
                 /// at `min` of the two counts (RFC-0002 §9.1).\n",
                message.name
            ));
            out.push_str(&format!(
                "inline constexpr std::uint64_t {constant}_PREFIXES[] = {{\n"
            ));
            if message.prefixes.is_empty() {
                out.push_str("    0ULL,\n");
            } else {
                for prefix in &message.prefixes {
                    out.push_str(&format!("    {}ULL,\n", hex64(prefix.u64())));
                }
            }
            out.push_str("};\n");
            out.push_str(&format!(
                "inline constexpr std::size_t {constant}_PREFIX_COUNT = {};\n",
                message.prefixes.len()
            ));
        }
        out.push('\n');
    }

    out.push_str(TYPES);

    let mut messages: Vec<_> = schema.messages().collect();
    messages.sort_by_key(|message| message.id);

    out.push_str("/// Every message this schema declares, sorted by id.\n");
    out.push_str("inline const std::vector<CycloneMessage> CYCLONE_MESSAGES = {\n");
    for message in &messages {
        let constant = message_constant(&message.model, &message.codec);
        out.push_str(&format!(
            "    CycloneMessage{{{constant}_MESSAGE_ID, {:?}, {constant}_FINGERPRINT, \
             {constant}_PREFIXES, {constant}_PREFIX_COUNT}},\n",
            message.name
        ));
    }
    out.push_str("};\n");

    out.push_str(HANDSHAKE);

    out.push_str(&format!(
        "\n/// Whether this schema was generated with `validate_message_fingerprint`.\n\
         inline constexpr bool CYCLONE_VALIDATE_MESSAGE_FINGERPRINT = {validate_message_fingerprint};\n"
    ));
    if validate_message_fingerprint {
        out.push_str(ENVELOPE);
    } else {
        out.push_str(ENVELOPE_OFF);
    }

    out.push_str(&format!("\n}}  // namespace {namespace}\n"));

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
struct CycloneMessage {
    /// Stable across schema changes; derived from the name alone.
    std::uint32_t id;
    /// `Model.codec`.
    const char* name;
    /// Changes whenever the message's fields do.
    std::uint64_t fingerprint;
    /// One entry per field: entry `k-1` covers the first `k` fields. The last
    /// entry is `fingerprint`. Stays local; only its length and its last entry
    /// ever go on the wire.
    const std::uint64_t* prefixes;
    std::size_t prefix_count;
};

";

const HANDSHAKE: &str = r####"
/// What a peer's fingerprints mean for this one.
enum class CycloneHandshake {
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
};

/// What one of the peer's messages means for this schema's message of the
/// same id.
enum class CycloneMessageCheck {
    /// Either this schema does not declare the message at all, or the fields
    /// both ends carry agree. Nothing to do.
    Match,
    /// Both ends put different fields at an index both of them carry.
    Reject,
    /// Undecidable from what the peer sent: the peer has more fields than this
    /// schema, so the answer lives at an index only the peer can produce. Ask
    /// it for its prefix fingerprint at the reported field count, then compare
    /// the reply against `cyclone_prefix` for the same id.
    NeedPrefix,
};

/// One entry of a peer's `(id, field count, fingerprint)` table - what
/// `CYCLONE_MESSAGES` is on its side.
struct CyclonePeerMessage {
    std::uint32_t id;
    std::uint32_t field_count;
    std::uint64_t fingerprint;
};

/// The message with this id, or `nullptr` if this schema does not declare it.
inline const CycloneMessage* cyclone_message(std::uint32_t id) {
    std::size_t low = 0;
    std::size_t high = CYCLONE_MESSAGES.size();
    while (low < high) {
        std::size_t middle = (low + high) / 2;
        if (CYCLONE_MESSAGES[middle].id < id) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    if (low < CYCLONE_MESSAGES.size() && CYCLONE_MESSAGES[low].id == id) {
        return &CYCLONE_MESSAGES[low];
    }
    return nullptr;
}

/// This schema's fingerprint for the first `field_count` fields of a message,
/// or `std::nullopt` if it does not declare that message or does not have that
/// many fields. `field_count` counts from 1; 0 is the empty prefix and has no
/// fingerprint because it always matches.
inline std::optional<std::uint64_t> cyclone_prefix(std::uint32_t id, std::uint32_t field_count) {
    const CycloneMessage* message = cyclone_message(id);
    if (message == nullptr || field_count == 0 ||
        static_cast<std::size_t>(field_count) > message->prefix_count) {
        return std::nullopt;
    }
    return message->prefixes[field_count - 1];
}

/// What `cyclone_check_message` decided, and which field count to ask the peer
/// about. `ask_for` is only meaningful for `CycloneMessageCheck::NeedPrefix`.
struct CycloneMessageOutcome {
    CycloneMessageCheck check;
    std::uint32_t ask_for;
};

/// Compares one of the peer's messages against this schema's.
///
/// `peer_field_count` and `peer_fingerprint` are what the peer declares for
/// this id. This is RFC-0002 §9.1's prefix test: the two are compatible when
/// the shorter field list is an exact prefix of the longer one, so the
/// comparison happens at the smaller of the two field counts.
inline CycloneMessageOutcome cyclone_check_message(std::uint32_t id,
                                                   std::uint32_t peer_field_count,
                                                   std::uint64_t peer_fingerprint) {
    const CycloneMessage* known = cyclone_message(id);
    if (known == nullptr) {
        // Not a message this schema declares, so it is never exchanged.
        return {CycloneMessageCheck::Match, 0};
    }
    const auto local_field_count = static_cast<std::uint32_t>(known->prefix_count);

    if (peer_fingerprint == known->fingerprint) {
        return {CycloneMessageCheck::Match, 0};
    }
    if (peer_field_count == 0 || local_field_count == 0) {
        // The empty field list is a prefix of everything.
        return {CycloneMessageCheck::Match, 0};
    }
    if (peer_field_count == local_field_count) {
        // Same length, different content - a prefix of equal length would have
        // to be equality, and it is not.
        return {CycloneMessageCheck::Reject, 0};
    }
    if (peer_field_count < local_field_count) {
        // The peer's own fingerprint already is the value at the shared index.
        return {known->prefixes[peer_field_count - 1] == peer_fingerprint
                    ? CycloneMessageCheck::Match
                    : CycloneMessageCheck::Reject,
                0};
    }
    return {CycloneMessageCheck::NeedPrefix, local_field_count};
}

/// Compares a peer's whole message table against this schema's.
///
/// `peer_messages` is the peer's `(id, field count, fingerprint)` table - what
/// `CYCLONE_MESSAGES` is on its side. A `NeedMore` result means at least one
/// message needs the extra round; walk the table with `cyclone_check_message`
/// to find which ones.
inline CycloneHandshake cyclone_handshake(
    std::uint64_t peer_schema_fingerprint,
    const std::vector<CyclonePeerMessage>& peer_messages) {
    if (peer_schema_fingerprint == CYCLONE_SCHEMA_FINGERPRINT) {
        return CycloneHandshake::Current;
    }

    bool need_more = false;
    for (const auto& peer : peer_messages) {
        switch (cyclone_check_message(peer.id, peer.field_count, peer.fingerprint).check) {
            case CycloneMessageCheck::Reject:
                // One mismatch decides the whole session. Every other message
                // could agree and it would still be unsafe to speak.
                return CycloneHandshake::Reject;
            case CycloneMessageCheck::NeedPrefix:
                need_more = true;
                break;
            case CycloneMessageCheck::Match:
                break;
        }
    }

    return need_more ? CycloneHandshake::NeedMore : CycloneHandshake::Outdated;
}
"####;

const ENVELOPE: &str = r####"
// ==========================================================================
// Per-frame validation - validate_message_fingerprint = true.
//
//     [MessageId: u32][MessageFingerprint: u64][Payload]
//
// Twelve bytes in front of every message, so that a peer that got past the
// handshake still cannot decode one message as another. Off by default: the
// wire format's premise is that there is no metadata on it, and the handshake
// already answers this question once per connection instead of once per
// frame.
// ==========================================================================

/// A frame whose envelope did not describe a message this schema can decode.
struct CycloneEnvelopeError {
    enum class Kind {
        /// The envelope itself could not be read.
        Malformed,
        /// An id this schema does not declare.
        UnknownMessage,
        /// The right message, the wrong shape.
        FingerprintMismatch,
    };

    Kind kind = Kind::Malformed;
    /// `Kind::Malformed`.
    DecodeError malformed{};
    /// `Kind::UnknownMessage` / `Kind::FingerprintMismatch`.
    std::uint32_t id = 0;
    /// `Kind::FingerprintMismatch`.
    std::uint64_t expected = 0;
    /// `Kind::FingerprintMismatch`.
    std::uint64_t received = 0;

    std::string message() const {
        char buffer[160];
        switch (kind) {
            case Kind::Malformed:
                return "malformed envelope: " + malformed.message();
            case Kind::UnknownMessage:
                std::snprintf(buffer, sizeof(buffer), "unknown message id 0x%08X",
                              static_cast<unsigned>(id));
                return buffer;
            case Kind::FingerprintMismatch:
                std::snprintf(
                    buffer, sizeof(buffer),
                    "message 0x%08X: peer fingerprint 0x%016llX, ours 0x%016llX",
                    static_cast<unsigned>(id),
                    static_cast<unsigned long long>(received),
                    static_cast<unsigned long long>(expected));
                return buffer;
        }
        return "unknown envelope error";
    }
};

/// Writes `[MessageId][MessageFingerprint]`, immediately before the payload.
inline void cyclone_write_envelope(Writer& writer, const CycloneMessage& message) {
    writer.write_u32(message.id);
    writer.write_u64(message.fingerprint);
}

/// Reads an envelope and resolves it against this schema. On success, `out`
/// is left pointing at the resolved message and the reader is positioned at
/// the payload; on failure `error` describes what went wrong and `out` is
/// untouched.
inline bool cyclone_read_envelope(Reader& reader, const CycloneMessage*& out,
                                   CycloneEnvelopeError& error) {
    std::uint32_t id = 0;
    if (DecodeError decode_error = reader.read_u32(id); !decode_error.ok()) {
        error.kind = CycloneEnvelopeError::Kind::Malformed;
        error.malformed = decode_error;
        return false;
    }
    std::uint64_t fingerprint = 0;
    if (DecodeError decode_error = reader.read_u64(fingerprint); !decode_error.ok()) {
        error.kind = CycloneEnvelopeError::Kind::Malformed;
        error.malformed = decode_error;
        return false;
    }

    const CycloneMessage* message = cyclone_message(id);
    if (message == nullptr) {
        error.kind = CycloneEnvelopeError::Kind::UnknownMessage;
        error.id = id;
        return false;
    }
    if (message->fingerprint != fingerprint) {
        error.kind = CycloneEnvelopeError::Kind::FingerprintMismatch;
        error.id = id;
        error.expected = message->fingerprint;
        error.received = fingerprint;
        return false;
    }

    out = message;
    return true;
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
            source: PathBuf::from("models.hpp"),
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
        handshake_file(&Schema::build(models).expect("build"), "generated", false).expect("render")
    }

    #[test]
    fn every_constant_the_brief_asks_for_is_generated() {
        let text = generated(&[model("Player", &["edge"]), model("Enemy", &["edge"])]);

        assert!(text.contains("namespace generated {\n"), "{text}");
        assert!(
            text.contains("inline constexpr std::uint64_t CYCLONE_SCHEMA_FINGERPRINT = 0x"),
            "{text}"
        );
        assert!(
            text.contains("inline constexpr std::uint64_t PLAYER_FINGERPRINT = 0x"),
            "{text}"
        );
        assert!(
            text.contains("inline constexpr std::uint64_t ENEMY_FINGERPRINT = 0x"),
            "{text}"
        );
        assert!(
            text.contains("inline constexpr std::uint64_t PLAYER_EDGE_FINGERPRINT = 0x"),
            "{text}"
        );
        assert!(
            text.contains("inline constexpr std::uint32_t PLAYER_EDGE_MESSAGE_ID = 0x"),
            "{text}"
        );
        assert!(
            text.contains("inline const std::vector<CycloneMessage> CYCLONE_MESSAGES ="),
            "{text}"
        );
    }

    #[test]
    fn the_message_table_is_sorted_by_id_so_lookup_can_bisect() {
        let text = generated(&[
            model("Player", &["edge", "unity"]),
            model("Enemy", &["edge"]),
        ]);

        let lines: Vec<&str> = text
            .lines()
            .filter(|line| line.trim_start().starts_with("CycloneMessage{"))
            .collect();
        assert_eq!(lines.len(), 3, "{text}");

        let schema = Schema::build(&[
            model("Player", &["edge", "unity"]),
            model("Enemy", &["edge"]),
        ])
        .expect("build");
        let mut sorted: Vec<u32> = schema.messages().map(|message| message.id).collect();
        sorted.sort_unstable();
        for (line, id) in lines.iter().zip(sorted) {
            let name = schema
                .messages()
                .find(|message| message.id == id)
                .expect("message")
                .name
                .clone();
            assert!(line.contains(&format!("{name:?}")), "{line} / {name}");
        }
    }

    #[test]
    fn the_envelope_is_off_unless_asked_for() {
        let off = generated(&[model("Player", &["edge"])]);
        assert!(
            off.contains("CYCLONE_VALIDATE_MESSAGE_FINGERPRINT = false;"),
            "{off}"
        );
        assert!(!off.contains("inline void cyclone_write_envelope"), "{off}");

        let schema = Schema::build(&[model("Player", &["edge"])]).expect("build");
        let on = handshake_file(&schema, "generated", true).expect("render");
        assert!(
            on.contains("CYCLONE_VALIDATE_MESSAGE_FINGERPRINT = true;"),
            "{on}"
        );
        assert!(on.contains("inline void cyclone_write_envelope"), "{on}");
        assert!(on.contains("inline bool cyclone_read_envelope"), "{on}");
        assert!(on.contains("#include \"runtime.hpp\"\n"), "{on}");
    }

    #[test]
    fn two_constants_spelled_the_same_are_reported_not_emitted() {
        let schema =
            Schema::build(&[model("Player", &["edge"]), model("PlayerEdge", &[])]).expect("build");
        let error = handshake_file(&schema, "generated", false).expect_err("collision");
        assert!(error.contains("PLAYER_EDGE_FINGERPRINT"), "{error}");
    }
}
