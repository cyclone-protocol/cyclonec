//! Cross-language fingerprint compatibility (issue.md §11 / RFC-0002's own
//! promise): the same model definition, written in different source
//! languages, must produce the same Cyclone fingerprint. Nothing in
//! `cyclonec` computes a fingerprint from anything language-specific - see
//! `src/fingerprint.rs`'s module docs - so this is a property of the IR, not
//! something any one backend has to arrange; these tests exist to prove it
//! rather than assume it.
//!
//! Field names are hashed (`src/fingerprint.rs` explains why), but in their
//! canonical spelling, so each language may write the same field its own way:
//! Rust's `player_id`, Go's `PlayerID` and C#'s `PlayerId` are one field and
//! one fingerprint. That is what
//! [`each_language_may_spell_a_field_its_own_way`] pins down, and it is the
//! reason the older tests below - which spell every field identically in every
//! language - are no longer the only fair comparison available.

use std::path::Path;

use cyclonec::ir::Schema;
use cyclonec::parser;

fn build(path: &str, text: &str) -> Schema {
    let models = parser::parse(Path::new(path), text).expect("parse");
    Schema::build(&models).expect("build")
}

/// The exact pair issue.md §11 gives as its own worked example.
#[test]
fn the_brief_s_worked_example_fingerprints_identically_in_rust_and_typescript() {
    let rust = build(
        "device_state.rs",
        "#[network]\n#[codec(edge)]\nstruct DeviceState {\n\
         \t#[network(u32)]\n\t#[codec(edge)]\n\tId: u32,\n\n\
         \t#[network(f32)]\n\t#[codec(edge)]\n\tTemperature: f32,\n\n\
         \t#[network(string)]\n\t#[codec(edge)]\n\tDisplayName: String,\n}\n",
    );

    let typescript = build(
        "device_state.ts",
        "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass DeviceState {\n\
         \t// CYCLONE_FIELD(u32)\n\t// CYCLONE_CODEC(\"edge\")\n\tId: number = 0;\n\n\
         \t// CYCLONE_FIELD(f32)\n\t// CYCLONE_CODEC(\"edge\")\n\tTemperature: number = 0;\n\n\
         \t// CYCLONE_FIELD(string)\n\t// CYCLONE_CODEC(\"edge\")\n\tDisplayName: string = \"\";\n}\n",
    );

    assert_eq!(
        rust.fingerprint, typescript.fingerprint,
        "the whole schema must fingerprint identically"
    );

    let rust_model = rust.model("DeviceState").expect("Rust model");
    let ts_model = typescript.model("DeviceState").expect("TypeScript model");
    assert_eq!(
        rust_model.fingerprint, ts_model.fingerprint,
        "the model's own declaration fingerprint must match"
    );

    let rust_message = rust.message("DeviceState.edge").expect("Rust message");
    let ts_message = typescript.message("DeviceState.edge").expect("TS message");
    assert_eq!(
        rust_message.fingerprint, ts_message.fingerprint,
        "the wire contract fingerprint must match"
    );
    assert_eq!(
        rust_message.id, ts_message.id,
        "the message id, derived from the name alone, must match"
    );
}

/// Every language this generator reads, compared against the same schema -
/// the general case the worked example above is one instance of. Every
/// primitive, an array, and a nested model, so a mismatch anywhere in the
/// wire-type table would show up here.
#[test]
fn every_backend_reads_the_same_schema_into_the_same_fingerprints() {
    let rust = build(
        "models.rs",
        "#[network]\n#[codec(edge)]\nstruct Info {\n\
         \t#[network(u32)]\n\t#[codec(edge)]\n\tLevel: u32,\n}\n\n\
         #[network]\n#[codec(edge)]\nstruct Widget {\n\
         \t#[network(u32)]\n\t#[codec(edge)]\n\tId: u32,\n\n\
         \t#[network(string)]\n\t#[codec(edge)]\n\tName: String,\n\n\
         \t#[network(Array<u32>)]\n\t#[codec(edge)]\n\tScores: Vec<u32>,\n\n\
         \t#[network(Info)]\n\t#[codec(edge)]\n\tOwner: Info,\n}\n",
    );

    let typescript = build(
        "models.ts",
        "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass Info {\n\
         \t// CYCLONE_FIELD(u32)\n\t// CYCLONE_CODEC(\"edge\")\n\tLevel: number = 0;\n}\n\n\
         // CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass Widget {\n\
         \t// CYCLONE_FIELD(u32)\n\t// CYCLONE_CODEC(\"edge\")\n\tId: number = 0;\n\n\
         \t// CYCLONE_FIELD(string)\n\t// CYCLONE_CODEC(\"edge\")\n\tName: string = \"\";\n\n\
         \t// CYCLONE_FIELD(Array<u32>)\n\t// CYCLONE_CODEC(\"edge\")\n\tScores: number[] = [];\n\n\
         \t// CYCLONE_FIELD(Info)\n\t// CYCLONE_CODEC(\"edge\")\n\tOwner: Info = new Info();\n}\n",
    );

    let javascript = build(
        "models.js",
        "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass Info {\n\
         \t// CYCLONE_FIELD(u32)\n\t// CYCLONE_CODEC(\"edge\")\n\tLevel = 0;\n}\n\n\
         // CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass Widget {\n\
         \t// CYCLONE_FIELD(u32)\n\t// CYCLONE_CODEC(\"edge\")\n\tId = 0;\n\n\
         \t// CYCLONE_FIELD(string)\n\t// CYCLONE_CODEC(\"edge\")\n\tName = \"\";\n\n\
         \t// CYCLONE_FIELD(Array<u32>)\n\t// CYCLONE_CODEC(\"edge\")\n\tScores = [];\n\n\
         \t// CYCLONE_FIELD(Info)\n\t// CYCLONE_CODEC(\"edge\")\n\tOwner = new Info();\n}\n",
    );

    let go = build(
        "models.go",
        "package models\n\n//cyclone:model codec=edge\ntype Info struct {\n\
         \tLevel uint32 `cyclone:\"u32\" codec:\"edge\"`\n}\n\n\
         //cyclone:model codec=edge\ntype Widget struct {\n\
         \tId uint32 `cyclone:\"u32\" codec:\"edge\"`\n\
         \tName string `cyclone:\"string\" codec:\"edge\"`\n\
         \tScores []uint32 `cyclone:\"Array<u32>\" codec:\"edge\"`\n\
         \tOwner Info `cyclone:\"Info\" codec:\"edge\"`\n}\n",
    );

    let csharp = build(
        "Models.cs",
        "[Network]\n[Codec(\"edge\")]\npublic class Info {\n\
         \t[Network(\"u32\")]\n\t[Codec(\"edge\")]\n\tpublic uint Level;\n}\n\n\
         [Network]\n[Codec(\"edge\")]\npublic class Widget {\n\
         \t[Network(\"u32\")]\n\t[Codec(\"edge\")]\n\tpublic uint Id;\n\
         \t[Network(\"string\")]\n\t[Codec(\"edge\")]\n\tpublic string Name;\n\
         \t[Network(\"Array<u32>\")]\n\t[Codec(\"edge\")]\n\tpublic uint[] Scores;\n\
         \t[Network(\"Info\")]\n\t[Codec(\"edge\")]\n\tpublic Info Owner;\n}\n",
    );

    let others: [(&str, &Schema); 4] = [
        ("TypeScript", &typescript),
        ("JavaScript", &javascript),
        ("Go", &go),
        ("C#", &csharp),
    ];

    for (name, other) in others {
        assert_eq!(
            rust.fingerprint, other.fingerprint,
            "{name}'s schema fingerprint must match Rust's"
        );

        let rust_widget = rust.model("Widget").expect("Rust Widget");
        let other_widget = other.model("Widget").expect("Widget");
        assert_eq!(
            rust_widget.fingerprint, other_widget.fingerprint,
            "{name}'s Widget fingerprint must match Rust's"
        );

        let rust_message = rust.message("Widget.edge").expect("Rust message");
        let other_message = other.message("Widget.edge").expect("message");
        assert_eq!(
            rust_message.fingerprint, other_message.fingerprint,
            "{name}'s Widget.edge fingerprint must match Rust's - a nested model, an array \
             and every other primitive all resolve to the same wire type"
        );
        assert_eq!(
            rust_message.id, other_message.id,
            "{name}'s message id must match Rust's"
        );
    }
}

/// The case `cyclone-fingerprint/2` exists for: one model, written the way
/// each language's own convention would write it, is one fingerprint.
///
/// `cyclonec` reads one language per run (see the README), so a project with a
/// Rust server and a Go client has two annotated sources, each idiomatic. Under
/// `/1` those two hashed differently and the handshake said `Reject` about two
/// peers whose bytes were identical - which is a naming convention wearing a
/// schema disagreement's clothes.
#[test]
fn each_language_may_spell_a_field_its_own_way() {
    let rust = build(
        "player.rs",
        "#[network]\n#[codec(edge)]\nstruct Player {\n\
         \t#[network(u32)]\n\t#[codec(edge)]\n\tplayer_id: u32,\n\n\
         \t#[network(f32)]\n\t#[codec(edge)]\n\tposition_x: f32,\n\n\
         \t#[network(string)]\n\t#[codec(edge)]\n\tdisplay_name: String,\n}\n",
    );

    let go = build(
        "player.go",
        "package models\n\n//cyclone:model codec=edge\ntype Player struct {\n\
         \tPlayerID uint32 `cyclone:\"u32\" codec:\"edge\"`\n\
         \tPositionX float32 `cyclone:\"f32\" codec:\"edge\"`\n\
         \tDisplayName string `cyclone:\"string\" codec:\"edge\"`\n}\n",
    );

    let csharp = build(
        "Player.cs",
        "[Network]\n[Codec(\"edge\")]\npublic class Player {\n\
         \t[Network(\"u32\")]\n\t[Codec(\"edge\")]\n\tpublic uint PlayerId;\n\
         \t[Network(\"f32\")]\n\t[Codec(\"edge\")]\n\tpublic float PositionX;\n\
         \t[Network(\"string\")]\n\t[Codec(\"edge\")]\n\tpublic string DisplayName;\n}\n",
    );

    let typescript = build(
        "player.ts",
        "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass Player {\n\
         \t// CYCLONE_FIELD(u32)\n\t// CYCLONE_CODEC(\"edge\")\n\tplayerId: number = 0;\n\n\
         \t// CYCLONE_FIELD(f32)\n\t// CYCLONE_CODEC(\"edge\")\n\tpositionX: number = 0;\n\n\
         \t// CYCLONE_FIELD(string)\n\t// CYCLONE_CODEC(\"edge\")\n\tdisplayName: string = \"\";\n}\n",
    );

    for (name, other) in [("Go", &go), ("C#", &csharp), ("TypeScript", &typescript)] {
        assert_eq!(
            rust.fingerprint, other.fingerprint,
            "{name} spells the same schema its own way; the fingerprint must not notice"
        );
        assert_eq!(
            rust.message("Player.edge").expect("Rust").fingerprint,
            other.message("Player.edge").expect("other").fingerprint,
            "{name}'s Player.edge must match Rust's"
        );
    }
}

/// The property canonicalising must not cost: a rename a human meant is still
/// a different schema, and two same-typed fields swapped is still a mismatch.
#[test]
fn canonicalising_does_not_blind_the_fingerprint() {
    let base = build(
        "player.rs",
        "#[network]\n#[codec(edge)]\nstruct Player {\n\
         \t#[network(f32)]\n\t#[codec(edge)]\n\tx: f32,\n\n\
         \t#[network(f32)]\n\t#[codec(edge)]\n\ty: f32,\n}\n",
    );

    let renamed = build(
        "player.rs",
        "#[network]\n#[codec(edge)]\nstruct Player {\n\
         \t#[network(f32)]\n\t#[codec(edge)]\n\tposition_x: f32,\n\n\
         \t#[network(f32)]\n\t#[codec(edge)]\n\ty: f32,\n}\n",
    );

    let swapped = build(
        "player.rs",
        "#[network]\n#[codec(edge)]\nstruct Player {\n\
         \t#[network(f32)]\n\t#[codec(edge)]\n\ty: f32,\n\n\
         \t#[network(f32)]\n\t#[codec(edge)]\n\tx: f32,\n}\n",
    );

    assert_ne!(base.fingerprint, renamed.fingerprint, "a real rename");
    assert_ne!(base.fingerprint, swapped.fingerprint, "a reorder");
}
