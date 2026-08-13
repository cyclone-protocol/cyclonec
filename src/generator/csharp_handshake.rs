//! `Handshake.cs` - the fingerprints, generated.
//!
//! The C# counterpart of [`super::handshake`] and [`super::go_handshake`] -
//! same contract, same safety property:
//!
//! ```text
//! peer schema fingerprint == ours               -> Current    accept
//! a message both ends know, fingerprints differ -> Reject     disconnect
//! otherwise                                      -> Outdated   accept
//! ```
//!
//! C# has no top-level `const` or `static` outside a type, so where Go writes
//! every fingerprint as a package-level constant, this backend gathers them -
//! and the lookup table and the compare function that go with them - into one
//! `public static class Handshake`. [`CycloneMessage`] and
//! [`CycloneHandshake`] are ordinary namespace-level types beside it, the
//! same shape they have in every other backend.

use std::collections::BTreeMap;

use crate::ir::Schema;
use crate::model::pascal_case;
use crate::schema::hex64;

/// The file name, relative to the output directory.
pub const FILE_NAME: &str = "handshake.cs";

/// Renders `Handshake.cs`.
///
/// # Errors
///
/// Two constants that would be spelled the same - a model named `PlayerEdge`
/// beside a `Player` with an `edge` codec. Rare, mechanical, and far better
/// reported here than as a redefinition error in the user's build.
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
        }
        out.push('\n');
    }

    // Sorted by id, so a peer's table and ours can be compared without either
    // side sorting first.
    let mut messages: Vec<_> = schema.messages().collect();
    messages.sort_by_key(|message| message.id);

    out.push_str(
        "    /// <summary>Every message this schema declares, sorted by id.</summary>\n\
         \x20\x20\x20\x20public static readonly CycloneMessage[] CycloneMessages = new CycloneMessage[]\n    {\n",
    );
    for message in &messages {
        let constant = message_constant(&message.model, &message.codec);
        out.push_str(&format!(
            "        new CycloneMessage({constant}MessageId, \"{}\", {constant}Fingerprint),\n",
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

/// `Player` + `edge` → `PlayerEdge`.
fn message_constant(model: &str, codec: &str) -> String {
    format!("{model}{}", pascal_case(codec))
}

/// Two constants may not be spelled the same.
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

/// The message descriptor and the handshake verdict, identical in every
/// generated `Handshake.cs`.
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

    public CycloneMessage(uint id, string name, ulong fingerprint)
    {
        Id = id;
        Name = name;
        Fingerprint = fingerprint;
    }
}

/// <summary>One entry of a peer's (id, fingerprint) table - what
/// <see cref="Handshake.CycloneMessages"/> is on its side.</summary>
public readonly struct CyclonePeerMessage
{
    public readonly uint Id;
    public readonly ulong Fingerprint;

    public CyclonePeerMessage(uint id, ulong fingerprint)
    {
        Id = id;
        Fingerprint = fingerprint;
    }
}

/// <summary>What a peer's fingerprints mean for this one.</summary>
public enum CycloneHandshake
{
    /// <summary>The same schema, exactly.</summary>
    Current,
    /// <summary>A different schema, but no message both ends know disagrees. One side is
    /// older; every message they share is byte-identical.</summary>
    Outdated,
    /// <summary>A message both ends know has two different shapes. There is nothing to
    /// negotiate: disconnect.</summary>
    Reject,
}

"####;

/// The lookup and compare functions, identical in every generated
/// `Handshake.cs` - appended right after the generated `CycloneMessages`
/// table they read.
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

    /// <summary>Compares a peer's fingerprints against this schema's.</summary>
    /// <remarks><paramref name="peerMessages"/> is only worth sending when the schema
    /// fingerprints already differ.</remarks>
    public static CycloneHandshake CycloneHandshakeCompare(
        ulong peerSchemaFingerprint,
        System.Collections.Generic.IEnumerable<CyclonePeerMessage> peerMessages)
    {
        if (peerSchemaFingerprint == CycloneSchemaFingerprint)
        {
            return CycloneHandshake.Current;
        }

        foreach (var peer in peerMessages)
        {
            var known = CycloneMessageById(peer.Id);
            if (known.HasValue && known.Value.Fingerprint != peer.Fingerprint)
            {
                // A message both ends know, with two shapes. Every other
                // message could match and it would still be unsafe to speak.
                return CycloneHandshake.Reject;
            }
        }

        return CycloneHandshake.Outdated;
    }
"####;

/// The optional per-frame envelope, when `validate_message_fingerprint` is
/// on.
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

/// What stands in for the envelope when it is off.
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
