//! `handshake.h` - the fingerprints, generated.
//!
//! The C counterpart of [`super::cpp_handshake`] - same contract, same
//! safety property:
//!
//! ```text
//! peer schema fingerprint == ours               -> Current    accept
//! a message both ends know, fingerprints differ -> Reject     disconnect
//! otherwise                                      -> Outdated   accept
//! ```
//!
//! reshaped the same way [`super::c`] reshapes [`super::cpp`]: `static const`
//! globals instead of `namespace`-scoped `inline constexpr` ones (each header
//! gets its own private copy per translation unit, exactly like every
//! `static inline` function here - no linker involved either way),
//! `cyclone_message` returning `const CycloneMessage *`/`NULL` in place of a
//! `nullptr`-returning pointer, and the optional per-frame envelope's write
//! side returning `bool` rather than nothing at all, because
//! [`super::c_runtime`]'s `Writer` can fail where C++'s cannot (see its
//! module docs).

use std::collections::BTreeMap;

use crate::ir::Schema;
use crate::model::screaming_snake_case;
use crate::schema::hex64;

/// The file name, relative to the output directory.
pub const FILE_NAME: &str = "handshake.h";

/// Renders `handshake.h`.
///
/// `validate_message_fingerprint` adds the optional per-frame envelope
/// helpers - see the brief's SS11. Off by default, for the same reason as
/// every other backend: a fingerprint in every frame is 12 bytes of overhead
/// per message on a wire format whose entire premise is that there is no
/// metadata on it.
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
    out.push_str("#pragma once\n\n");
    out.push_str("#include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n");
    // The handshake itself needs nothing from the runtime; the optional frame
    // envelope needs CycloneDecodeError, CycloneReader and CycloneWriter.
    if validate_message_fingerprint {
        out.push_str("#include <stdio.h>\n\n#include \"runtime.h\"\n");
    }
    out.push('\n');

    out.push_str(
        "// The fingerprint of the whole schema: every message, by name, with its own\n\
         // fingerprint, hashed together. Two peers that agree on this agree on\n\
         // everything.\n",
    );
    out.push_str(&format!(
        "static const uint64_t CYCLONE_SCHEMA_FINGERPRINT = {}ULL;\n\n",
        hex64(schema.fingerprint.u64())
    ));

    for model in &schema.models {
        out.push_str(&format!(
            "// `{}`, as declared - every annotated field, whatever codec it joined.\n",
            model.name
        ));
        out.push_str(&format!(
            "static const uint64_t {}_FINGERPRINT = {}ULL;\n",
            screaming_snake_case(&model.name),
            hex64(model.fingerprint.u64())
        ));

        for message in &model.messages {
            out.push_str(&format!(
                "// `{}` - the wire contract `{}` encodes and decodes.\n",
                message.name,
                super::codec_type_name(&model.name, &message.codec)
            ));
            out.push_str(&format!(
                "static const uint32_t {}_MESSAGE_ID = 0x{:08X}u;\n",
                message_constant(&model.name, &message.codec),
                message.id
            ));
            out.push_str(&format!(
                "static const uint64_t {}_FINGERPRINT = {}ULL;\n",
                message_constant(&model.name, &message.codec),
                hex64(message.fingerprint.u64())
            ));
        }
        out.push('\n');
    }

    out.push_str(TYPES);

    // Sorted by id, so a peer's table and ours can be compared without either
    // side sorting first, and so `cyclone_message` can bisect.
    let mut messages: Vec<_> = schema.messages().collect();
    messages.sort_by_key(|message| message.id);

    out.push_str("// Every message this schema declares, sorted by id.\n");
    if messages.is_empty() {
        out.push_str("static const CycloneMessage *const CYCLONE_MESSAGES = NULL;\n");
        out.push_str("static const size_t CYCLONE_MESSAGES_COUNT = 0;\n");
    } else {
        out.push_str("static const CycloneMessage CYCLONE_MESSAGES[] = {\n");
        for message in &messages {
            let constant = message_constant(&message.model, &message.codec);
            out.push_str(&format!(
                "    {{{constant}_MESSAGE_ID, {:?}, {constant}_FINGERPRINT}},\n",
                message.name
            ));
        }
        out.push_str("};\n");
        out.push_str(
            "static const size_t CYCLONE_MESSAGES_COUNT = \
             sizeof(CYCLONE_MESSAGES) / sizeof(CYCLONE_MESSAGES[0]);\n",
        );
    }

    out.push_str(HANDSHAKE);

    out.push_str(&format!(
        "\n// Whether this schema was generated with `validate_message_fingerprint`.\n\
         static const bool CYCLONE_VALIDATE_MESSAGE_FINGERPRINT = {validate_message_fingerprint};\n"
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

/// The message descriptor, identical in every generated `handshake.h`.
const TYPES: &str = "\
// One message: its id, its name, and the fingerprint of its wire contract.
typedef struct CycloneMessage {
    // Stable across schema changes; derived from the name alone.
    uint32_t id;
    // `Model.codec`.
    const char *name;
    // Changes whenever the message's fields do.
    uint64_t fingerprint;
} CycloneMessage;

";

/// The handshake itself, identical in every generated `handshake.h`.
const HANDSHAKE: &str = "
// What a peer's fingerprints mean for this one.
typedef enum CycloneHandshake {
    // The same schema, exactly.
    CYCLONE_HANDSHAKE_CURRENT,
    // A different schema, but no message both ends know disagrees. One side
    // is older; every message they share is byte-identical.
    CYCLONE_HANDSHAKE_OUTDATED,
    // A message both ends know has two different shapes. There is nothing
    // to negotiate: disconnect.
    CYCLONE_HANDSHAKE_REJECT,
} CycloneHandshake;

// One entry of a peer's (id, fingerprint) table - what `CYCLONE_MESSAGES` is
// on its side.
typedef struct CyclonePeerMessage {
    uint32_t id;
    uint64_t fingerprint;
} CyclonePeerMessage;

// The message with this id, or `NULL` if this schema does not declare it.
static inline const CycloneMessage *cyclone_message(uint32_t id) {
    size_t low = 0;
    size_t high = CYCLONE_MESSAGES_COUNT;
    while (low < high) {
        size_t middle = (low + high) / 2;
        if (CYCLONE_MESSAGES[middle].id < id) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    if (low < CYCLONE_MESSAGES_COUNT && CYCLONE_MESSAGES[low].id == id) {
        return &CYCLONE_MESSAGES[low];
    }
    return NULL;
}

// Compares a peer's fingerprints against this schema's.
//
// `peer_messages`/`peer_messages_count` is the peer's (id, fingerprint)
// table - what `CYCLONE_MESSAGES`/`CYCLONE_MESSAGES_COUNT` is on its side.
// It is only worth sending when the schema fingerprints already differ.
static inline CycloneHandshake cyclone_handshake(uint64_t peer_schema_fingerprint,
                                                   const CyclonePeerMessage *peer_messages,
                                                   size_t peer_messages_count) {
    if (peer_schema_fingerprint == CYCLONE_SCHEMA_FINGERPRINT) {
        return CYCLONE_HANDSHAKE_CURRENT;
    }

    for (size_t i = 0; i < peer_messages_count; ++i) {
        const CycloneMessage *known = cyclone_message(peer_messages[i].id);
        if (known != NULL && known->fingerprint != peer_messages[i].fingerprint) {
            // A message both ends know, with two shapes. Every other
            // message could match and it would still be unsafe to speak.
            return CYCLONE_HANDSHAKE_REJECT;
        }
    }

    return CYCLONE_HANDSHAKE_OUTDATED;
}
";

/// The optional per-frame envelope, when `validate_message_fingerprint` is
/// on.
const ENVELOPE: &str = "
// ==========================================================================
// Per-frame validation - validate_message_fingerprint = true.
//
//     [MessageId: u32][MessageFingerprint: u64][Payload]
//
// Twelve bytes in front of every message, so that a peer that got past the
// handshake still cannot decode one message as another. Off by default: the
// wire format's premise is that there is no metadata on it, and the
// handshake already answers this question once per connection instead of
// once per frame.
// ==========================================================================

// A frame whose envelope did not describe a message this schema can decode.
typedef enum CycloneEnvelopeErrorKind {
    // The envelope itself could not be read.
    CYCLONE_ENVELOPE_MALFORMED,
    // An id this schema does not declare.
    CYCLONE_ENVELOPE_UNKNOWN_MESSAGE,
    // The right message, the wrong shape.
    CYCLONE_ENVELOPE_FINGERPRINT_MISMATCH,
} CycloneEnvelopeErrorKind;

typedef struct CycloneEnvelopeError {
    CycloneEnvelopeErrorKind kind;
    // CYCLONE_ENVELOPE_MALFORMED.
    CycloneDecodeError malformed;
    // CYCLONE_ENVELOPE_UNKNOWN_MESSAGE / CYCLONE_ENVELOPE_FINGERPRINT_MISMATCH.
    uint32_t id;
    // CYCLONE_ENVELOPE_FINGERPRINT_MISMATCH.
    uint64_t expected;
    // CYCLONE_ENVELOPE_FINGERPRINT_MISMATCH.
    uint64_t received;
} CycloneEnvelopeError;

static inline void cyclone_envelope_error_message(const CycloneEnvelopeError *error, char *buffer,
                                                    size_t size) {
    switch (error->kind) {
        case CYCLONE_ENVELOPE_MALFORMED: {
            char inner[160];
            cyclone_decode_error_message(&error->malformed, inner, sizeof(inner));
            snprintf(buffer, size, \"malformed envelope: %s\", inner);
            return;
        }
        case CYCLONE_ENVELOPE_UNKNOWN_MESSAGE:
            snprintf(buffer, size, \"unknown message id 0x%08X\", (unsigned)error->id);
            return;
        case CYCLONE_ENVELOPE_FINGERPRINT_MISMATCH:
            snprintf(buffer, size, \"message 0x%08X: peer fingerprint 0x%016llX, ours 0x%016llX\",
                      (unsigned)error->id, (unsigned long long)error->received,
                      (unsigned long long)error->expected);
            return;
    }
    snprintf(buffer, size, \"unknown envelope error\");
}

// Writes [MessageId][MessageFingerprint], immediately before the payload.
// Returns `false` if the writer's buffer failed to grow.
static inline bool cyclone_write_envelope(CycloneWriter *writer, const CycloneMessage *message) {
    if (!cyclone_writer_write_u32(writer, message->id)) return false;
    if (!cyclone_writer_write_u64(writer, message->fingerprint)) return false;
    return true;
}

// Reads an envelope and resolves it against this schema. On success, `*out`
// is left pointing at the resolved message and the reader is positioned at
// the payload; on failure `*error` describes what went wrong and `*out` is
// untouched.
static inline bool cyclone_read_envelope(CycloneReader *reader, const CycloneMessage **out,
                                          CycloneEnvelopeError *error) {
    uint32_t id = 0;
    CycloneDecodeError decode_error = cyclone_reader_read_u32(reader, &id);
    if (!cyclone_decode_error_ok(&decode_error)) {
        error->kind = CYCLONE_ENVELOPE_MALFORMED;
        error->malformed = decode_error;
        return false;
    }
    uint64_t fingerprint = 0;
    decode_error = cyclone_reader_read_u64(reader, &fingerprint);
    if (!cyclone_decode_error_ok(&decode_error)) {
        error->kind = CYCLONE_ENVELOPE_MALFORMED;
        error->malformed = decode_error;
        return false;
    }

    const CycloneMessage *message = cyclone_message(id);
    if (message == NULL) {
        error->kind = CYCLONE_ENVELOPE_UNKNOWN_MESSAGE;
        error->id = id;
        return false;
    }
    if (message->fingerprint != fingerprint) {
        error->kind = CYCLONE_ENVELOPE_FINGERPRINT_MISMATCH;
        error->id = id;
        error->expected = message->fingerprint;
        error->received = fingerprint;
        return false;
    }

    *out = message;
    return true;
}
";

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
            source: PathBuf::from("models.h"),
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
            text.contains("static const uint64_t CYCLONE_SCHEMA_FINGERPRINT = 0x"),
            "{text}"
        );
        assert!(
            text.contains("static const uint64_t PLAYER_FINGERPRINT = 0x"),
            "{text}"
        );
        assert!(
            text.contains("static const uint64_t ENEMY_FINGERPRINT = 0x"),
            "{text}"
        );
        assert!(
            text.contains("static const uint64_t PLAYER_EDGE_FINGERPRINT = 0x"),
            "{text}"
        );
        assert!(
            text.contains("static const uint32_t PLAYER_EDGE_MESSAGE_ID = 0x"),
            "{text}"
        );
        assert!(
            text.contains("static const CycloneMessage CYCLONE_MESSAGES[] = {"),
            "{text}"
        );
    }

    #[test]
    fn an_empty_schema_declares_no_messages_but_still_compiles() {
        let text = handshake_file(&Schema::build(&[]).expect("build"), false).expect("render");
        assert!(
            text.contains("static const CycloneMessage *const CYCLONE_MESSAGES = NULL;"),
            "{text}"
        );
        assert!(
            text.contains("static const size_t CYCLONE_MESSAGES_COUNT = 0;"),
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
            .filter(|line| line.trim_start().starts_with('{'))
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
        assert!(
            !off.contains("static inline bool cyclone_write_envelope"),
            "{off}"
        );

        let schema = Schema::build(&[model("Player", &["edge"])]).expect("build");
        let on = handshake_file(&schema, true).expect("render");
        assert!(
            on.contains("CYCLONE_VALIDATE_MESSAGE_FINGERPRINT = true;"),
            "{on}"
        );
        assert!(
            on.contains("static inline bool cyclone_write_envelope"),
            "{on}"
        );
        assert!(
            on.contains("static inline bool cyclone_read_envelope"),
            "{on}"
        );
        assert!(on.contains("#include \"runtime.h\"\n"), "{on}");
    }

    #[test]
    fn two_constants_spelled_the_same_are_reported_not_emitted() {
        let schema =
            Schema::build(&[model("Player", &["edge"]), model("PlayerEdge", &[])]).expect("build");
        let error = handshake_file(&schema, false).expect_err("collision");
        assert!(error.contains("PLAYER_EDGE_FINGERPRINT"), "{error}");
    }
}
