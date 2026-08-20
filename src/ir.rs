use std::collections::BTreeMap;
use std::path::Path;

use crate::fingerprint::{self, Fingerprint};
use crate::model;

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
    Str,
    Bytes,
    Array(Box<WireType>),
    Model(String),
}

impl WireType {
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

    pub fn model_name(&self) -> Option<&str> {
        match self {
            WireType::Model(name) => Some(name),
            WireType::Array(element) => element.model_name(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: WireType,
    pub codecs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub model: String,
    pub codec: String,
    pub name: String,
    pub id: u32,
    pub fingerprint: Fingerprint,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
    pub source: String,
    pub codecs: Vec<String>,
    pub fields: Vec<Field>,
    pub fingerprint: Fingerprint,
    pub messages: Vec<Message>,
}

impl Model {
    pub fn fields_in<'a>(&'a self, codec: &'a str) -> impl Iterator<Item = &'a Field> {
        self.fields
            .iter()
            .filter(move |field| field.codecs.iter().any(|name| name == codec))
    }
}

#[derive(Debug, Clone)]
pub struct Schema {
    pub schema_version: u32,
    pub generator: String,
    pub fingerprint: Fingerprint,
    pub models: Vec<Model>,
}

pub const SCHEMA_VERSION: u32 = 1;

impl Schema {
    pub fn model(&self, name: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.name == name)
    }

    pub fn messages(&self) -> impl Iterator<Item = &Message> {
        self.models.iter().flat_map(|model| model.messages.iter())
    }

    pub fn message(&self, name: &str) -> Option<&Message> {
        self.messages().find(|message| message.name == name)
    }

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
                fingerprint: Fingerprint::ZERO,
                messages: Vec::new(),
            });
        }

        models.sort_by(|left, right| left.name.cmp(&right.name));

        check_nested_codecs(&models)?;
        check_duplicate_fields(&models)?;
        check_canonical_field_names(&models)?;

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

pub fn generator_name() -> String {
    format!("cyclonec {}", env!("CARGO_PKG_VERSION"))
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

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

fn check_canonical_field_names(models: &[Model]) -> Result<(), String> {
    for model in models {
        for codec in &model.codecs {
            let mut seen: BTreeMap<(String, String), &str> = BTreeMap::new();
            for field in model.fields_in(codec) {
                let canonical = fingerprint::canonical_field_name(&field.name);
                let key = (canonical.clone(), field.ty.spelling());
                if let Some(previous) = seen.insert(key, &field.name) {
                    return Err(format!(
                        "message '{}.{codec}': fields '{}' and '{}' are both '{canonical}' of \
                         type {} - a fingerprint could not tell them apart, so rename one",
                        model.name,
                        previous,
                        field.name,
                        field.ty.spelling()
                    ));
                }
            }
        }
    }
    Ok(())
}

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

    #[test]
    fn two_fields_a_fingerprint_could_not_tell_apart_are_an_error() {
        let error = Schema::build(&[model(
            "Player",
            &["edge"],
            vec![field("ID", "u32", &["edge"]), field("Id", "u32", &["edge"])],
        )])
        .expect_err("collision");
        assert!(error.contains("Player.edge"), "{error}");
        assert!(error.contains("both 'id' of type u32"), "{error}");

        let also = Schema::build(&[model(
            "Player",
            &["edge"],
            vec![
                field("player_id", "u32", &["edge"]),
                field("playerId", "u32", &["edge"]),
            ],
        )])
        .expect_err("collision");
        assert!(also.contains("both 'playerid' of type u32"), "{also}");
    }

    #[test]
    fn a_shared_canonical_name_with_different_types_is_allowed() {
        let schema = Schema::build(&[model(
            "Player",
            &["edge"],
            vec![field("ID", "u32", &["edge"]), field("Id", "f32", &["edge"])],
        )])
        .expect("build");

        let swapped = Schema::build(&[model(
            "Player",
            &["edge"],
            vec![field("Id", "f32", &["edge"]), field("ID", "u32", &["edge"])],
        )])
        .expect("build");

        assert_ne!(
            schema.message("Player.edge").expect("one").fingerprint,
            swapped.message("Player.edge").expect("other").fingerprint,
            "the reorder must still be visible"
        );
    }

    #[test]
    fn a_collision_across_codecs_is_not_a_collision() {
        Schema::build(&[model(
            "Player",
            &["edge", "unity"],
            vec![
                field("ID", "u32", &["edge"]),
                field("Id", "u32", &["unity"]),
            ],
        )])
        .expect("two codecs, one field each");

        Schema::build(&[model(
            "Player",
            &["edge"],
            vec![field("id", "u32", &["edge"]), field("Id", "u32", &[])],
        )])
        .expect("the second field is on no wire");
    }
}
