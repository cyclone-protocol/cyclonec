//! `.cyclone/build-graph.json` - which source produced which file.
//!
//! ```text
//! source model
//!     ↓
//! generated codec files
//! ```
//!
//! Two questions it exists to answer, both of which are otherwise guesswork:
//!
//! *Where did this file come from?* A generated file names its source in its
//! own header, but only one file at a time and only if it is still there. The
//! graph answers it for the tree, including for files whose source has since
//! been deleted - which is exactly when the question gets asked.
//!
//! *Is this file stale?* Each entry carries the SHA-256 of the bytes
//! `cyclonec` wrote. A file whose digest no longer matches was edited by hand
//! (the header did say not to), and one that is missing was deleted; either way
//! the build cannot trust it. Nothing else in the project stores that, because
//! the generated files themselves cannot: a file cannot contain its own digest.

use std::collections::BTreeMap;

use crate::fingerprint::Fingerprint;
use crate::ir::Schema;
use crate::json::Json;

/// The path the build graph is written to, relative to the project root.
pub const PATH: &str = ".cyclone/build-graph.json";

/// One generated codec file.
pub struct Artifact {
    /// Where it was written, as the command line spelled the output directory.
    pub path: String,
    /// The source file the model was read from.
    pub source: String,
    pub model: String,
    pub codec: String,
    /// The fingerprint of the message it encodes.
    pub fingerprint: Fingerprint,
    /// SHA-256 of the file's bytes, as written.
    pub sha256: String,
}

/// One generated file that belongs to no single model: the runtime, the
/// handshake constants, the root include.
pub struct Shared {
    pub path: String,
    pub sha256: String,
    /// What it is, in one word: `runtime`, `handshake`, `root`.
    pub kind: &'static str,
}

/// Renders the build graph.
pub fn to_json(schema: &Schema, artifacts: &[Artifact], shared: &[Shared]) -> String {
    // Grouped by source, so the file reads the way the pipeline runs.
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

/// The digest a build-graph entry carries, for text about to be written.
///
/// Taken over the file with its `// generated-at:` line emptied out, so that
/// two runs of an unchanged schema produce the same digest and the graph does
/// not churn on every build. A hand edit anywhere else still changes it, which
/// is what the digest is for.
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

    /// The digest survives a run happening at a different time, and nothing
    /// else.
    #[test]
    fn a_digest_ignores_the_timestamp_and_only_the_timestamp() {
        let monday = "// GENERATED BY cyclonec\n// generated-at: 2026-01-01T00:00:00Z\nbody\n";
        let friday = "// GENERATED BY cyclonec\n// generated-at: 2030-06-06T12:00:00Z\nbody\n";
        let edited = "// GENERATED BY cyclonec\n// generated-at: 2026-01-01T00:00:00Z\nedited\n";

        assert_eq!(digest(monday), digest(friday));
        assert_ne!(digest(monday), digest(edited));
    }
}
