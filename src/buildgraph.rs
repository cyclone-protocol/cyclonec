use std::collections::BTreeMap;

use crate::fingerprint::Fingerprint;
use crate::ir::Schema;
use crate::json::Json;

pub const PATH: &str = ".cyclone/build-graph.json";

pub struct Artifact {
    pub path: String,
    pub source: String,
    pub model: String,
    pub codec: String,
    pub fingerprint: Fingerprint,
    pub sha256: String,
}

pub struct Shared {
    pub path: String,
    pub sha256: String,
    pub kind: &'static str,
}

pub fn to_json(schema: &Schema, artifacts: &[Artifact], shared: &[Shared]) -> String {
    let mut by_source: BTreeMap<&str, Vec<&Artifact>> = BTreeMap::new();
    for artifact in artifacts {
        by_source
            .entry(&artifact.source)
            .or_default()
            .push(artifact);
    }

    let mut sources: Vec<(String, Json)> = Vec::with_capacity(by_source.len());
    for (source, artifacts) in by_source {
        let models: Vec<Json> = schema
            .models
            .iter()
            .filter(|model| model.source == source)
            .map(|model| Json::string(model.name.as_str()))
            .collect();

        let outputs: Vec<Json> = artifacts
            .iter()
            .map(|artifact| {
                Json::object(vec![
                    ("path", Json::string(artifact.path.as_str())),
                    ("model", Json::string(artifact.model.as_str())),
                    ("codec", Json::string(artifact.codec.as_str())),
                    ("fingerprint", Json::string(artifact.fingerprint.tagged())),
                    ("sha256", Json::string(artifact.sha256.as_str())),
                ])
            })
            .collect();

        sources.push((
            source.to_owned(),
            Json::object(vec![
                ("models", Json::Array(models)),
                ("outputs", Json::Array(outputs)),
            ]),
        ));
    }

    let shared: Vec<Json> = shared
        .iter()
        .map(|file| {
            Json::object(vec![
                ("path", Json::string(file.path.as_str())),
                ("kind", Json::string(file.kind)),
                ("sha256", Json::string(file.sha256.as_str())),
            ])
        })
        .collect();

    Json::object(vec![
        ("schema_version", Json::number(schema.schema_version)),
        ("generator", Json::string(schema.generator.as_str())),
        (
            "schema_fingerprint",
            Json::string(schema.fingerprint.tagged()),
        ),
        (
            "sha256_of",
            Json::string(
                "each file with its `// generated-at:` line emptied, so an unchanged \
                 schema keeps an unchanged digest",
            ),
        ),
        ("sources", Json::Object(sources)),
        ("shared", Json::Array(shared)),
    ])
    .to_pretty()
}

pub fn digest(contents: &str) -> String {
    let stable = crate::generator::without_timestamp(contents);
    crate::sha256::hex(&crate::sha256::hash(stable.as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{digest, to_json, Artifact, Shared};
    use crate::ir::Schema;
    use crate::json;
    use crate::model::{Field, Model};

    fn schema() -> Schema {
        Schema::build(&[Model {
            name: "Player".to_owned(),
            source: PathBuf::from("src/models/player.rs"),
            line: 1,
            codecs: vec!["edge".to_owned()],
            fields: vec![Field {
                name: "id".to_owned(),
                network_type: "u32".to_owned(),
                codecs: vec!["edge".to_owned()],
                line: 2,
            }],
        }])
        .expect("build")
    }

    #[test]
    fn a_source_maps_to_the_files_generated_from_it() {
        let schema = schema();
        let message = schema.message("Player.edge").expect("message");

        let text = to_json(
            &schema,
            &[Artifact {
                path: "generated/models/player/player.edge.rs".to_owned(),
                source: "src/models/player.rs".to_owned(),
                model: "Player".to_owned(),
                codec: "edge".to_owned(),
                fingerprint: message.fingerprint,
                sha256: digest("codec"),
            }],
            &[Shared {
                path: "generated/runtime.rs".to_owned(),
                sha256: digest("runtime"),
                kind: "runtime",
            }],
        );

        let document = json::parse(&text).expect("parse");
        let source = document
            .get("sources")
            .and_then(|sources| sources.get("src/models/player.rs"))
            .expect("source entry");

        assert_eq!(
            source
                .get("models")
                .and_then(json::Json::as_array)
                .and_then(|models| models[0].as_str()),
            Some("Player")
        );
        let output = &source
            .get("outputs")
            .and_then(json::Json::as_array)
            .expect("outputs")[0];
        assert_eq!(
            output.get("path").and_then(json::Json::as_str),
            Some("generated/models/player/player.edge.rs")
        );
        assert_eq!(
            output.get("codec").and_then(json::Json::as_str),
            Some("edge")
        );
        assert!(output
            .get("fingerprint")
            .and_then(json::Json::as_str)
            .is_some_and(|text| text.starts_with("sha256:")));
    }

    #[test]
    fn the_graph_is_byte_stable() {
        let schema = schema();
        assert_eq!(to_json(&schema, &[], &[]), to_json(&schema, &[], &[]));
    }

    #[test]
    fn a_digest_ignores_the_timestamp_and_only_the_timestamp() {
        let monday = "// GENERATED BY cyclonec\n// generated-at: 2026-01-01T00:00:00Z\nbody\n";
        let friday = "// GENERATED BY cyclonec\n// generated-at: 2030-06-06T12:00:00Z\nbody\n";
        let edited = "// GENERATED BY cyclonec\n// generated-at: 2026-01-01T00:00:00Z\nedited\n";

        assert_eq!(digest(monday), digest(friday));
        assert_ne!(digest(monday), digest(edited));
    }
}
