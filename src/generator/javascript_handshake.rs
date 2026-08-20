use std::collections::BTreeMap;

use crate::ir::Schema;
use crate::model::screaming_snake_case;
use crate::schema::hex64;

pub const FILE_NAME: &str = "handshake.js";

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

    if validate_message_fingerprint {
        out.push_str("import { Writer, Reader } from \"./runtime.js\";\n\n");
    }

    out.push_str(
        "/**\n * The fingerprint of the whole schema: every message, by name, with its own\n * \
         fingerprint, hashed together. Two peers that agree on this agree on everything.\n */\n",
    );
    out.push_str(&format!(
        "export const CYCLONE_SCHEMA_FINGERPRINT = {}n;\n\n",
        hex64(schema.fingerprint.u64())
    ));

    for model in &schema.models {
        out.push_str(&format!(
            "/** `{}`, as declared - every annotated field, whatever codec it joined. */\n",
            model.name
        ));
        out.push_str(&format!(
            "export const {}_FINGERPRINT = {}n;\n",
            screaming_snake_case(&model.name),
            hex64(model.fingerprint.u64())
        ));

        for message in &model.messages {
            out.push('\n');
            let constant = message_constant(&model.name, &message.codec);
            out.push_str(&format!(
                "/** `{}` - the wire contract `{}` encodes and decodes. */\n",
                message.name,
                super::codec_type_name(&model.name, &message.codec)
            ));
            out.push_str(&format!(
                "export const {constant}_MESSAGE_ID = 0x{:08X};\n",
                message.id
            ));
            out.push_str(&format!(
                "export const {constant}_FINGERPRINT = {}n;\n",
                hex64(message.fingerprint.u64())
            ));
            out.push_str(&format!(
                "/**\n                 * One fingerprint per prefix of `{}`: entry `k-1` covers its first `k`\n                 * fields. The last entry is `{constant}_FINGERPRINT`. Never sent whole -\n                 * a peer sends its field count and its last entry, and the two sides\n                 * compare at `min` of the two counts (RFC-0002 §9.1).\n                 */\n",
                message.name
            ));
            out.push_str(&format!("export const {constant}_PREFIXES = [\n"));
            for prefix in &message.prefixes {
                out.push_str(&format!("    {}n,\n", hex64(prefix.u64())));
            }
            out.push_str("];\n");
        }
        out.push('\n');
    }

    let mut messages: Vec<_> = schema.messages().collect();
    messages.sort_by_key(|message| message.id);

    out.push_str(
        "/**\n * One message: its id, its name, and the fingerprint of its wire contract.\n *\n \
         * @typedef {object} CycloneMessage\n * @property {number} id Stable across schema \
         changes; derived from the name alone.\n * @property {string} name `Model.codec`.\n * \
         @property {bigint} fingerprint Changes whenever the message's fields do.\n */\n\n",
    );
    out.push_str(
        "/** Every message this schema declares, sorted by id.\n * @type {CycloneMessage[]} */\n",
    );
    out.push_str("export const CYCLONE_MESSAGES = [\n");
    for message in &messages {
        let constant = message_constant(&message.model, &message.codec);
        out.push_str(&format!(
            "    {{ id: {constant}_MESSAGE_ID, name: \"{}\", fingerprint: {constant}_FINGERPRINT, \
             prefixes: {constant}_PREFIXES }},\n",
            message.name
        ));
    }
    out.push_str("];\n\n");

    out.push_str(HANDSHAKE);

    out.push_str(&format!(
        "\n/** Whether this schema was generated with `validate_message_fingerprint`. */\n\
         export const CYCLONE_VALIDATE_MESSAGE_FINGERPRINT = {validate_message_fingerprint};\n"
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

const HANDSHAKE: &str = "\
/** What a peer's fingerprints mean for this one. */
export const CycloneHandshake = Object.freeze({
    /** The same schema, exactly. */
    CURRENT: \"current\",
    /**
     * A different schema, but every message both ends know agrees on the
     * fields both ends carry. Safe to proceed.
     */
    OUTDATED: \"outdated\",
    /**
     * Both ends put different fields at an index both of them carry. There
     * is nothing to negotiate: disconnect.
     */
    REJECT: \"reject\",
    /**
     * Not decidable from the peer's table alone - at least one message needs
     * the extra exchange described on `CycloneMessageCheck.NEED_PREFIX`.
     */
    NEED_MORE: \"need-more\",
});

/** What one of the peer's messages means for this schema's message of the same id. */
export const CycloneMessageCheck = Object.freeze({
    /**
     * Either this schema does not declare the message at all, or the fields
     * both ends carry agree. Nothing to do.
     */
    MATCH: \"match\",
    /** Both ends put different fields at an index both of them carry. */
    REJECT: \"reject\",
    /**
     * Undecidable from what the peer sent: the peer has more fields than this
     * schema, so the answer lives at an index only the peer can produce. Ask
     * it for its prefix fingerprint at the reported field count, then compare
     * the reply against `cyclonePrefix` for the same id.
     */
    NEED_PREFIX: \"need-prefix\",
});

/**
 * The message with this id, if this schema declares it.
 * @param {number} id
 * @returns {CycloneMessage | undefined}
 */
export function cycloneMessage(id) {
    let low = 0;
    let high = CYCLONE_MESSAGES.length;
    while (low < high) {
        const middle = (low + high) >>> 1;
        if (CYCLONE_MESSAGES[middle].id < id) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    const candidate = CYCLONE_MESSAGES[low];
    return candidate !== undefined && candidate.id === id ? candidate : undefined;
}

/**
 * This schema's fingerprint for the first `fieldCount` fields of a message,
 * or `undefined` if it does not declare that message or does not have that
 * many fields. `fieldCount` counts from 1; 0 is the empty prefix and has no
 * fingerprint because it always matches.
 *
 * @param {number} id
 * @param {number} fieldCount
 * @returns {bigint | undefined}
 */
export function cyclonePrefix(id, fieldCount) {
    const message = cycloneMessage(id);
    if (message === undefined || fieldCount === 0) {
        return undefined;
    }
    return message.prefixes[fieldCount - 1];
}

/**
 * Compares one of the peer's messages against this schema's.
 *
 * `peerFieldCount` and `peerFingerprint` are what the peer declares for this
 * id. This is RFC-0002 §9.1's prefix test: the two are compatible when the
 * shorter field list is an exact prefix of the longer one, so the comparison
 * happens at `min(peerFieldCount, local field count)`. `askFor` is the field
 * count to ask the peer about, and is only meaningful for `NEED_PREFIX`.
 *
 * @param {number} id
 * @param {number} peerFieldCount
 * @param {bigint} peerFingerprint
 * @returns {{ check: string, askFor: number }}
 */
export function cycloneCheckMessage(id, peerFieldCount, peerFingerprint) {
    const known = cycloneMessage(id);
    if (known === undefined) {
        // Not a message this schema declares, so it is never exchanged.
        return { check: CycloneMessageCheck.MATCH, askFor: 0 };
    }
    const localFieldCount = known.prefixes.length;

    if (peerFingerprint === known.fingerprint) {
        return { check: CycloneMessageCheck.MATCH, askFor: 0 };
    }
    if (peerFieldCount === 0 || localFieldCount === 0) {
        // The empty field list is a prefix of everything.
        return { check: CycloneMessageCheck.MATCH, askFor: 0 };
    }
    if (peerFieldCount === localFieldCount) {
        // Same length, different content - a prefix of equal length would have
        // to be equality, and it is not.
        return { check: CycloneMessageCheck.REJECT, askFor: 0 };
    }
    if (peerFieldCount < localFieldCount) {
        // The peer's own fingerprint already is the value at the shared index.
        const local = known.prefixes[peerFieldCount - 1];
        return local === peerFingerprint
            ? { check: CycloneMessageCheck.MATCH, askFor: 0 }
            : { check: CycloneMessageCheck.REJECT, askFor: 0 };
    }
    return { check: CycloneMessageCheck.NEED_PREFIX, askFor: localFieldCount };
}

/**
 * Compares a peer's whole message table against this schema's.
 *
 * `peerMessages` is the peer's `(id, field count, fingerprint)` table - what
 * `CYCLONE_MESSAGES` is on its side. A `NEED_MORE` result means at least one
 * message needs the extra round; walk the table with `cycloneCheckMessage` to
 * find which ones.
 *
 * @param {bigint} peerSchemaFingerprint
 * @param {ReadonlyArray<readonly [number, number, bigint]>} peerMessages
 * @returns {string} one of `CycloneHandshake`'s values
 */
export function cycloneHandshake(peerSchemaFingerprint, peerMessages) {
    if (peerSchemaFingerprint === CYCLONE_SCHEMA_FINGERPRINT) {
        return CycloneHandshake.CURRENT;
    }

    let needMore = false;
    for (const [id, fieldCount, fingerprint] of peerMessages) {
        const { check } = cycloneCheckMessage(id, fieldCount, fingerprint);
        if (check === CycloneMessageCheck.REJECT) {
            // One mismatch decides the whole session. Every other message could
            // agree and it would still be unsafe to speak.
            return CycloneHandshake.REJECT;
        }
        if (check === CycloneMessageCheck.NEED_PREFIX) {
            needMore = true;
        }
    }

    return needMore ? CycloneHandshake.NEED_MORE : CycloneHandshake.OUTDATED;
}
";

const ENVELOPE: &str = "\
// ==========================================================================
// Per-frame validation - validate_message_fingerprint = true.
//
//     [MessageId: u32][MessageFingerprint: u64][Payload]
//
// Twelve bytes in front of every message, so that a peer that got past the
// handshake still cannot decode one message as another. Off by default: the
// wire format's premise is that there is no metadata on it, and the handshake
// already answers this question once per connection instead of once per frame.
// ==========================================================================

/** A frame whose envelope did not describe a message this schema can decode. */
export class CycloneEnvelopeError extends Error {
    constructor(message) {
        super(message);
        this.name = \"CycloneEnvelopeError\";
    }

    /** An id this schema does not declare.
     * @param {number} id
     * @returns {CycloneEnvelopeError} */
    static unknownMessage(id) {
        return new CycloneEnvelopeError(
            `cyclone: unknown message id 0x${id.toString(16).padStart(8, \"0\")}`,
        );
    }

    /** The right message, the wrong shape.
     * @param {number} id
     * @param {bigint} expected
     * @param {bigint} received
     * @returns {CycloneEnvelopeError} */
    static fingerprintMismatch(id, expected, received) {
        return new CycloneEnvelopeError(
            `cyclone: message 0x${id.toString(16).padStart(8, \"0\")}: peer fingerprint ` +
                `0x${received.toString(16).padStart(16, \"0\")}, ours 0x${expected
                    .toString(16)
                    .padStart(16, \"0\")}`,
        );
    }
}

/** Writes `[MessageId][MessageFingerprint]`, immediately before the payload.
 * @param {Writer} writer
 * @param {CycloneMessage} message */
export function cycloneWriteEnvelope(writer, message) {
    writer.writeU32(message.id);
    writer.writeU64(message.fingerprint);
}

/**
 * Reads an envelope and resolves it against this schema, leaving the reader
 * positioned at the payload.
 * @param {Reader} reader
 * @returns {CycloneMessage}
 */
export function cycloneReadEnvelope(reader) {
    const id = reader.readU32();
    const fingerprint = reader.readU64();

    const message = cycloneMessage(id);
    if (message === undefined) {
        throw CycloneEnvelopeError.unknownMessage(id);
    }
    if (message.fingerprint !== fingerprint) {
        throw CycloneEnvelopeError.fingerprintMismatch(id, message.fingerprint, fingerprint);
    }

    return message;
}
";

const ENVELOPE_OFF: &str = "\
// Per-frame message validation is off, so no envelope is generated and no
// frame carries one. Turn it on in cyclone.toml:
//
//     validate_message_fingerprint = true
//
// and every frame gains [MessageId: u32][MessageFingerprint: u64] in front of
// its payload, with cycloneWriteEnvelope / cycloneReadEnvelope to match.
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
            source: PathBuf::from("src/models.js"),
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
            text.contains("export const CYCLONE_SCHEMA_FINGERPRINT = 0x"),
            "{text}"
        );
        assert!(
            text.contains("export const PLAYER_FINGERPRINT = 0x"),
            "{text}"
        );
        assert!(
            text.contains("export const ENEMY_FINGERPRINT = 0x"),
            "{text}"
        );
        assert!(
            text.contains("export const PLAYER_EDGE_FINGERPRINT = 0x"),
            "{text}"
        );
        assert!(
            text.contains("export const PLAYER_EDGE_MESSAGE_ID = 0x"),
            "{text}"
        );
        assert!(text.contains("export const CYCLONE_MESSAGES = ["), "{text}");
    }

    #[test]
    fn the_envelope_is_off_unless_asked_for() {
        let off = generated(&[model("Player", &["edge"])]);
        assert!(
            off.contains("CYCLONE_VALIDATE_MESSAGE_FINGERPRINT = false"),
            "{off}"
        );
        assert!(!off.contains("function cycloneWriteEnvelope"), "{off}");

        let schema = Schema::build(&[model("Player", &["edge"])]).expect("build");
        let on = handshake_file(&schema, true).expect("render");
        assert!(
            on.contains("CYCLONE_VALIDATE_MESSAGE_FINGERPRINT = true"),
            "{on}"
        );
        assert!(on.contains("function cycloneWriteEnvelope"), "{on}");
        assert!(on.contains("function cycloneReadEnvelope"), "{on}");
    }

    #[test]
    fn no_enum_keyword_the_handshake_is_a_frozen_object() {
        let text = generated(&[model("Player", &["edge"])]);
        assert!(text.contains("Object.freeze({"), "{text}");
        assert!(!text.contains("enum "), "{text}");
    }

    #[test]
    fn two_constants_spelled_the_same_are_reported_not_emitted() {
        let schema =
            Schema::build(&[model("Player", &["edge"]), model("PlayerEdge", &[])]).expect("build");
        let error = handshake_file(&schema, false).expect_err("collision");
        assert!(error.contains("PLAYER_EDGE_FINGERPRINT"), "{error}");
    }
}
