//! Deterministic schema fingerprints.
//!
//! A fingerprint answers exactly one question - **same, or different** - about
//! one wire contract, in 32 bytes, without sending a schema anywhere. When two
//! peers disagree, working out *how* they disagree is
//! [`crate::compat`]'s job, from two schemas, at build time. A fingerprint is
//! never asked to explain itself.
//!
//! # The canonical form
//!
//! A fingerprint is `SHA-256` over a canonical UTF-8 text. The text is fully
//! specified so that Go, C#, C++ and every future SDK can produce the same 32
//! bytes from the same schema; nothing about it is Rust's, and nothing about it
//! is left to an implementation to choose. `SPEC-FINGERPRINT.md` in this
//! repository is the normative statement of what follows.
//!
//! ```text
//! cyclone-fingerprint/1
//! message Player.edge
//! field 0 id u32
//! field 1 x f32
//! end
//! ```
//!
//! Lines are `\n`-separated, fields are separated by a single space, and the
//! text always ends in a newline. A field's line is its **index**, its
//! **canonical name** and its **type spelling** - the three things a decoder
//! positioned at that offset depends on.
//!
//! # Why the name is canonicalised
//!
//! A field's name is hashed in the one spelling every language agrees on, not
//! the spelling its source happened to use. `cyclonec` reads one language per
//! run, so the Rust side of a project writes `player_id` while the Go side
//! writes `PlayerID` and the C# side writes `PlayerId` - three names for one
//! field, on a wire that is byte-identical. Hashing them as written made those
//! three peers reject each other at the handshake, which is a false alarm about
//! a naming convention dressed up as a schema disagreement.
//!
//! The canonical name drops `_`, `-` and spaces and lowercases what is left,
//! so all three of those spell `playerid` - see [`canonical_field_name`].
//!
//! It deliberately does not try to recover *words*. A word-splitting rule has
//! to answer `UserIDs`, and `HTTPServer` proves there is no answer: the two are
//! the same shape - an acronym run followed by lowercase - so any rule that
//! splits `HTTP|Server` also splits `I|Ds`, and Go's `UserIDs` then stops
//! matching Rust's `user_ids`. Erasing the boundaries instead means every SDK
//! reaches the same string without having to agree on where a word begins,
//! which is the same reason [`crate::sha256`] is written out rather than
//! depended on.
//!
//! The price is that two names differing only in where their underscores sit
//! (`notify_url` and `notif_yurl`) canonicalise together. Where that could hide
//! a reorder - both in one message, both the same type - [`crate::ir`] rejects
//! the schema. Everywhere else it is two spellings of one field at one offset
//! with one type: a label, and the wire never carried the label. What the
//! canonical name does *not* erase is a real rename, so `x` to `position_x` is
//! still a different fingerprint.
//!
//! A field whose type is another model this run parsed is expanded **inline**,
//! under the same codec, because that is what the bytes do:
//!
//! ```text
//! field 2 info model<PlayerInfo:message PlayerInfo.edge
//! field 0 level u32
//! end
//! >
//! ```
//!
//! so a change deep inside a nested model changes the fingerprint of everything
//! that carries it. A model the run never parsed is `extern<Name>` - the layout
//! cannot be seen from here, so its name is all there is to hash. A model that
//! is already being expanded (a recursive schema, reached through an `Array<T>`)
//! is `recursive<Name>`, which terminates the expansion without losing the fact
//! that the recursion is there.
//!
//! # Why field names are hashed
//!
//! The wire format does not carry field names - position is the only identifier
//! (RFC-0001 §4.1) - so hashing the layout alone would be defensible. It is
//! also unsafe, and this is the one place where the two readings of the
//! requirement "a fingerprint must detect field reorder" come apart:
//!
//! ```text
//! v1: x: f32, y: f32          v2: y: f32, x: f32
//! ```
//!
//! Layout-only, those two hash **identically**: same types, same offsets, same
//! byte count. Two peers would shake hands, agree they are current, and then
//! quietly transpose every coordinate they exchange. Hashing the names makes
//! that a fingerprint mismatch, which a handshake can act on.
//!
//! The price is that a pure rename - `x` to `position_x`, no byte on the wire
//! changed - also changes the fingerprint, so peers that could have talked will
//! be told they are on different schemas and reject each other. That is a loud,
//! immediate, harmless failure, and the alternative is a silent, permanent,
//! data-corrupting success. `SPEC-FINGERPRINT.md` records the choice.
//!
//! Canonicalising the name narrows that price to the renames a human meant.
//! `Id` to `id` is a convention, not a rename, and no longer costs a handshake.

use crate::ir::{Field, Model, WireType};
use crate::sha256;

/// A 32-byte schema digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// The placeholder the IR holds while a schema is still being assembled.
    /// Never written to an artifact.
    pub const ZERO: Fingerprint = Fingerprint([0; 32]);

    /// The fingerprint of some canonical text.
    pub fn of(canonical: &str) -> Fingerprint {
        Fingerprint(sha256::hash(canonical.as_bytes()))
    }

    /// The 32 raw bytes.
    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The digest as lowercase hexadecimal.
    pub fn hex(&self) -> String {
        sha256::hex(&self.0)
    }

    /// The digest as `schema.json` writes it: `sha256:<64 hex digits>`.
    pub fn tagged(&self) -> String {
        format!("sha256:{}", self.hex())
    }

    /// The 64-bit form, for generated constants and for a handshake: the
    /// **first eight bytes** of the digest, big endian.
    ///
    /// Truncation is deliberate. 64 bits is what fits in a register, in a
    /// generated `const`, and in a handshake frame; the full digest stays in
    /// `schema.json` for anything that needs to be certain.
    pub fn u64(&self) -> u64 {
        u64::from_be_bytes([
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5], self.0[6], self.0[7],
        ])
    }

    /// Reads back the `sha256:<hex>` form.
    ///
    /// # Errors
    ///
    /// A missing prefix, a wrong length, or a non-hex digit.
    pub fn parse(text: &str) -> Result<Fingerprint, String> {
        let hex = text
            .strip_prefix("sha256:")
            .ok_or_else(|| format!("`{text}` is not a `sha256:` fingerprint"))?;
        if hex.len() != 64 {
            return Err(format!("`{text}` is {} hex digits, not 64", hex.len()));
        }

        let mut bytes = [0u8; 32];
        for (index, slot) in bytes.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
                .map_err(|_| format!("`{text}` is not hexadecimal"))?;
        }
        Ok(Fingerprint(bytes))
    }
}

/// The version tag every canonical text starts with.
///
/// It is inside the hash on purpose: a future canonical form is a different
/// tag, and old and new fingerprints then cannot silently compare equal.
///
/// `/2` canonicalises field names; `/1` hashed them as the source spelled them.
const HEADER: &str = "cyclone-fingerprint/2\n";

/// The one spelling of a field name that every language agrees on, as
/// `SPEC-FINGERPRINT.md` §3.2 defines it.
pub fn canonical_field_name(name: &str) -> String {
    let canonical: String = name
        .chars()
        .filter(|character| !matches!(character, '_' | '-' | ' '))
        .map(|character| character.to_ascii_lowercase())
        .collect();

    if canonical.is_empty() {
        return name.to_owned();
    }
    canonical
}

/// The fingerprint of one wire contract: `model` as rendered by `codec`.
pub fn message(model: &Model, codec: &str, schema: &[Model]) -> Fingerprint {
    Fingerprint::of(&canonical_message(model, codec, schema))
}

/// The fingerprint of a model's whole declaration, codec routing aside.
pub fn model(model: &Model, schema: &[Model]) -> Fingerprint {
    Fingerprint::of(&canonical_model(model, schema))
}

/// The fingerprint of a whole schema: every message, by name, with its own
/// fingerprint.
///
/// Messages are sorted by name, so the order files happened to be discovered in
/// cannot change the answer.
pub fn project(models: &[Model]) -> Fingerprint {
    let mut lines: Vec<String> = models
        .iter()
        .flat_map(|model| model.messages.iter())
        .map(|message| format!("message {} {}\n", message.name, message.fingerprint.hex()))
        .collect();
    lines.sort();

    let mut text = String::with_capacity(64 + lines.len() * 96);
    text.push_str(HEADER);
    text.push_str("schema\n");
    for line in lines {
        text.push_str(&line);
    }
    text.push_str("end\n");

    Fingerprint::of(&text)
}

/// A stable 32-bit id for a message name.
///
/// Derived from the name and nothing else, so that appending a field does not
/// renumber a message and a peer can still say *which* message it is talking
/// about while disagreeing about its shape.
pub fn message_id(name: &str) -> u32 {
    let digest = sha256::hash(format!("cyclone-message-id/1\n{name}\n").as_bytes());
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}

/// The canonical text a message's fingerprint is taken over.
///
/// Public because it is the thing to print when a fingerprint is not what
/// somebody expected - most of all when a second SDK computes a different one,
/// where diffing these two texts is the whole of the debugging.
pub fn canonical_message(model: &Model, codec: &str, schema: &[Model]) -> String {
    let mut text = String::with_capacity(256);
    text.push_str(HEADER);
    write_message(&mut text, model, codec, schema, &mut Vec::new());
    text
}

/// The canonical text a model's fingerprint is taken over.
pub fn canonical_model(model: &Model, schema: &[Model]) -> String {
    let mut text = String::with_capacity(256);
    text.push_str(HEADER);
    write_model(&mut text, model, schema, &mut Vec::new());
    text
}

fn write_message(
    text: &mut String,
    model: &Model,
    codec: &str,
    schema: &[Model],
    stack: &mut Vec<String>,
) {
    text.push_str("message ");
    text.push_str(&model.name);
    text.push('.');
    text.push_str(codec);
    text.push('\n');
    for (index, field) in model.fields_in(codec).enumerate() {
        write_field(text, index, field, Some(codec), schema, stack);
    }
    text.push_str("end\n");
}

fn write_model(text: &mut String, model: &Model, schema: &[Model], stack: &mut Vec<String>) {
    text.push_str("model ");
    text.push_str(&model.name);
    text.push('\n');
    for (index, field) in model.fields.iter().enumerate() {
        write_field(text, index, field, None, schema, stack);
    }
    text.push_str("end\n");
}

fn write_field(
    text: &mut String,
    index: usize,
    field: &Field,
    codec: Option<&str>,
    schema: &[Model],
    stack: &mut Vec<String>,
) {
    text.push_str("field ");
    text.push_str(&index.to_string());
    text.push(' ');
    text.push_str(&canonical_field_name(&field.name));
    text.push(' ');
    write_type(text, &field.ty, codec, schema, stack);
    text.push('\n');
}

fn write_type(
    text: &mut String,
    ty: &WireType,
    codec: Option<&str>,
    schema: &[Model],
    stack: &mut Vec<String>,
) {
    match ty {
        WireType::Array(element) => {
            text.push_str("Array<");
            write_type(text, element, codec, schema, stack);
            text.push('>');
        }
        WireType::Model(name) => {
            if stack.iter().any(|seen| seen == name) {
                text.push_str("recursive<");
                text.push_str(name);
                text.push('>');
                return;
            }
            let Some(nested) = schema.iter().find(|model| &model.name == name) else {
                // A type this run never parsed. Its layout is not visible from
                // here, so its name is all there is to hash - and a schema that
                // resolves it later will produce a different fingerprint, which
                // is correct: it is a different, now-known, contract.
                text.push_str("extern<");
                text.push_str(name);
                text.push('>');
                return;
            };

            stack.push(name.clone());
            text.push_str("model<");
            text.push_str(name);
            text.push(':');
            match codec {
                Some(codec) => write_message(text, nested, codec, schema, stack),
                None => write_model(text, nested, schema, stack),
            }
            text.push('>');
            stack.pop();
        }
        primitive => text.push_str(&primitive.spelling()),
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_field_name, canonical_message, message_id, Fingerprint};
    use crate::ir::Schema;
    use crate::model::{Field, Model};
    use std::path::PathBuf;

    fn field(name: &str, ty: &str) -> Field {
        Field {
            name: name.to_owned(),
            network_type: ty.to_owned(),
            codecs: vec!["edge".to_owned()],
            line: 1,
        }
    }

    fn model(name: &str, fields: Vec<Field>) -> Model {
        Model {
            name: name.to_owned(),
            source: PathBuf::from("src/models.rs"),
            line: 1,
            codecs: vec!["edge".to_owned()],
            fields,
        }
    }

    fn schema(models: &[Model]) -> Schema {
        Schema::build(models).expect("build")
    }

    fn player(fields: Vec<Field>) -> Schema {
        schema(&[model("Player", fields)])
    }

    fn player_fingerprint(fields: Vec<Field>) -> Fingerprint {
        player(fields)
            .message("Player.edge")
            .expect("message")
            .fingerprint
    }

    /// The exact bytes hashed, written out. If this test has to change, every
    /// SDK's fingerprints change with it - which is the point of pinning it.
    #[test]
    fn the_canonical_text_is_exactly_this() {
        let schema = player(vec![field("id", "u32"), field("x", "f32")]);
        let model = schema.model("Player").expect("model");

        assert_eq!(
            canonical_message(model, "edge", &schema.models),
            "cyclone-fingerprint/2\nmessage Player.edge\nfield 0 id u32\nfield 1 x f32\nend\n"
        );
    }

    /// The whole point of `/2`: the same model, spelled by each language's own
    /// convention, is one fingerprint. These are the exact spellings the Rust,
    /// Go and C# fixtures use for `Player.edge`.
    #[test]
    fn a_naming_convention_is_not_a_schema_difference() {
        let rust = player_fingerprint(vec![field("id", "u32"), field("x", "f32")]);
        let go = player_fingerprint(vec![field("ID", "u32"), field("X", "f32")]);
        let csharp = player_fingerprint(vec![field("Id", "u32"), field("X", "f32")]);
        let screaming = player_fingerprint(vec![field("ID_", "u32"), field("X", "f32")]);

        assert_eq!(rust, go);
        assert_eq!(rust, csharp);
        assert_eq!(rust, screaming);
    }

    /// A rename a human meant still costs a handshake - canonicalising names
    /// narrows the false positive, it does not remove the property.
    #[test]
    fn a_real_rename_still_changes_the_fingerprint() {
        assert_ne!(
            player_fingerprint(vec![field("x", "f32")]),
            player_fingerprint(vec![field("position_x", "f32")]),
        );
    }

    /// The test vectors `SPEC-FINGERPRINT.md` §3.2 publishes. Every SDK has to
    /// reproduce this table exactly or its fingerprints are not Cyclone's.
    #[test]
    fn a_canonical_name_is_exactly_this() {
        for (spelling, canonical) in [
            ("id", "id"),
            ("ID", "id"),
            ("Id", "id"),
            ("player_id", "playerid"),
            ("playerId", "playerid"),
            ("PlayerId", "playerid"),
            ("PlayerID", "playerid"),
            ("PLAYER_ID", "playerid"),
            ("player-id", "playerid"),
            ("__player_id__", "playerid"),
            ("HTTPServer", "httpserver"),
            ("http_server", "httpserver"),
            ("HttpServer", "httpserver"),
            ("UserIDs", "userids"),
            ("user_ids", "userids"),
            ("UserIds", "userids"),
            ("vec3", "vec3"),
            ("Vec3", "vec3"),
            ("vec_3", "vec3"),
            ("position3D", "position3d"),
            ("position_3d", "position3d"),
            ("café_id", "caféid"),
            ("_", "_"),
            ("__", "__"),
        ] {
            assert_eq!(canonical_field_name(spelling), canonical, "{spelling}");
        }
    }

    /// The two things the canonical name must still tell apart, or hashing it
    /// would be worse than hashing nothing.
    #[test]
    fn a_canonical_name_still_separates_what_matters() {
        assert_ne!(canonical_field_name("x"), canonical_field_name("y"));
        assert_ne!(
            canonical_field_name("x"),
            canonical_field_name("position_x")
        );
    }

    /// A pinned digest. A change to the canonical form, the header tag, or
    /// SHA-256 itself shows up here and nowhere else.
    #[test]
    fn a_fingerprint_is_stable_across_runs_and_releases() {
        let fingerprint = player_fingerprint(vec![field("id", "u32"), field("x", "f32")]);
        assert_eq!(
            fingerprint.tagged(),
            "sha256:f1ed8779e2a4a35d30067fa88ba1cec15f7417ea9907df5752f06b97a0d93ddc"
        );
    }

    /// Every change §8 requires a fingerprint to detect, detected.
    #[test]
    fn every_wire_change_changes_the_fingerprint() {
        let base = player_fingerprint(vec![field("id", "u32"), field("x", "f32")]);

        // reorder
        assert_ne!(
            base,
            player_fingerprint(vec![field("x", "f32"), field("id", "u32")])
        );
        // type change
        assert_ne!(
            base,
            player_fingerprint(vec![field("id", "u64"), field("x", "f32")])
        );
        // deletion
        assert_ne!(base, player_fingerprint(vec![field("id", "u32")]));
        // insertion in the middle
        assert_ne!(
            base,
            player_fingerprint(vec![
                field("id", "u32"),
                field("level", "u32"),
                field("x", "f32"),
            ])
        );
        // append at the end
        assert_ne!(
            base,
            player_fingerprint(vec![
                field("id", "u32"),
                field("x", "f32"),
                field("level", "u32"),
            ])
        );
    }

    /// The case layout-only hashing cannot see: two same-typed fields swapped.
    #[test]
    fn swapping_two_same_typed_fields_changes_the_fingerprint() {
        assert_ne!(
            player_fingerprint(vec![field("x", "f32"), field("y", "f32")]),
            player_fingerprint(vec![field("y", "f32"), field("x", "f32")]),
        );
    }

    /// A change inside a nested model changes the fingerprint of everything
    /// that carries it, because the bytes move.
    #[test]
    fn a_nested_change_reaches_the_outer_fingerprint() {
        let before = schema(&[
            model("Player", vec![field("info", "PlayerInfo")]),
            model("PlayerInfo", vec![field("level", "u32")]),
        ]);
        let after = schema(&[
            model("Player", vec![field("info", "PlayerInfo")]),
            model("PlayerInfo", vec![field("level", "u64")]),
        ]);

        assert_ne!(
            before.message("Player.edge").expect("before").fingerprint,
            after.message("Player.edge").expect("after").fingerprint,
        );
    }

    /// A recursive schema terminates instead of expanding forever.
    #[test]
    fn a_recursive_model_terminates() {
        let schema = schema(&[model(
            "Node",
            vec![field("id", "u32"), field("children", "Array<Node>")],
        )]);
        let text = canonical_message(schema.model("Node").expect("model"), "edge", &schema.models);
        assert!(text.contains("recursive<Node>"), "{text}");
    }

    /// A message id follows the name, not the layout: appending a field leaves
    /// the id alone so a peer can still say which message it means.
    #[test]
    fn a_message_id_follows_the_name_only() {
        let one = player(vec![field("id", "u32")]);
        let two = player(vec![field("id", "u32"), field("x", "f32")]);

        let one = one.message("Player.edge").expect("one");
        let two = two.message("Player.edge").expect("two");
        assert_eq!(one.id, two.id);
        assert_ne!(one.fingerprint, two.fingerprint);
        assert_eq!(one.id, message_id("Player.edge"));
    }

    #[test]
    fn a_fingerprint_round_trips_through_its_text_form() {
        let fingerprint = player_fingerprint(vec![field("id", "u32")]);
        assert_eq!(
            Fingerprint::parse(&fingerprint.tagged()).expect("parse"),
            fingerprint
        );
        assert!(Fingerprint::parse("abc").is_err());
        assert!(Fingerprint::parse("sha256:xyz").is_err());
    }

    /// The 64-bit form is the first eight bytes, big endian - the form a
    /// generated constant and a handshake frame carry.
    #[test]
    fn the_u64_form_is_the_leading_eight_bytes() {
        let fingerprint =
            Fingerprint::parse(&format!("sha256:{}", "0123456789abcdef".repeat(4))).expect("parse");
        assert_eq!(fingerprint.u64(), 0x0123_4567_89ab_cdef);
    }
}
