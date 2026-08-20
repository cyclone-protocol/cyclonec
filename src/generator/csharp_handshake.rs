use std::collections::BTreeMap;

use crate::ir::Schema;
use crate::model::pascal_case;
use crate::schema::hex64;

pub const FILE_NAME: &str = "Handshake.cs";

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
    out.push_str(&format!("namespace {namespace}\n{{\n\n"));

    out.push_str(TYPES);

    out.push_str("/// <summary>One message: its id, its name, and the fingerprint of its wire\n");
    out.push_str("/// contract.</summary>\n");
    out.push_str("public static class Handshake\n{\n");

    out.push_str(
        "    /// <summary>The fingerprint of the whole schema: every message, by name, with \
         its own\n    /// fingerprint, hashed together. Two peers that agree on this agree on\n\
         \x20\x20\x20\x20/// everything.</summary>\n",
    );
    out.push_str(&format!(
        "    public const ulong CycloneSchemaFingerprint = {};\n\n",
        hex64(schema.fingerprint.u64())
    ));

    for model in &schema.models {
        out.push_str(&format!(
            "    /// <summary><c>{}</c>, as declared - every annotated field, whatever codec \
             it joined.</summary>\n",
            model.name
        ));
        out.push_str(&format!(
            "    public const ulong {}Fingerprint = {};\n",
            model.name,
            hex64(model.fingerprint.u64())
        ));

        for message in &model.messages {
            out.push('\n');
            let constant = message_constant(&model.name, &message.codec);
            out.push_str(&format!(
                "    /// <summary><c>{}</c> - the wire contract <c>{}</c> encodes and \
                 decodes.</summary>\n",
                message.name,
                super::codec_type_name(&model.name, &message.codec),
            ));
            out.push_str(&format!(
                "    public const uint {constant}MessageId = 0x{:08X};\n",
                message.id
            ));
            out.push_str(&format!(
                "    public const ulong {constant}Fingerprint = {};\n",
                hex64(message.fingerprint.u64())
            ));
            out.push_str(&format!(
                "    /// <summary>One fingerprint per prefix of <c>{}</c>: entry <c>k-1</c>\n\
                 \x20   /// covers its first <c>k</c> fields. The last entry is\n\
                 \x20   /// <see cref=\"{constant}Fingerprint\"/>. Never sent whole - a peer\n\
                 \x20   /// sends its field count and its last entry, and the two sides\n\
                 \x20   /// compare at <c>min</c> of the two counts (RFC-0002 9.1).</summary>\n",
                message.name
            ));
            out.push_str(&format!(
                "    public static readonly ulong[] {constant}Prefixes = new ulong[]\n    {{\n"
            ));
            for prefix in &message.prefixes {
                out.push_str(&format!("        {},\n", hex64(prefix.u64())));
            }
            out.push_str("    };\n");
        }
        out.push('\n');
    }

    let mut messages: Vec<_> = schema.messages().collect();
    messages.sort_by_key(|message| message.id);

    out.push_str(
        "    /// <summary>Every message this schema declares, sorted by id.</summary>\n\
         \x20\x20\x20\x20public static readonly CycloneMessage[] CycloneMessages = new CycloneMessage[]\n    {\n",
    );
    for message in &messages {
        let constant = message_constant(&message.model, &message.codec);
        out.push_str(&format!(
            "        new CycloneMessage({constant}MessageId, \"{}\", {constant}Fingerprint, \
             {constant}Prefixes),\n",
            message.name
        ));
    }
    out.push_str("    };\n");

    out.push_str(HANDSHAKE);

    out.push_str(&format!(
        "\n    /// <summary>Whether this schema was generated with \
         validate_message_fingerprint.</summary>\n    \
         public const bool CycloneValidateMessageFingerprint = {};\n",
        if validate_message_fingerprint {
            "true"
        } else {
            "false"
        }
    ));
    if validate_message_fingerprint {
        out.push_str(ENVELOPE);
    } else {
        out.push_str(ENVELOPE_OFF);
    }

    out.push_str("}\n\n}\n");

    Ok(out)
}

fn message_constant(model: &str, codec: &str) -> String {
    format!("{model}{}", pascal_case(codec))
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

const TYPES: &str = r####"/// <summary>One message: its id, its name, and the fingerprint of its wire
/// contract.</summary>
public readonly struct CycloneMessage
{
    /// <summary>Stable across schema changes; derived from the name alone.</summary>
    public readonly uint Id;
    /// <summary><c>Model.codec</c>.</summary>
    public readonly string Name;
    /// <summary>Changes whenever the message's fields do.</summary>
    public readonly ulong Fingerprint;
    /// <summary>One entry per field: entry <c>k-1</c> covers the first <c>k</c> fields.
    /// The last entry is <see cref="Fingerprint"/>. Stays local; only its length and its
    /// last entry ever go on the wire.</summary>
    public readonly ulong[] Prefixes;

    public CycloneMessage(uint id, string name, ulong fingerprint, ulong[] prefixes)
    {
        Id = id;
        Name = name;
        Fingerprint = fingerprint;
        Prefixes = prefixes;
    }
}

/// <summary>One entry of a peer's (id, field count, fingerprint) table - what
/// <see cref="Handshake.CycloneMessages"/> is on its side.</summary>
public readonly struct CyclonePeerMessage
{
    public readonly uint Id;
    public readonly uint FieldCount;
    public readonly ulong Fingerprint;

    public CyclonePeerMessage(uint id, uint fieldCount, ulong fingerprint)
    {
        Id = id;
        FieldCount = fieldCount;
        Fingerprint = fingerprint;
    }
}

/// <summary>What a peer's fingerprints mean for this one.</summary>
public enum CycloneHandshake
{
    /// <summary>The same schema, exactly.</summary>
    Current,
    /// <summary>A different schema, but every message both ends know agrees on the
    /// fields both ends carry. Safe to proceed.</summary>
    Outdated,
    /// <summary>Both ends put different fields at an index both of them carry. There is
    /// nothing to negotiate: disconnect.</summary>
    Reject,
    /// <summary>Not decidable from the peer's table alone - at least one message needs
    /// the extra exchange described on <see cref="CycloneMessageCheck.NeedPrefix"/>.</summary>
    NeedMore,
}

/// <summary>What one of the peer's messages means for this schema's message of the same
/// id.</summary>
public enum CycloneMessageCheck
{
    /// <summary>Either this schema does not declare the message at all, or the fields
    /// both ends carry agree. Nothing to do.</summary>
    Match,
    /// <summary>Both ends put different fields at an index both of them carry.</summary>
    Reject,
    /// <summary>Undecidable from what the peer sent: the peer has more fields than this
    /// schema, so the answer lives at an index only the peer can produce. Ask it for its
    /// prefix fingerprint at the reported field count, then compare the reply against
    /// <c>CyclonePrefix</c> for the same id.</summary>
    NeedPrefix,
}

"####;

const HANDSHAKE: &str = r####"
    /// <summary>The message with this id, if this schema declares it.</summary>
    public static CycloneMessage? CycloneMessageById(uint id)
    {
        int low = 0;
        int high = CycloneMessages.Length;
        while (low < high)
        {
            int middle = (low + high) / 2;
            if (CycloneMessages[middle].Id < id)
            {
                low = middle + 1;
            }
            else
            {
                high = middle;
            }
        }
        if (low < CycloneMessages.Length && CycloneMessages[low].Id == id)
        {
            return CycloneMessages[low];
        }
        return null;
    }

    /// <summary>This schema's fingerprint for the first <paramref name="fieldCount"/>
    /// fields of a message, or <c>null</c> if it does not declare that message or does
    /// not have that many fields. <paramref name="fieldCount"/> counts from 1; 0 is the
    /// empty prefix and has no fingerprint because it always matches.</summary>
    public static ulong? CyclonePrefix(uint id, uint fieldCount)
    {
        var message = CycloneMessageById(id);
        if (!message.HasValue || fieldCount == 0 || fieldCount > message.Value.Prefixes.Length)
        {
            return null;
        }
        return message.Value.Prefixes[fieldCount - 1];
    }

    /// <summary>Compares one of the peer's messages against this schema's.</summary>
    /// <remarks>This is RFC-0002 9.1's prefix test: the two are compatible when the
    /// shorter field list is an exact prefix of the longer one, so the comparison happens
    /// at the smaller of the two field counts. <paramref name="askFor"/> is the field
    /// count to ask the peer about, and is only meaningful for
    /// <see cref="CycloneMessageCheck.NeedPrefix"/>.</remarks>
    public static CycloneMessageCheck CycloneCheckMessage(
        uint id,
        uint peerFieldCount,
        ulong peerFingerprint,
        out uint askFor)
    {
        askFor = 0;
        var message = CycloneMessageById(id);
        if (!message.HasValue)
        {
            // Not a message this schema declares, so it is never exchanged.
            return CycloneMessageCheck.Match;
        }
        var known = message.Value;
        uint localFieldCount = (uint)known.Prefixes.Length;

        if (peerFingerprint == known.Fingerprint)
        {
            return CycloneMessageCheck.Match;
        }
        if (peerFieldCount == 0 || localFieldCount == 0)
        {
            // The empty field list is a prefix of everything.
            return CycloneMessageCheck.Match;
        }
        if (peerFieldCount == localFieldCount)
        {
            // Same length, different content - a prefix of equal length would
            // have to be equality, and it is not.
            return CycloneMessageCheck.Reject;
        }
        if (peerFieldCount < localFieldCount)
        {
            // The peer's own fingerprint already is the value at the shared index.
            return known.Prefixes[peerFieldCount - 1] == peerFingerprint
                ? CycloneMessageCheck.Match
                : CycloneMessageCheck.Reject;
        }
        askFor = localFieldCount;
        return CycloneMessageCheck.NeedPrefix;
    }

    /// <summary>Compares a peer's whole message table against this schema's.</summary>
    /// <remarks>A <see cref="CycloneHandshake.NeedMore"/> result means at least one
    /// message needs the extra round; walk the table with
    /// <see cref="CycloneCheckMessage"/> to find which ones.</remarks>
    public static CycloneHandshake CycloneHandshakeCompare(
        ulong peerSchemaFingerprint,
        System.Collections.Generic.IEnumerable<CyclonePeerMessage> peerMessages)
    {
        if (peerSchemaFingerprint == CycloneSchemaFingerprint)
        {
            return CycloneHandshake.Current;
        }

        bool needMore = false;
        foreach (var peer in peerMessages)
        {
            switch (CycloneCheckMessage(peer.Id, peer.FieldCount, peer.Fingerprint, out _))
            {
                case CycloneMessageCheck.Reject:
                    // One mismatch decides the whole session. Every other message
                    // could agree and it would still be unsafe to speak.
                    return CycloneHandshake.Reject;
                case CycloneMessageCheck.NeedPrefix:
                    needMore = true;
                    break;
            }
        }

        return needMore ? CycloneHandshake.NeedMore : CycloneHandshake.Outdated;
    }
"####;

const ENVELOPE: &str = r####"
    // ======================================================================
    // Per-frame validation - validate_message_fingerprint = true.
    //
    //     [MessageId: uint][MessageFingerprint: ulong][Payload]
    //
    // Twelve bytes in front of every message, so that a peer that got past the
    // handshake still cannot decode one message as another. Off by default: the
    // wire format's premise is that there is no metadata on it, and the handshake
    // already answers this question once per connection instead of once per frame.
    // ======================================================================

    /// <summary>A frame whose envelope did not describe a message this schema can
    /// decode.</summary>
    public sealed class CycloneEnvelopeException : System.Exception
    {
        private CycloneEnvelopeException(string message) : base(message) { }

        /// <summary>An id this schema does not declare.</summary>
        public static CycloneEnvelopeException UnknownMessage(uint id) =>
            new CycloneEnvelopeException($"unknown message id 0x{id:X8}");

        /// <summary>The right message, the wrong shape.</summary>
        public static CycloneEnvelopeException FingerprintMismatch(uint id, ulong expected, ulong received) =>
            new CycloneEnvelopeException(
                $"message 0x{id:X8}: peer fingerprint 0x{received:X16}, ours 0x{expected:X16}");
    }

    /// <summary>Writes [MessageId][MessageFingerprint], immediately before the
    /// payload.</summary>
    public static void CycloneWriteEnvelope(Writer writer, CycloneMessage message)
    {
        writer.WriteU32(message.Id);
        writer.WriteU64(message.Fingerprint);
    }

    /// <summary>Reads an envelope and resolves it against this schema, leaving the
    /// reader positioned at the payload.</summary>
    /// <exception cref="DecodeException">The envelope itself could not be read.</exception>
    /// <exception cref="CycloneEnvelopeException">It names an id this schema does not
    /// declare, or it names the right message with the wrong fingerprint.</exception>
    public static CycloneMessage CycloneReadEnvelope(ref Reader reader)
    {
        uint id = reader.ReadU32();
        ulong fingerprint = reader.ReadU64();

        var message = CycloneMessageById(id);
        if (!message.HasValue)
        {
            throw CycloneEnvelopeException.UnknownMessage(id);
        }
        if (message.Value.Fingerprint != fingerprint)
        {
            throw CycloneEnvelopeException.FingerprintMismatch(id, message.Value.Fingerprint, fingerprint);
        }
        return message.Value;
    }
"####;

const ENVELOPE_OFF: &str = r####"
    // Per-frame message validation is off, so no envelope is generated and no
    // frame carries one. Turn it on in cyclone.toml:
    //
    //     validate_message_fingerprint = true
    //
    // and every frame gains [MessageId: uint][MessageFingerprint: ulong] in front of
    // its payload, with CycloneWriteEnvelope / CycloneReadEnvelope to match.
"####;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::handshake_file;
    use crate::ir::Schema;
    use crate::model::{Field, Model};

    fn model(name: &str, codecs: &[&str]) -> Model {
        Model {
            name: name.to_owned(),
            source: PathBuf::from("Models.cs"),
            line: 1,
            codecs: codecs.iter().map(|codec| (*codec).to_owned()).collect(),
            fields: vec![Field {
                name: "Id".to_owned(),
                network_type: "u32".to_owned(),
                codecs: codecs.iter().map(|codec| (*codec).to_owned()).collect(),
                line: 2,
            }],
        }
    }

    fn generated(models: &[Model]) -> String {
        handshake_file(&Schema::build(models).expect("build"), "Generated", false).expect("render")
    }

    #[test]
    fn every_constant_the_brief_asks_for_is_generated() {
        let text = generated(&[model("Player", &["edge"]), model("Enemy", &["edge"])]);

        assert!(text.contains("namespace Generated\n"), "{text}");
        assert!(
            text.contains("public const ulong CycloneSchemaFingerprint = 0x"),
            "{text}"
        );
        assert!(
            text.contains("public const ulong PlayerFingerprint = 0x"),
            "{text}"
        );
        assert!(
            text.contains("public const ulong EnemyFingerprint = 0x"),
            "{text}"
        );
        assert!(
            text.contains("public const ulong PlayerEdgeFingerprint = 0x"),
            "{text}"
        );
        assert!(
            text.contains("public const uint PlayerEdgeMessageId = 0x"),
            "{text}"
        );
        assert!(
            text.contains("public static readonly CycloneMessage[] CycloneMessages"),
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
        let text = handshake_file(&schema, "Generated", false).expect("render");

        let lines: Vec<&str> = text
            .lines()
            .filter(|line| line.trim_start().starts_with("new CycloneMessage("))
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
        assert!(
            !off.contains("public static void CycloneWriteEnvelope"),
            "{off}"
        );

        let schema = Schema::build(&[model("Player", &["edge"])]).expect("build");
        let on = handshake_file(&schema, "Generated", true).expect("render");
        assert!(
            on.contains("CycloneValidateMessageFingerprint = true"),
            "{on}"
        );
        assert!(on.contains("CycloneWriteEnvelope"), "{on}");
        assert!(on.contains("CycloneReadEnvelope"), "{on}");
    }

    #[test]
    fn two_constants_spelled_the_same_are_reported_not_emitted() {
        let schema =
            Schema::build(&[model("Player", &["edge"]), model("PlayerEdge", &[])]).expect("build");
        let error = handshake_file(&schema, "Generated", false).expect_err("collision");
        assert!(error.contains("PlayerEdgeFingerprint"), "{error}");
    }
}
