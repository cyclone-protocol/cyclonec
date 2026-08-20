use crate::fingerprint::Fingerprint;
use crate::ir::{Field, Message, Model, Schema, WireType, SCHEMA_VERSION};
use crate::json::Json;

pub const PATH: &str = ".cyclone/schema.json";

pub fn to_json(schema: &Schema) -> String {
    document(schema).to_pretty()
}

fn document(schema: &Schema) -> Json {
    let models = schema
        .models
        .iter()
        .map(|model| (model.name.clone(), model_json(model)))
        .collect();

    Json::object(vec![
        ("schema_version", Json::number(schema.schema_version)),
        ("generator", Json::string(&schema.generator)),
        ("fingerprint", Json::string(schema.fingerprint.tagged())),
        (
            "fingerprint_u64",
            Json::string(hex64(schema.fingerprint.u64())),
        ),
        ("models", Json::Object(models)),
    ])
}

fn model_json(model: &Model) -> Json {
    let messages = model
        .messages
        .iter()
        .map(|message| (message.codec.clone(), message_json(message)))
        .collect();

    Json::object(vec![
        ("source", Json::string(&model.source)),
        ("fingerprint", Json::string(model.fingerprint.tagged())),
        (
            "codecs",
            Json::Array(model.codecs.iter().map(Json::string).collect()),
        ),
        (
            "fields",
            Json::Array(model.fields.iter().map(field_json).collect()),
        ),
        ("messages", Json::Object(messages)),
    ])
}

fn field_json(field: &Field) -> Json {
    Json::object(vec![
        ("name", Json::string(&field.name)),
        ("type", Json::string(field.ty.spelling())),
        (
            "codecs",
            Json::Array(field.codecs.iter().map(Json::string).collect()),
        ),
    ])
}

fn message_json(message: &Message) -> Json {
    Json::object(vec![
        ("id", Json::string(format!("0x{:08X}", message.id))),
        ("fingerprint", Json::string(message.fingerprint.tagged())),
        (
            "fingerprint_u64",
            Json::string(hex64(message.fingerprint.u64())),
        ),
        (
            "fields",
            Json::Array(
                message
                    .fields
                    .iter()
                    .map(|field| {
                        Json::object(vec![
                            ("name", Json::string(&field.name)),
                            ("type", Json::string(field.ty.spelling())),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

pub fn hex64(value: u64) -> String {
    format!("0x{value:016X}")
}

pub fn from_json(text: &str) -> Result<Schema, String> {
    let document = crate::json::parse(text)?;

    let version = document
        .get("schema_version")
        .and_then(Json::as_u64)
        .ok_or("schema.json has no `schema_version`")?;
    if version != u64::from(SCHEMA_VERSION) {
        return Err(format!(
            "schema.json is version {version}; this cyclonec reads version {SCHEMA_VERSION}"
        ));
    }

    let generator = document
        .get("generator")
        .and_then(Json::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let fingerprint = read_fingerprint(&document, "fingerprint")?;

    let models = document
        .get("models")
        .and_then(Json::as_object)
        .ok_or("schema.json has no `models` object")?;

    let models = models
        .iter()
        .map(|(name, value)| read_model(name, value))
        .collect::<Result<Vec<Model>, String>>()?;

    Ok(Schema {
        schema_version: SCHEMA_VERSION,
        generator,
        fingerprint,
        models,
    })
}

fn read_model(name: &str, value: &Json) -> Result<Model, String> {
    let source = value
        .get("source")
        .and_then(Json::as_str)
        .unwrap_or_default()
        .to_owned();
    let fingerprint = read_fingerprint(value, "fingerprint")
        .map_err(|problem| format!("model '{name}': {problem}"))?;
    let codecs = read_strings(value.get("codecs"));

    let fields = value
        .get("fields")
        .and_then(Json::as_array)
        .ok_or_else(|| format!("model '{name}' has no `fields`"))?
        .iter()
        .map(|field| read_field(name, field))
        .collect::<Result<Vec<Field>, String>>()?;

    let messages = value
        .get("messages")
        .and_then(Json::as_object)
        .map(|entries| {
            entries
                .iter()
                .map(|(codec, message)| read_message(name, codec, message))
                .collect::<Result<Vec<Message>, String>>()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(Model {
        name: name.to_owned(),
        source,
        codecs,
        fields,
        fingerprint,
        messages,
    })
}

fn read_field(model: &str, value: &Json) -> Result<Field, String> {
    let name = value
        .get("name")
        .and_then(Json::as_str)
        .ok_or_else(|| format!("model '{model}' has a field with no `name`"))?
        .to_owned();
    let spelling = value
        .get("type")
        .and_then(Json::as_str)
        .ok_or_else(|| format!("model '{model}' field '{name}' has no `type`"))?;
    let ty = WireType::parse(spelling)
        .map_err(|problem| format!("model '{model}' field '{name}': {problem}"))?;

    Ok(Field {
        name,
        ty,
        codecs: read_strings(value.get("codecs")),
    })
}

fn read_message(model: &str, codec: &str, value: &Json) -> Result<Message, String> {
    let name = format!("{model}.{codec}");
    let fingerprint = read_fingerprint(value, "fingerprint")
        .map_err(|problem| format!("message '{name}': {problem}"))?;

    let id = value
        .get("id")
        .and_then(Json::as_str)
        .map(|text| {
            u32::from_str_radix(text.trim_start_matches("0x"), 16)
                .map_err(|_| format!("message '{name}' has a malformed `id`: {text}"))
        })
        .transpose()?
        .unwrap_or_else(|| crate::fingerprint::message_id(&name));

    let fields = value
        .get("fields")
        .and_then(Json::as_array)
        .ok_or_else(|| format!("message '{name}' has no `fields`"))?
        .iter()
        .map(|field| read_field(model, field))
        .collect::<Result<Vec<Field>, String>>()?;

    Ok(Message {
        model: model.to_owned(),
        codec: codec.to_owned(),
        name,
        id,
        fingerprint,
        fields,
    })
}

fn read_fingerprint(value: &Json, key: &str) -> Result<Fingerprint, String> {
    let text = value
        .get(key)
        .and_then(Json::as_str)
        .ok_or_else(|| format!("no `{key}`"))?;
    Fingerprint::parse(text)
}

fn read_strings(value: Option<&Json>) -> Vec<String> {
    value
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{from_json, to_json};
    use crate::ir::Schema;
    use crate::model::{Field, Model};

    fn schema() -> Schema {
        Schema::build(&[
            Model {
                name: "Player".to_owned(),
                source: PathBuf::from("src/models/player.rs"),
                line: 1,
                codecs: vec!["edge".to_owned(), "unity".to_owned()],
                fields: vec![
                    Field {
                        name: "id".to_owned(),
                        network_type: "u32".to_owned(),
                        codecs: vec!["edge".to_owned(), "unity".to_owned()],
                        line: 2,
                    },
                    Field {
                        name: "info".to_owned(),
                        network_type: "PlayerInfo".to_owned(),
                        codecs: vec!["edge".to_owned()],
                        line: 3,
                    },
                    Field {
                        name: "tags".to_owned(),
                        network_type: "Array<string>".to_owned(),
                        codecs: vec!["unity".to_owned()],
                        line: 4,
                    },
                ],
            },
            Model {
                name: "PlayerInfo".to_owned(),
                source: PathBuf::from("src/models/player.rs"),
                line: 10,
                codecs: vec!["edge".to_owned(), "unity".to_owned()],
                fields: vec![Field {
                    name: "level".to_owned(),
                    network_type: "u32".to_owned(),
                    codecs: vec!["edge".to_owned(), "unity".to_owned()],
                    line: 11,
                }],
            },
        ])
        .expect("build")
    }

    #[test]
    fn a_schema_round_trips_through_json() {
        let schema = schema();
        let text = to_json(&schema);
        let read = from_json(&text).expect("read");

        assert_eq!(read.fingerprint, schema.fingerprint);
        assert_eq!(read.models.len(), schema.models.len());
        assert_eq!(read.models[0].fields, schema.models[0].fields);
        assert_eq!(read.models[0].messages, schema.models[0].messages);
        assert_eq!(to_json(&read), text);
    }

    #[test]
    fn the_json_is_byte_stable() {
        assert_eq!(to_json(&schema()), to_json(&schema()));
    }

    #[test]
    fn a_future_schema_version_is_refused_rather_than_guessed_at() {
        let text = to_json(&schema()).replace("\"schema_version\": 1", "\"schema_version\": 2");
        let error = from_json(&text).expect_err("version");
        assert!(error.contains("version 2"), "{error}");
    }
}
