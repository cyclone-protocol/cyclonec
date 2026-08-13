//! The Cyclone IR - the source of truth for one run.
//!
//! ```text
//! Source Model
//!       ↓
//! Scanner / Parser            crate::parser  →  crate::model
//!       ↓
//! Cyclone IR                  this module
//!       ├──────────────→ generated codec
//!       ├──────────────→ schema.json
//!       ├──────────────→ fingerprints
//!       └──────────────→ build-graph
//! ```
//!
//! Everything a run produces is derived from here and from nothing else. In
//! particular an existing `.cyclone/schema.json` is **never** read to decide
//! what to generate: it is an artifact of a previous IR, useful for comparing
//! against, and re-deriving the current schema from source on every run is what
//! makes the comparison mean anything.
//!
//! The IR is where a schema stops being source text and becomes a schema: a
//! `network_type` string becomes a [`WireType`], a model plus a codec becomes a
//! [`Message`] (the thing that actually goes on the wire, and the thing a
//! fingerprint identifies), and the whole set is checked before a single byte
//! of output is written.

use std::collections::BTreeMap;
use std::path::Path;

use crate::fingerprint::{self, Fingerprint};
use crate::model;

/// A Cyclone type, as it appears on the wire.
///
/// The scanner captures `#[network(TYPE)]` as an opaque string; this is where
/// that string is resolved, once, so nothing downstream has to parse type
/// syntax again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireType {
    Bool,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    /// A `u32` byte length, then that many UTF-8 bytes (RFC-0002 §3).
    Str,
    /// A `u32` byte length, then that many raw bytes (RFC-0002 §4).
    Bytes,
    /// A `u32` element count, then that many elements (RFC-0002 §6).
    Array(Box<WireType>),
    /// Another model's fields, inline (RFC-0002 §5). The name may belong to a
    /// model this run parsed or to one it never saw - resolution is the host
    /// compiler's, same as ever.
    Model(String),
}

impl WireType {
    /// Resolves the string a `#[network(...)]` annotation spelled.
    ///
    /// # Errors
    ///
    /// A malformed composite: `Array<`, `Array<>`, an empty type.
    pub fn parse(text: &str) -> Result<WireType, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("empty network type".to_owned());
        }

        if let Some(rest) = text.strip_prefix("Array<") {
            let inner = rest
                .strip_suffix('>')
                .ok_or_else(|| format!("`{text}` is missing its closing `>`"))?;
            if inner.is_empty() {
                return Err("`Array<>` has no element type".to_owned());
            }
            return Ok(WireType::Array(Box::new(WireType::parse(inner)?)));
        }

        // A stray `<` or `>` anywhere else is not a Cyclone type at all; only
        // `Array<T>` is composite (RFC-0002 §6).
        if text.contains('<') || text.contains('>') {
            return Err(format!("`{text}` is not a Cyclone type"));
        }

        Ok(match text {
            "bool" => WireType::Bool,
            "i8" => WireType::I8,
            "u8" => WireType::U8,
            "i16" => WireType::I16,
            "u16" => WireType::U16,
            "i32" => WireType::I32,
            "u32" => WireType::U32,
            "i64" => WireType::I64,
            "u64" => WireType::U64,
            "f32" => WireType::F32,
            "f64" => WireType::F64,
            "string" => WireType::Str,
            "bytes" => WireType::Bytes,
            other => WireType::Model(other.to_owned()),
        })
    }

    /// The Specification's own spelling of the type, as written in
    /// `schema.json` and hashed into a fingerprint.
    pub fn spelling(&self) -> String {
        match self {
            WireType::Bool => "bool".to_owned(),
            WireType::I8 => "i8".to_owned(),
            WireType::U8 => "u8".to_owned(),
            WireType::I16 => "i16".to_owned(),
            WireType::U16 => "u16".to_owned(),
            WireType::I32 => "i32".to_owned(),
            WireType::U32 => "u32".to_owned(),
            WireType::I64 => "i64".to_owned(),
            WireType::U64 => "u64".to_owned(),
            WireType::F32 => "f32".to_owned(),
            WireType::F64 => "f64".to_owned(),
            WireType::Str => "string".to_owned(),
            WireType::Bytes => "bytes".to_owned(),
            WireType::Array(element) => format!("Array<{}>", element.spelling()),
            WireType::Model(name) => name.clone(),
        }
    }

    /// The model name this type reaches, if any: `PlayerInfo` and
    /// `Array<PlayerInfo>` both reach `PlayerInfo`.
    pub fn model_name(&self) -> Option<&str> {
        match self {
            WireType::Model(name) => Some(name),
            WireType::Array(element) => element.model_name(),
            _ => None,
        }
    }
}

/// A field of a model, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: WireType,
    /// The codecs this field belongs to, in the order written.
    pub codecs: Vec<String>,
}

/// One wire contract: a model rendered by one codec.
///
/// This - not the model - is what a peer encodes, decodes, and fingerprints.
/// A model with three codecs is three messages, each carrying its own subset of
/// fields in declaration order, and each with its own fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The model this message renders.
    pub model: String,
    /// The codec it renders it by.
    pub codec: String,
    /// `Model.codec` - the message's identity, and the key everything else
    /// uses to find it.
    pub name: String,
    /// A stable 32-bit id derived from [`Message::name`] alone.
    ///
    /// Deliberately *not* derived from the fingerprint: an id names a message,
    /// a fingerprint describes its current layout, and an id that changed
    /// whenever a field was appended could not be used to look the message up.
    pub id: u32,
    /// The fingerprint of this message's wire contract.
    pub fingerprint: Fingerprint,
    /// The fields this codec carries, in declaration order.
    ///
    /// Their [`Field::codecs`] is always empty: the message already is one
    /// codec, and a field inside it has nothing further to belong to.
    pub fields: Vec<Field>,
}

/// A model, resolved.
#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
    /// The file it was read from, as given on the command line, with `/`
    /// separators so a schema written on Windows and one written on Linux
    /// compare equal.
    pub source: String,
    /// The codecs the model declares, in declaration order.
    pub codecs: Vec<String>,
    /// Every annotated field, in declaration order, whether or not it joined a
    /// codec.
    pub fields: Vec<Field>,
    /// The fingerprint of the model's whole declaration.
    ///
    /// Not a wire contract - see [`Message::fingerprint`] for that - but the
    /// thing a human means by "did `Player` change?".
    pub fingerprint: Fingerprint,
    /// One message per declared codec, in declaration order.
    pub messages: Vec<Message>,
}

impl Model {
    /// The fields belonging to `codec`, in declaration order.
    pub fn fields_in<'a>(&'a self, codec: &'a str) -> impl Iterator<Item = &'a Field> {
        self.fields
            .iter()
            .filter(move |field| field.codecs.iter().any(|name| name == codec))
    }
}

/// Every model of one run, checked, resolved and fingerprinted.
#[derive(Debug, Clone)]
pub struct Schema {
    /// The `schema.json` format version.
    pub schema_version: u32,
    /// `cyclonec <version>` - the generator that produced this.
    pub generator: String,
    /// The fingerprint of the whole schema: every message, in name order.
    pub fingerprint: Fingerprint,
    /// Models, sorted by name so that file discovery order cannot change the
    /// output.
    pub models: Vec<Model>,
}

/// The `schema.json` format version this generator reads and writes.
pub const SCHEMA_VERSION: u32 = 1;

impl Schema {
    /// The model called `name`.
    pub fn model(&self, name: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.name == name)
    }

    /// Every message of every model, in model then codec declaration order.
    pub fn messages(&self) -> impl Iterator<Item = &Message> {
        self.models.iter().flat_map(|model| model.messages.iter())
    }

    /// The message called `Model.codec`.
    pub fn message(&self, name: &str) -> Option<&Message> {
        self.messages().find(|message| message.name == name)
    }

    /// Builds the IR from what the scanner collected.
    ///
    /// This is the one place a schema is validated: every check that needs to
    /// see more than one field, or more than one model, happens here, before
    /// anything is generated.
    ///
    /// # Errors
    ///
    /// A malformed network type, a duplicate model name, a nested field routed
    /// into a codec the referenced model does not declare, or two messages
    /// whose ids collide.
    pub fn build(parsed: &[model::Model]) -> Result<Schema, String> {
        let mut models: Vec<Model> = Vec::with_capacity(parsed.len());

        for source_model in parsed {
            if let Some(previous) = models.iter().find(|kept| kept.name == source_model.name) {
                return Err(format!(
                    "model '{}' is declared twice: {} and {}",
                    source_model.name,
                    previous.source,
                    slashed(&source_model.source),
                ));
            }

            let mut fields = Vec::with_capacity(source_model.fields.len());
            for field in &source_model.fields {
                let ty = WireType::parse(&field.network_type).map_err(|problem| {
                    format!(
                        "{}:{}: model '{}' field '{}': {problem}",
                        slashed(&source_model.source),
                        field.line,
                        source_model.name,
                        field.name,
                    )
                })?;
                fields.push(Field {
                    name: field.name.clone(),
                    ty,
                    codecs: field.codecs.clone(),
                });
            }

            models.push(Model {
                name: source_model.name.clone(),
                source: slashed(&source_model.source),
                codecs: source_model.codecs.clone(),
                fields,
                // Filled in below, once every model is present: a fingerprint
                // expands the models a field references, so it cannot be
                // computed while the set is still incomplete.
                fingerprint: Fingerprint::ZERO,
                messages: Vec::new(),
            });
        }

        models.sort_by(|left, right| left.name.cmp(&right.name));

        check_nested_codecs(&models)?;
        check_duplicate_fields(&models)?;

        // ------------------------------------------------- fingerprints
        let resolved = models.clone();
        for model in &mut models {
            model.fingerprint = fingerprint::model(model, &resolved);
            model.messages = model
                .codecs
                .iter()
                .map(|codec| {
                    let name = format!("{}.{}", model.name, codec);
                    Message {
                        model: model.name.clone(),
                        codec: codec.clone(),
                        id: fingerprint::message_id(&name),
                        fingerprint: fingerprint::message(model, codec, &resolved),
                        // A message *is* one codec, so its fields carry no
                        // codec list of their own: there is nothing left for
                        // one to say. `schema.json` writes them the same way,
                        // which is what lets a schema read back from disk
                        // compare equal to the one that wrote it.
                        fields: model
                            .fields_in(codec)
                            .map(|field| Field {
                                name: field.name.clone(),
                                ty: field.ty.clone(),
                                codecs: Vec::new(),
                            })
                            .collect(),
                        name,
                    }
                })
                .collect();
        }

        check_message_ids(&models)?;

        let fingerprint = fingerprint::project(&models);

        Ok(Schema {
            schema_version: SCHEMA_VERSION,
            generator: generator_name(),
            fingerprint,
            models,
        })
    }
}

/// `cyclonec <version>`, as written into every artifact.
pub fn generator_name() -> String {
    format!("cyclonec {}", env!("CARGO_PKG_VERSION"))
}

/// A path as a schema writes it: `/` separators, whatever the host uses.
fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Checks that every nested-model field routes only into codecs the
/// *referenced* model actually declares.
///
/// A field whose Cyclone type names another model carries its own codec list
/// over to the nested call: routing `Player.info` into `edge` means the
/// generated `PlayerEdgeCodec` calls `PlayerInfoEdgeCodec`. If `PlayerInfo`
/// itself never declared an `edge` codec, that call names a type nothing will
/// ever generate.
///
/// A type this run never parsed at all (a hand-written codec, a type from
/// another crate) is left for the host compiler to resolve or reject, exactly
/// as before - only a model we *did* parse has already told us which codecs it
/// has.
fn check_nested_codecs(models: &[Model]) -> Result<(), String> {
    for model in models {
        for codec in &model.codecs {
            for field in model.fields_in(codec) {
                let Some(element) = field.ty.model_name() else {
                    continue;
                };
                let Some(referenced) = models.iter().find(|candidate| candidate.name == element)
                else {
                    continue;
                };
                if referenced.codecs.iter().any(|declared| declared == codec) {
                    continue;
                }

                let declares = if referenced.codecs.is_empty() {
                    "no codecs".to_owned()
                } else {
                    format!("only: {}", referenced.codecs.join(", "))
                };
                return Err(format!(
                    "model '{}' field '{}' routes into codec '{codec}', but the model it \
                     references, '{}', declares {declares} - '{}{}Codec' would never be generated",
                    model.name,
                    field.name,
                    referenced.name,
                    referenced.name,
                    model::pascal_case(codec),
                ));
            }
        }
    }

    Ok(())
}

/// Two fields of one model may not share a name.
///
/// The scanner would happily collect both; the generated code would not
/// compile, and the schema would describe something no source can mean.
fn check_duplicate_fields(models: &[Model]) -> Result<(), String> {
    for model in models {
        for (index, field) in model.fields.iter().enumerate() {
            if model.fields[..index]
                .iter()
                .any(|kept| kept.name == field.name)
            {
                return Err(format!(
                    "model '{}' declares field '{}' twice",
                    model.name, field.name
                ));
            }
        }
    }
    Ok(())
}

/// Two messages may not share an id.
///
/// A [`Message::id`] is 32 bits of a hash of the message name, so a collision
/// takes a deliberate effort to arrange - but a collision would make two
/// different wire contracts indistinguishable at handshake, so it is reported
/// rather than hoped against.
fn check_message_ids(models: &[Model]) -> Result<(), String> {
    let mut seen: BTreeMap<u32, &str> = BTreeMap::new();

    for message in models.iter().flat_map(|model| model.messages.iter()) {
        if let Some(previous) = seen.insert(message.id, &message.name) {
            return Err(format!(
                "message id collision: '{}' and '{}' both hash to 0x{:08X} - rename one of them",
                previous, message.name, message.id,
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Schema, WireType};
    use crate::model::{Field, Model};

    fn field(name: &str, ty: &str, codecs: &[&str]) -> Field {
        Field {
            name: name.to_owned(),
            network_type: ty.to_owned(),
            codecs: codecs.iter().map(|codec| (*codec).to_owned()).collect(),
            line: 1,
        }
    }

    fn model(name: &str, codecs: &[&str], fields: Vec<Field>) -> Model {
        Model {
            name: name.to_owned(),
            source: PathBuf::from("src/models.rs"),
            line: 1,
            codecs: codecs.iter().map(|codec| (*codec).to_owned()).collect(),
            fields,
        }
    }

    #[test]
    fn resolves_every_primitive_and_the_composites() {
        assert_eq!(WireType::parse("u32"), Ok(WireType::U32));
        assert_eq!(WireType::parse("string"), Ok(WireType::Str));
        assert_eq!(
            WireType::parse("Array<f64>"),
            Ok(WireType::Array(Box::new(WireType::F64)))
        );
        assert_eq!(
            WireType::parse("Array<Array<u8>>"),
            Ok(WireType::Array(Box::new(WireType::Array(Box::new(
                WireType::U8
            )))))
        );
        assert_eq!(
            WireType::parse("PlayerInfo"),
            Ok(WireType::Model("PlayerInfo".to_owned()))
        );
    }

    #[test]
    fn rejects_a_malformed_composite() {
        assert!(WireType::parse("Array<").is_err());
        assert!(WireType::parse("Array<>").is_err());
        assert!(WireType::parse("Vec<u32>").is_err());
        assert!(WireType::parse("").is_err());
    }

    #[test]
    fn a_spelling_round_trips() {
        for text in ["bool", "u64", "string", "bytes", "Array<Player>", "Player"] {
            assert_eq!(WireType::parse(text).expect("parse").spelling(), text);
        }
    }

    #[test]
    fn a_model_becomes_one_message_per_codec() {
        let schema = Schema::build(&[model(
            "Player",
            &["edge", "unity"],
            vec![
                field("id", "u32", &["edge", "unity"]),
                field("x", "f32", &["edge"]),
            ],
        )])
        .expect("build");

        let names: Vec<&str> = schema.messages().map(|message| &*message.name).collect();
        assert_eq!(names, ["Player.edge", "Player.unity"]);

        let edge = schema.message("Player.edge").expect("edge");
        assert_eq!(edge.fields.len(), 2);
        let unity = schema.message("Player.unity").expect("unity");
        assert_eq!(unity.fields.len(), 1);
        assert_ne!(edge.fingerprint, unity.fingerprint);
    }

    #[test]
    fn models_are_sorted_so_discovery_order_cannot_matter() {
        let one = Schema::build(&[
            model("Zebra", &["edge"], vec![field("id", "u32", &["edge"])]),
            model("Alpha", &["edge"], vec![field("id", "u32", &["edge"])]),
        ])
        .expect("build");
        let other = Schema::build(&[
            model("Alpha", &["edge"], vec![field("id", "u32", &["edge"])]),
            model("Zebra", &["edge"], vec![field("id", "u32", &["edge"])]),
        ])
        .expect("build");

        assert_eq!(one.fingerprint, other.fingerprint);
        assert_eq!(one.models[0].name, "Alpha");
    }

    #[test]
    fn a_nested_field_must_route_into_a_codec_the_referenced_model_declares() {
        let error = Schema::build(&[
            model(
                "Player",
                &["edge"],
                vec![field("info", "PlayerInfo", &["edge"])],
            ),
            model(
                "PlayerInfo",
                &["unity"],
                vec![field("level", "u32", &["unity"])],
            ),
        ])
        .expect_err("dangling nested codec");

        assert!(error.contains("PlayerInfoEdgeCodec"), "{error}");
    }

    #[test]
    fn a_duplicate_model_name_is_an_error() {
        let error = Schema::build(&[
            model("Player", &["edge"], vec![]),
            model("Player", &["edge"], vec![]),
        ])
        .expect_err("duplicate");
        assert!(error.contains("declared twice"), "{error}");
    }

    #[test]
    fn a_duplicate_field_name_is_an_error() {
        let error = Schema::build(&[model(
            "Player",
            &["edge"],
            vec![field("id", "u32", &["edge"]), field("id", "f32", &["edge"])],
        )])
        .expect_err("duplicate field");
        assert!(error.contains("twice"), "{error}");
    }
}
