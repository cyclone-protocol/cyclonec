use std::collections::BTreeMap;

use crate::ir::Schema;
use crate::model::screaming_snake_case;

use super::gdscript::{u32_literal, u64_literal};

pub const FILE_NAME: &str = "handshake.gd";

pub fn handshake_file(
    schema: &Schema,
    validate_message_fingerprint: bool,
) -> Result<String, String> {
    check_constant_names(schema)?;

    let mut out = super::gdscript::Header {
        fingerprint: Some(schema.fingerprint.tagged()),
        note: Some(
            "Every fingerprint this schema publishes, and the handshake that compares\n\
             them. Generated - never edit, and never hand-maintain a copy of these\n\
             values anywhere else.",
        ),
        ..super::gdscript::Header::default()
    }
    .render();
    out.push_str("class_name CycloneHandshake\n\n");

    out.push_str(
        "# The fingerprint of the whole schema: every message, by name, with its own\n\
         # fingerprint, hashed together. Two peers that agree on this agree on everything.\n",
    );
    out.push_str(&format!(
        "const SCHEMA_FINGERPRINT: int = {}\n\n",
        u64_literal(schema.fingerprint.u64())
    ));

    for model in &schema.models {
        let model_constant = screaming_snake_case(&model.name);
        out.push_str(&format!(
            "# {}, as declared - every annotated field, whatever codec it joined.\n",
            model.name
        ));
        out.push_str(&format!(
            "const {model_constant}_FINGERPRINT: int = {}\n",
            u64_literal(model.fingerprint.u64())
        ));

        for message in &model.messages {
            out.push('\n');
            let constant = message_constant(&model.name, &message.codec);
            out.push_str(&format!(
                "# {} - the wire contract {} encodes and decodes.\n",
                message.name,
                super::codec_type_name(&model.name, &message.codec),
            ));
            out.push_str(&format!(
                "const {constant}_MESSAGE_ID: int = {}\n",
                u32_literal(message.id)
            ));
            out.push_str(&format!(
                "const {constant}_FINGERPRINT: int = {}\n",
                u64_literal(message.fingerprint.u64())
            ));
            out.push_str(&format!(
                "# One fingerprint per prefix of {}: entry k-1 covers its first k fields.\n\
                 # The last entry is {constant}_FINGERPRINT. Never sent whole - a peer sends\n\
                 # its field count and its last entry, and the two sides compare at min of\n\
                 # the two counts (RFC-0002 9.1).\n",
                message.name
            ));
            out.push_str(&format!("const {constant}_PREFIXES: Array = [\n"));
            for prefix in &message.prefixes {
                out.push_str(&format!("\t{},\n", u64_literal(prefix.u64())));
            }
            out.push_str("]\n");
        }
        out.push('\n');
    }

    out.push_str(TYPES);

    let mut messages: Vec<_> = schema.messages().collect();
    messages.sort_by_key(|message| message.id);

    out.push_str("# Every message this schema declares, sorted by id.\n");
    out.push_str("static var MESSAGES: Array = [\n");
    for message in &messages {
        let constant = message_constant(&message.model, &message.codec);
        out.push_str(&format!(
            "\tCycloneMessage.new({constant}_MESSAGE_ID, {:?}, {constant}_FINGERPRINT, \
             {constant}_PREFIXES),\n",
            message.name
        ));
    }
    out.push_str("]\n");

    out.push_str(HANDSHAKE);

    out.push_str(&format!(
        "\n# Whether this schema was generated with validate_message_fingerprint.\n\
         const VALIDATE_MESSAGE_FINGERPRINT: bool = {validate_message_fingerprint}\n\n"
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
# One message: its id, its name, and the fingerprint of its wire contract.
class CycloneMessage:
\tvar id: int
\tvar name: String
\tvar fingerprint: int
\t# One entry per field: entry k-1 covers the first k fields. The last entry
\t# is fingerprint. Stays local; only its size and its last entry ever go on
\t# the wire.
\tvar prefixes: Array

\tfunc _init(message_id: int, message_name: String, message_fingerprint: int, message_prefixes: Array) -> void:
\t\tid = message_id
\t\tname = message_name
\t\tfingerprint = message_fingerprint
\t\tprefixes = message_prefixes

# One entry of a peer's (id, field count, fingerprint) table - what MESSAGES is
# on its side.
class CyclonePeerMessage:
\tvar id: int
\tvar field_count: int
\tvar fingerprint: int

\tfunc _init(message_id: int, message_field_count: int, message_fingerprint: int) -> void:
\t\tid = message_id
\t\tfield_count = message_field_count
\t\tfingerprint = message_fingerprint

# What a peer's fingerprints mean for this one.
enum Verdict {
\t# The same schema, exactly.
\tCURRENT,
\t# A different schema, but every message both ends know agrees on the fields
\t# both ends carry. Safe to proceed.
\tOUTDATED,
\t# Both ends put different fields at an index both of them carry. There is
\t# nothing to negotiate: disconnect.
\tREJECT,
\t# Not decidable from the peer's table alone - at least one message needs the
\t# extra exchange described on MessageCheck.NEED_PREFIX.
\tNEED_MORE,
}

# What one of the peer's messages means for this schema's message of the same id.
enum MessageCheck {
\t# Either this schema does not declare the message at all, or the fields both
\t# ends carry agree. Nothing to do.
\tMATCH,
\t# Both ends put different fields at an index both of them carry.
\tREJECT,
\t# Undecidable from what the peer sent: the peer has more fields than this
\t# schema, so the answer lives at an index only the peer can produce. Ask it
\t# for its prefix fingerprint at the reported field count, then compare the
\t# reply against prefix() for the same id.
\tNEED_PREFIX,
}

";

const HANDSHAKE: &str = r####"
# The message with this id, if this schema declares it - null otherwise.
static func message_by_id(id: int) -> CycloneMessage:
	var low := 0
	var high := MESSAGES.size()
	while low < high:
		var middle := (low + high) / 2
		if MESSAGES[middle].id < id:
			low = middle + 1
		else:
			high = middle
	if low < MESSAGES.size() and MESSAGES[low].id == id:
		return MESSAGES[low]
	return null

# This schema's fingerprint for the first field_count fields of a message, or
# -1 if it does not declare that message or does not have that many fields.
# field_count counts from 1; 0 is the empty prefix and has no fingerprint
# because it always matches.
static func prefix(id: int, field_count: int) -> int:
	var message := message_by_id(id)
	if message == null or field_count <= 0 or field_count > message.prefixes.size():
		return -1
	return message.prefixes[field_count - 1]

# Compares one of the peer's messages against this schema's. Returns
# [check, ask_for]; ask_for is the field count to ask the peer about, and is
# only meaningful for MessageCheck.NEED_PREFIX.
#
# This is RFC-0002 9.1's prefix test: the two are compatible when the shorter
# field list is an exact prefix of the longer one, so the comparison happens at
# the smaller of the two field counts.
static func check_message(id: int, peer_field_count: int, peer_fingerprint: int) -> Array:
	var known := message_by_id(id)
	if known == null:
		# Not a message this schema declares, so it is never exchanged.
		return [MessageCheck.MATCH, 0]
	var local_field_count := known.prefixes.size()

	if peer_fingerprint == known.fingerprint:
		return [MessageCheck.MATCH, 0]
	if peer_field_count == 0 or local_field_count == 0:
		# The empty field list is a prefix of everything.
		return [MessageCheck.MATCH, 0]
	if peer_field_count == local_field_count:
		# Same length, different content - a prefix of equal length would have
		# to be equality, and it is not.
		return [MessageCheck.REJECT, 0]
	if peer_field_count < local_field_count:
		# The peer's own fingerprint already is the value at the shared index.
		if known.prefixes[peer_field_count - 1] == peer_fingerprint:
			return [MessageCheck.MATCH, 0]
		return [MessageCheck.REJECT, 0]
	return [MessageCheck.NEED_PREFIX, local_field_count]

# Compares a peer's whole message table against this schema's. A NEED_MORE
# result means at least one message needs the extra round; walk the table with
# check_message() to find which ones.
static func compare(peer_schema_fingerprint: int, peer_messages: Array) -> Verdict:
	if peer_schema_fingerprint == SCHEMA_FINGERPRINT:
		return Verdict.CURRENT

	var need_more := false
	for peer in peer_messages:
		var outcome := check_message(peer.id, peer.field_count, peer.fingerprint)
		if outcome[0] == MessageCheck.REJECT:
			# One mismatch decides the whole session. Every other message could
			# agree and it would still be unsafe to speak.
			return Verdict.REJECT
		if outcome[0] == MessageCheck.NEED_PREFIX:
			need_more = true

	if need_more:
		return Verdict.NEED_MORE
	return Verdict.OUTDATED
"####;

const ENVELOPE: &str = r####"# ==========================================================================
# Per-frame validation - validate_message_fingerprint = true.
#
#     [MessageId: u32][MessageFingerprint: u64][Payload]
#
# Twelve bytes in front of every message, so that a peer that got past the
# handshake still cannot decode one message as another. Off by default: the
# wire format's premise is that there is no metadata on it, and the handshake
# already answers this question once per connection instead of once per frame.
# ==========================================================================

# A frame whose envelope did not describe a message this schema can decode.
# GDScript has no exceptions, so - like CycloneRuntime.DecodeError - this is
# returned, never thrown.
class EnvelopeError:
	var kind: String = ""
	var message_id: int = 0
	var expected_fingerprint: int = 0
	var received_fingerprint: int = 0

	func message() -> String:
		match kind:
			"unknown_message":
				return "unknown message id 0x%08X" % message_id
			"fingerprint_mismatch":
				return "message 0x%08X: peer fingerprint 0x%016X, ours 0x%016X" % [message_id, received_fingerprint, expected_fingerprint]
			_:
				return "cyclone: envelope error"

# Writes [MessageId][MessageFingerprint], immediately before the payload.
static func write_envelope(writer: CycloneRuntime.Writer, message: CycloneMessage) -> void:
	writer.write_u32(message.id)
	writer.write_u64(message.fingerprint)

# Reads an envelope and resolves it against this schema. Returns
# [CycloneMessage, error], with the reader left positioned at the payload
# only when error is null.
static func read_envelope(reader: CycloneRuntime.Reader) -> Array:
	var id_result := reader.read_u32()
	if id_result[1] != null:
		return [null, id_result[1]]
	var fingerprint_result := reader.read_u64()
	if fingerprint_result[1] != null:
		return [null, fingerprint_result[1]]
	var id: int = id_result[0]
	var fingerprint: int = fingerprint_result[0]

	var known := message_by_id(id)
	if known == null:
		var error := EnvelopeError.new()
		error.kind = "unknown_message"
		error.message_id = id
		return [null, error]
	if known.fingerprint != fingerprint:
		var error := EnvelopeError.new()
		error.kind = "fingerprint_mismatch"
		error.message_id = id
		error.expected_fingerprint = known.fingerprint
		error.received_fingerprint = fingerprint
		return [null, error]
	return [known, null]
"####;

const ENVELOPE_OFF: &str = "\
# Per-frame message validation is off, so no envelope is generated and no
# frame carries one. Turn it on in cyclone.toml:
#
#     validate_message_fingerprint = true
#
# and every frame gains [MessageId: u32][MessageFingerprint: u64] in front of
# its payload, with write_envelope / read_envelope to match.
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
            source: PathBuf::from("models.gd"),
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

        assert!(text.contains("class_name CycloneHandshake\n"), "{text}");
        assert!(text.contains("const SCHEMA_FINGERPRINT: int ="), "{text}");
        assert!(text.contains("const PLAYER_FINGERPRINT: int ="), "{text}");
        assert!(text.contains("const ENEMY_FINGERPRINT: int ="), "{text}");
        assert!(
            text.contains("const PLAYER_EDGE_FINGERPRINT: int ="),
            "{text}"
        );
        assert!(
            text.contains("const PLAYER_EDGE_MESSAGE_ID: int ="),
            "{text}"
        );
        assert!(text.contains("static var MESSAGES: Array = ["), "{text}");
    }

    #[test]
    fn the_message_table_is_sorted_by_id_so_lookup_can_bisect() {
        let schema = Schema::build(&[
            model("Player", &["edge", "unity"]),
            model("Enemy", &["edge"]),
        ])
        .expect("build");
        let text = handshake_file(&schema, false).expect("render");

        let lines: Vec<&str> = text
            .lines()
            .filter(|line| line.trim_start().starts_with("CycloneMessage.new("))
            .collect();
        assert_eq!(lines.len(), 3, "{text}");

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
            off.contains("VALIDATE_MESSAGE_FINGERPRINT: bool = false"),
            "{off}"
        );
        assert!(!off.contains("static func write_envelope"), "{off}");

        let schema = Schema::build(&[model("Player", &["edge"])]).expect("build");
        let on = handshake_file(&schema, true).expect("render");
        assert!(
            on.contains("VALIDATE_MESSAGE_FINGERPRINT: bool = true"),
            "{on}"
        );
        assert!(on.contains("static func write_envelope"), "{on}");
        assert!(on.contains("static func read_envelope"), "{on}");
    }

    #[test]
    fn two_constants_spelled_the_same_are_reported_not_emitted() {
        let schema =
            Schema::build(&[model("Player", &["edge"]), model("PlayerEdge", &[])]).expect("build");
        let error = handshake_file(&schema, false).expect_err("collision");
        assert!(error.contains("PLAYER_EDGE_FINGERPRINT"), "{error}");
    }
}
