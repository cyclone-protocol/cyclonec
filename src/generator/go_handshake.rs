use std::collections::BTreeMap;

use crate::ir::Schema;

pub const FILE_NAME: &str = "handshake.go";

pub fn handshake_file(
    schema: &Schema,
    package: &str,
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
    out.push_str(&format!("package {package}\n\n"));

    if validate_message_fingerprint {
        out.push_str("import \"fmt\"\n\n");
    }

    out.push_str(
        "// CycloneSchemaFingerprint is the fingerprint of the whole schema: every\n\
         // message, by name, with its own fingerprint, hashed together. Two peers that\n\
         // agree on this agree on everything.\n",
    );
    out.push_str(&format!(
        "const CycloneSchemaFingerprint uint64 = {}\n\n",
        crate::schema::hex64(schema.fingerprint.u64())
    ));

    for model in &schema.models {
        out.push_str(&format!(
            "// {}Fingerprint is `{}`, as declared - every annotated field, whatever codec\n\
             // it joined.\n",
            model.name, model.name
        ));
        out.push_str(&format!(
            "const {}Fingerprint uint64 = {}\n",
            model.name,
            crate::schema::hex64(model.fingerprint.u64())
        ));

        for message in &model.messages {
            out.push('\n');
            let constant = message_constant(&model.name, &message.codec);
            out.push_str(&format!(
                "// {constant}MessageID and {constant}Fingerprint are `{}` - the wire\n\
                 // contract `{}` encodes and decodes.\n",
                message.name,
                super::codec_type_name(&model.name, &message.codec),
            ));
            out.push_str(&format!(
                "const {constant}MessageID uint32 = 0x{:08X}\n",
                message.id
            ));
            out.push_str(&format!(
                "const {constant}Fingerprint uint64 = {}\n",
                crate::schema::hex64(message.fingerprint.u64())
            ));
            out.push_str(&format!(
                "\n// {constant}Prefixes holds one fingerprint per prefix of {}: entry k-1\n\
                 // covers its first k fields. The last entry is {constant}Fingerprint.\n\
                 // Never sent whole - a peer sends its field count and its last entry, and\n\
                 // the two sides compare at min of the two counts (RFC-0002 9.1).\n",
                message.name
            ));
            out.push_str(&format!("var {constant}Prefixes = []uint64{{\n"));
            for prefix in &message.prefixes {
                out.push_str(&format!("\t{},\n", crate::schema::hex64(prefix.u64())));
            }
            out.push_str("}\n");
        }
        out.push('\n');
    }

    out.push_str(TYPES);

    let mut messages: Vec<_> = schema.messages().collect();
    messages.sort_by_key(|message| message.id);

    out.push_str(
        "// CycloneMessages is every message this schema declares, sorted by id.\n\
         var CycloneMessages = []CycloneMessage{\n",
    );
    for message in &messages {
        let constant = message_constant(&message.model, &message.codec);
        out.push_str(&format!(
            "\t{{ID: {constant}MessageID, Name: {:?}, Fingerprint: {constant}Fingerprint, \
             Prefixes: {constant}Prefixes}},\n",
            message.name
        ));
    }
    out.push_str("}\n");

    out.push_str(HANDSHAKE);

    out.push_str(&format!(
        "\n// CycloneValidateMessageFingerprint is whether this schema was generated with\n\
         // validate_message_fingerprint.\n\
         const CycloneValidateMessageFingerprint = {validate_message_fingerprint}\n"
    ));
    out.push('\n');
    if validate_message_fingerprint {
        out.push_str(ENVELOPE);
    } else {
        out.push_str(ENVELOPE_OFF);
    }

    Ok(out)
}

fn message_constant(model: &str, codec: &str) -> String {
    format!("{model}{}", crate::model::pascal_case(codec))
}

fn check_constant_names(schema: &Schema) -> Result<(), String> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();

    for model in &schema.models {
        let name = format!("{}Fingerprint", model.name);
        seen.insert(name, model.name.clone());
    }
    for message in schema.messages() {
        let name = format!(
            "{}Fingerprint",
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
// CycloneMessage is one message: its id, its name, and the fingerprint of its
// wire contract.
type CycloneMessage struct {
	// ID is stable across schema changes; derived from the name alone.
	ID uint32
	// Name is `Model.codec`.
	Name string
	// Fingerprint changes whenever the message's fields do.
	Fingerprint uint64
	// Prefixes holds one entry per field: entry k-1 covers the first k fields.
	// The last entry is Fingerprint. Stays local; only its length and its last
	// entry ever go on the wire.
	Prefixes []uint64
}

";

const HANDSHAKE: &str = r####"
// CycloneHandshake is what a peer's fingerprints mean for this one.
type CycloneHandshake int

const (
	// CycloneCurrent means the same schema, exactly.
	CycloneCurrent CycloneHandshake = iota
	// CycloneOutdated means a different schema, but every message both ends
	// know agrees on the fields both ends carry. Safe to proceed.
	CycloneOutdated
	// CycloneReject means both ends put different fields at an index both of
	// them carry. There is nothing to negotiate: disconnect.
	CycloneReject
	// CycloneNeedMore means the peer's table alone does not decide it - at
	// least one message needs the extra exchange described on
	// CycloneNeedPrefix.
	CycloneNeedMore
)

// CycloneMessageCheck is what one of the peer's messages means for this
// schema's message of the same id.
type CycloneMessageCheck int

const (
	// CycloneMatch means either this schema does not declare the message at
	// all, or the fields both ends carry agree.
	CycloneMatch CycloneMessageCheck = iota
	// CycloneMessageReject means both ends put different fields at an index
	// both of them carry.
	CycloneMessageReject
	// CycloneNeedPrefix means undecidable from what the peer sent: the peer has
	// more fields than this schema, so the answer lives at an index only the
	// peer can produce. Ask it for its prefix fingerprint at the returned field
	// count, then compare the reply against CyclonePrefix for the same id.
	CycloneNeedPrefix
)

// CyclonePeerMessage is one entry of a peer's (id, field count, fingerprint)
// table - what CycloneMessages is on its side.
type CyclonePeerMessage struct {
	ID          uint32
	FieldCount  uint32
	Fingerprint uint64
}

// CycloneMessageByID returns the message with this id, and whether this
// schema declares it.
func CycloneMessageByID(id uint32) (CycloneMessage, bool) {
	low, high := 0, len(CycloneMessages)
	for low < high {
		middle := (low + high) / 2
		if CycloneMessages[middle].ID < id {
			low = middle + 1
		} else {
			high = middle
		}
	}
	if low < len(CycloneMessages) && CycloneMessages[low].ID == id {
		return CycloneMessages[low], true
	}
	return CycloneMessage{}, false
}

// CyclonePrefix returns this schema's fingerprint for the first fieldCount
// fields of a message, and whether it exists. fieldCount counts from 1; 0 is
// the empty prefix and has no fingerprint because it always matches.
func CyclonePrefix(id uint32, fieldCount uint32) (uint64, bool) {
	message, ok := CycloneMessageByID(id)
	if !ok || fieldCount == 0 || int(fieldCount) > len(message.Prefixes) {
		return 0, false
	}
	return message.Prefixes[fieldCount-1], true
}

// CycloneCheckMessage compares one of the peer's messages against this
// schema's. peerFieldCount and peerFingerprint are what the peer declares for
// this id. This is RFC-0002 9.1's prefix test: the two are compatible when the
// shorter field list is an exact prefix of the longer one, so the comparison
// happens at min(peerFieldCount, local field count). The second return value
// is the field count to ask the peer about, and is only meaningful for
// CycloneNeedPrefix.
func CycloneCheckMessage(id uint32, peerFieldCount uint32, peerFingerprint uint64) (CycloneMessageCheck, uint32) {
	known, ok := CycloneMessageByID(id)
	if !ok {
		// Not a message this schema declares, so it is never exchanged.
		return CycloneMatch, 0
	}
	localFieldCount := uint32(len(known.Prefixes))

	if peerFingerprint == known.Fingerprint {
		return CycloneMatch, 0
	}
	if peerFieldCount == 0 || localFieldCount == 0 {
		// The empty field list is a prefix of everything.
		return CycloneMatch, 0
	}
	if peerFieldCount == localFieldCount {
		// Same length, different content - a prefix of equal length would have
		// to be equality, and it is not.
		return CycloneMessageReject, 0
	}
	if peerFieldCount < localFieldCount {
		// The peer's own fingerprint already is the value at the shared index.
		if known.Prefixes[peerFieldCount-1] == peerFingerprint {
			return CycloneMatch, 0
		}
		return CycloneMessageReject, 0
	}
	return CycloneNeedPrefix, localFieldCount
}

// CycloneHandshakeCompare compares a peer's whole message table against this
// schema's. A CycloneNeedMore result means at least one message needs the
// extra round; walk the table with CycloneCheckMessage to find which ones.
func CycloneHandshakeCompare(peerSchemaFingerprint uint64, peerMessages []CyclonePeerMessage) CycloneHandshake {
	if peerSchemaFingerprint == CycloneSchemaFingerprint {
		return CycloneCurrent
	}

	needMore := false
	for _, peer := range peerMessages {
		switch check, _ := CycloneCheckMessage(peer.ID, peer.FieldCount, peer.Fingerprint); check {
		case CycloneMessageReject:
			// One mismatch decides the whole session. Every other message could
			// agree and it would still be unsafe to speak.
			return CycloneReject
		case CycloneNeedPrefix:
			needMore = true
		}
	}

	if needMore {
		return CycloneNeedMore
	}
	return CycloneOutdated
}
"####;

const ENVELOPE: &str = r####"// ==========================================================================
// Per-frame validation - validate_message_fingerprint = true.
//
//     [MessageId: uint32][MessageFingerprint: uint64][Payload]
//
// Twelve bytes in front of every message, so that a peer that got past the
// handshake still cannot decode one message as another. Off by default: the
// wire format's premise is that there is no metadata on it, and the handshake
// already answers this question once per connection instead of once per frame.
// ==========================================================================

// CycloneWriteEnvelope writes [MessageId][MessageFingerprint], immediately
// before the payload.
func CycloneWriteEnvelope(w *Writer, message CycloneMessage) {
	w.WriteU32(message.ID)
	w.WriteU64(message.Fingerprint)
}

// CycloneReadEnvelope reads an envelope and resolves it against this schema,
// leaving the reader positioned at the payload.
func CycloneReadEnvelope(r *Reader) (CycloneMessage, error) {
	id, err := r.ReadU32()
	if err != nil {
		return CycloneMessage{}, err
	}
	fingerprint, err := r.ReadU64()
	if err != nil {
		return CycloneMessage{}, err
	}

	message, ok := CycloneMessageByID(id)
	if !ok {
		return CycloneMessage{}, fmt.Errorf("cyclone: unknown message id 0x%08X", id)
	}
	if message.Fingerprint != fingerprint {
		return CycloneMessage{}, fmt.Errorf(
			"cyclone: message 0x%08X: peer fingerprint 0x%016X, ours 0x%016X",
			id, fingerprint, message.Fingerprint,
		)
	}

	return message, nil
}
"####;

const ENVELOPE_OFF: &str = "\
// Per-frame message validation is off, so no envelope is generated and no
// frame carries one. Turn it on in cyclone.toml:
//
//     validate_message_fingerprint = true
//
// and every frame gains [MessageId: uint32][MessageFingerprint: uint64] in
// front of its payload, with CycloneWriteEnvelope / CycloneReadEnvelope to
// match.
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
            source: PathBuf::from("models.go"),
            line: 1,
            codecs: codecs.iter().map(|codec| (*codec).to_owned()).collect(),
            fields: vec![Field {
                name: "ID".to_owned(),
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

        assert!(text.contains("package generated\n"), "{text}");
        assert!(
            text.contains("const CycloneSchemaFingerprint uint64 = 0x"),
            "{text}"
        );
        assert!(
            text.contains("const PlayerFingerprint uint64 = 0x"),
            "{text}"
        );
        assert!(
            text.contains("const EnemyFingerprint uint64 = 0x"),
            "{text}"
        );
        assert!(
            text.contains("const PlayerEdgeFingerprint uint64 = 0x"),
            "{text}"
        );
        assert!(
            text.contains("const PlayerEdgeMessageID uint32 = 0x"),
            "{text}"
        );
        assert!(
            text.contains("var CycloneMessages = []CycloneMessage{"),
            "{text}"
        );
    }

    #[test]
    fn the_message_table_is_sorted_by_id_so_lookup_can_bisect() {
        let schema = Schema::build(&[
            model("Player", &["edge", "unity"]),
            model("Enemy", &["edge"]),
        ])
        .expect("build");
        let text = handshake_file(&schema, "generated", false).expect("render");

        let lines: Vec<&str> = text
            .lines()
            .filter(|line| line.trim_start().starts_with("{ID:"))
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
            off.contains("CycloneValidateMessageFingerprint = false"),
            "{off}"
        );
        assert!(!off.contains("func CycloneWriteEnvelope"), "{off}");

        let schema = Schema::build(&[model("Player", &["edge"])]).expect("build");
        let on = handshake_file(&schema, "generated", true).expect("render");
        assert!(
            on.contains("CycloneValidateMessageFingerprint = true"),
            "{on}"
        );
        assert!(on.contains("func CycloneWriteEnvelope"), "{on}");
        assert!(on.contains("func CycloneReadEnvelope"), "{on}");
    }

    #[test]
    fn two_constants_spelled_the_same_are_reported_not_emitted() {
        let schema =
            Schema::build(&[model("Player", &["edge"]), model("PlayerEdge", &[])]).expect("build");
        let error = handshake_file(&schema, "generated", false).expect_err("collision");
        assert!(error.contains("PlayerEdgeFingerprint"), "{error}");
    }
}
