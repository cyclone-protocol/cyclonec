//! One message → one JavaScript file.
//!
//! The JavaScript counterpart of [`super::typescript`] - see its module docs
//! for the reasoning this backend shares in full: the same runtime calls,
//! the same "construct a nested model with `new` if it is not there yet"
//! rule, the same RFC-0002 §9.1 decode policy, the same refusal of
//! `Array<Array<T>>`. The only difference is the surface syntax: no type
//! annotations, `@param`/`@returns` JSDoc in their place, and `.js` instead
//! of `.ts` - the wire format the two backends emit is identical (checked in
//! `tests/cli.rs`), because both are the same walk over the same
//! [`crate::ir`] message.
//!
//! # What JavaScript does not need imported
//!
//! Unlike [`super::typescript`], a generated file here never spells `Writer`
//! or `Reader` by name - a JS function parameter carries no type, so nothing
//! ever names those classes outside a comment - and the model a codec
//! encodes and decodes is imported only if this file actually constructs one
//! with `new` (a nested model field, or an array of them); the model a codec
//! *belongs to* is never constructed in its own file and so is never
//! imported at all.

use std::collections::{BTreeMap, BTreeSet};

use crate::ir::{Field, Message, Model, WireType};
use crate::model::snake_case;

use super::codec_type_name;

/// The runtime method each primitive maps to. Unlike
/// [`super::typescript::primitive`] this carries no host type - JavaScript
/// has none to spell - only what [`super::typescript_runtime`] (identical
/// method names) calls it.
fn primitive(ty: &WireType) -> Option<(&'static str, &'static str)> {
    Some(match ty {
        WireType::Bool => ("writeBool", "readBool"),
        WireType::I8 => ("writeI8", "readI8"),
        WireType::U8 => ("writeU8", "readU8"),
        WireType::I16 => ("writeI16", "readI16"),
        WireType::U16 => ("writeU16", "readU16"),
        WireType::I32 => ("writeI32", "readI32"),
        WireType::U32 => ("writeU32", "readU32"),
        WireType::I64 => ("writeI64", "readI64"),
        WireType::U64 => ("writeU64", "readU64"),
        WireType::F32 => ("writeF32", "readF32"),
        WireType::F64 => ("writeF64", "readF64"),
        WireType::Str => ("writeString", "readString"),
        WireType::Bytes => ("writeBytes", "readBytes"),
        WireType::Array(_) | WireType::Model(_) => return None,
    })
}

/// The JSDoc `@param`/`@returns` type spelling for a wire type - the closest
/// JavaScript comes to [`super::typescript::primitive`]'s host type column.
fn jsdoc_type(ty: &WireType) -> String {
    match ty {
        WireType::Bool => "boolean".to_owned(),
        WireType::I8
        | WireType::U8
        | WireType::I16
        | WireType::U16
        | WireType::I32
        | WireType::U32
        | WireType::F32
        | WireType::F64 => "number".to_owned(),
        WireType::I64 | WireType::U64 => "bigint".to_owned(),
        WireType::Str => "string".to_owned(),
        WireType::Bytes => "Uint8Array".to_owned(),
        WireType::Array(element) => format!("{}[]", jsdoc_type(element)),
        WireType::Model(name) => name.clone(),
    }
}

/// The JavaScript zero-value expression for a field the stream ended before
/// (RFC-0002 §9.1). Never called for [`WireType::Array`] or
/// [`WireType::Model`] - see [`super::typescript::zero`], which this mirrors
/// exactly.
fn zero(ty: &WireType) -> &'static str {
    match ty {
        WireType::Bool => "false",
        WireType::Str => "\"\"",
        WireType::Bytes => "new Uint8Array(0)",
        WireType::I64 | WireType::U64 => "0n",
        WireType::Array(_) => unreachable!("an array's absence is handled by decode_field"),
        WireType::Model(_) => unreachable!("a model field is decoded through its own codec"),
        _ => "0",
    }
}

/// The generated file name: `Player` + `edge` → `player_edge.js`.
pub fn file_name(model: &str, codec: &str) -> String {
    format!("{}_{}.js", snake_case(model), snake_case(codec))
}

fn module_stem(model: &str, codec: &str) -> String {
    format!("{}_{}", snake_case(model), snake_case(codec))
}

/// Where one model's own class is declared - see [`super::typescript::ModelLocation`].
pub struct ModelLocation {
    pub specifier: String,
}

/// Where every model this run parsed can be reached from inside a generated
/// JavaScript file.
pub struct Imports<'a> {
    pub locations: &'a BTreeMap<String, ModelLocation>,
}

/// Refuses `Array<Array<T>>` before any text is generated - see
/// [`super::typescript::check_no_nested_arrays`], which this mirrors exactly.
///
/// # Errors
///
/// A field whose type nests one `Array` inside another.
pub fn check_no_nested_arrays(model: &Model) -> Result<(), String> {
    for field in &model.fields {
        if let WireType::Array(element) = &field.ty {
            if matches!(element.as_ref(), WireType::Array(_)) {
                return Err(format!(
                    "model '{}' field '{}': the JavaScript backend does not support \
                     `Array<Array<T>>` - split '{}' into two codecs, or flatten the field",
                    model.name, field.name, field.name,
                ));
            }
        }
    }
    Ok(())
}

/// Renders one codec file: the header, its imports, the codec class, its
/// constants, and its `encode`/`decode`.
pub fn codec_file(model: &Model, message: &Message, imports: &Imports<'_>) -> String {
    let mut out = super::Header {
        source: Some(&model.source),
        model: Some(&model.name),
        codec: Some(&message.codec),
        fingerprint: Some(message.fingerprint.tagged()),
        note: None,
    }
    .render();

    write_imports(&mut out, model, message, imports);

    let name = codec_type_name(&model.name, &message.codec);

    out.push_str(&format!(
        "/**\n * The `{}` codec for {{@link {}}}, generated from its Cyclone attributes.\n",
        message.codec, model.name
    ));
    if message.fields.is_empty() {
        out.push_str(" *\n * This codec carries no fields: it encodes to zero bytes.\n */\n");
    } else {
        out.push_str(" *\n * The wire layout, in declaration order (RFC-0002 §5.1):\n *\n");
        for (index, field) in message.fields.iter().enumerate() {
            out.push_str(&format!(
                " *  {index}. `{}`: `{}`\n",
                field.name,
                field.ty.spelling()
            ));
        }
        out.push_str(" */\n");
    }
    out.push_str(&format!("export class {name} {{\n"));

    out.push_str("    /** This message's name: `Model.codec`. */\n");
    out.push_str(&format!(
        "    static MESSAGE_NAME = \"{}\";\n\n",
        message.name
    ));
    out.push_str("    /** This message's stable id, derived from its name alone. */\n");
    out.push_str(&format!(
        "    static MESSAGE_ID = 0x{:08X};\n\n",
        message.id
    ));
    out.push_str(
        "    /**\n     * This message's wire-contract fingerprint - the same value\n     \
         * `handshake.js` publishes, and the one a peer compares against.\n     */\n",
    );
    out.push_str(&format!(
        "    static FINGERPRINT = {}n;\n\n",
        crate::schema::hex64(message.fingerprint.u64())
    ));

    // -------------------------------------------------------------- encode
    out.push_str(&format!(
        "    /**\n     * Writes the `{}` fields of `value`, in declaration order.\n     *\n     \
         * @param {{Writer}} writer\n     * @param {{{}}} value\n     */\n",
        message.codec, model.name
    ));
    out.push_str("    static encode(writer, value) {\n");
    for field in &message.fields {
        encode_field(&mut out, field, &message.codec);
    }
    out.push_str("    }\n\n");

    // -------------------------------------------------------------- decode
    out.push_str(&format!(
        "    /**\n     * Reads the `{}` fields into `value`, in declaration order.\n     *\n",
        message.codec
    ));
    out.push_str(
        "     * Fields this codec does not carry are left as they were, which is what lets\n     \
         * one model be split across several codecs.\n     *\n     \
         * A field the stream ended before takes its zero value (RFC-0002 §9.1); a field\n     \
         * the stream ended inside throws. Bytes left over after the last field belong to\n     \
         * a newer writer's model and are ignored.\n     *\n",
    );
    out.push_str(&format!(
        "     * @param {{Reader}} reader\n     * @param {{{}}} value\n     */\n",
        model.name
    ));
    out.push_str("    static decode(reader, value) {\n");
    for field in &message.fields {
        decode_field(&mut out, field, &message.codec);
    }
    out.push_str("    }\n");
    out.push_str("}\n");

    out
}

/// The `import` block at the top of a codec file - see
/// [`super::typescript::write_imports`], which this mirrors except that the
/// model a codec belongs to is imported only if a `new` of it actually
/// appears in this file (see the module docs).
fn write_imports(out: &mut String, model: &Model, message: &Message, imports: &Imports<'_>) {
    let mut groups: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();

    // Every model this file constructs with `new` - a bare nested-model
    // field, or an array of them.
    let mut constructed: BTreeSet<&str> = BTreeSet::new();
    for field in &message.fields {
        match &field.ty {
            WireType::Model(name) => {
                constructed.insert(name);
            }
            WireType::Array(element) => {
                if let WireType::Model(name) = element.as_ref() {
                    constructed.insert(name);
                }
            }
            _ => {}
        }
    }
    for name in constructed {
        if let Some(location) = imports.locations.get(name) {
            groups
                .entry(location.specifier.as_str())
                .or_default()
                .insert(name.to_owned());
        }
    }

    // The codecs this one calls for its nested models - bare fields and
    // array elements alike.
    let mut codec_specifiers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for field in &message.fields {
        let Some(name) = field.ty.model_name() else {
            continue;
        };
        if name == model.name || !imports.locations.contains_key(name) {
            continue;
        }
        codec_specifiers
            .entry(format!("./{}", module_stem(name, &message.codec)))
            .or_default()
            .insert(codec_type_name(name, &message.codec));
    }

    // Every specifier ends in `.js`: unlike `super::typescript`'s output,
    // this file is run directly - by Node's ESM loader, or by a browser's -
    // neither of which resolves an extensionless relative specifier the way
    // a bundler or `tsc` would.
    for (specifier, names) in &groups {
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        out.push_str(&format!(
            "import {{ {} }} from \"{specifier}.js\";\n",
            names.join(", ")
        ));
    }
    for (specifier, names) in &codec_specifiers {
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        out.push_str(&format!(
            "import {{ {} }} from \"{specifier}.js\";\n",
            names.join(", ")
        ));
    }
    if !groups.is_empty() || !codec_specifiers.is_empty() {
        out.push('\n');
    }
}

// =================================================================== encoding

fn encode_field(out: &mut String, field: &Field, codec: &str) {
    let place = format!("value.{}", field.name);

    if let WireType::Array(element_type) = &field.ty {
        out.push_str(&format!(
            "        writer.writeArrayCount({place}.length);\n"
        ));
        out.push_str(&format!("        for (const element of {place}) {{\n"));
        encode_scalar(out, element_type, "element", codec, "            ");
        out.push_str("        }\n");
        return;
    }

    encode_scalar(out, &field.ty, &place, codec, "        ");
}

fn encode_scalar(out: &mut String, ty: &WireType, place: &str, codec: &str, pad: &str) {
    match primitive(ty) {
        Some((writer_method, ..)) => {
            out.push_str(&format!("{pad}writer.{writer_method}({place});\n"));
        }
        None => {
            let nested = codec_type_name(
                ty.model_name().expect("a non-primitive, non-array type"),
                codec,
            );
            out.push_str(&format!("{pad}{nested}.encode(writer, {place});\n"));
        }
    }
}

// =================================================================== decoding

fn decode_field(out: &mut String, field: &Field, codec: &str) {
    let place = format!("value.{}", field.name);

    if let Some(name) = as_model(&field.ty) {
        let nested = codec_type_name(name, codec);
        out.push_str(&format!(
            "        if ({place} === undefined || {place} === null) {{\n"
        ));
        out.push_str(&format!("            {place} = new {name}();\n"));
        out.push_str("        }\n");
        out.push_str(&format!("        {nested}.decode(reader, {place});\n"));
        return;
    }

    if let WireType::Array(element_type) = &field.ty {
        out.push_str("        {\n");
        out.push_str(
            "            const count = reader.fieldAbsent() ? 0 : reader.readArrayCount();\n",
        );
        out.push_str(&format!(
            "            /** @type {{{}}} */\n            const elements = [];\n",
            jsdoc_type(&field.ty)
        ));
        out.push_str("            for (let i = 0; i < count; i++) {\n");
        decode_element_into(out, element_type, "elements", codec, "                ");
        out.push_str("            }\n");
        out.push_str(&format!("            {place} = elements;\n"));
        out.push_str("        }\n");
        return;
    }

    let (_, reader_method) = primitive(&field.ty).expect("models and arrays handled above");
    out.push_str(&format!(
        "        {place} = reader.fieldAbsent() ? {} : reader.{reader_method}();\n",
        zero(&field.ty),
    ));
}

fn decode_element_into(out: &mut String, ty: &WireType, list: &str, codec: &str, pad: &str) {
    match as_model(ty) {
        Some(name) => {
            let nested = codec_type_name(name, codec);
            out.push_str(&format!("{pad}const element = new {name}();\n"));
            out.push_str(&format!("{pad}{nested}.decode(reader, element);\n"));
            out.push_str(&format!("{pad}{list}.push(element);\n"));
        }
        None => {
            let (_, reader_method) = primitive(ty).expect("models handled above");
            out.push_str(&format!("{pad}{list}.push(reader.{reader_method}());\n"));
        }
    }
}

fn as_model(ty: &WireType) -> Option<&str> {
    match ty {
        WireType::Model(name) => Some(name),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::{
        check_no_nested_arrays, codec_file, file_name, jsdoc_type, Imports, ModelLocation,
    };
    use crate::ir::{Schema, WireType};
    use crate::model::{Field, Model};

    fn generated(fields: &[(&str, &str)]) -> String {
        let schema = Schema::build(&[
            Model {
                name: "Player".to_owned(),
                source: PathBuf::from("src/models/player.js"),
                line: 1,
                codecs: vec!["edge".to_owned()],
                fields: fields
                    .iter()
                    .map(|(name, ty)| Field {
                        name: (*name).to_owned(),
                        network_type: (*ty).to_owned(),
                        codecs: vec!["edge".to_owned()],
                        line: 1,
                    })
                    .collect(),
            },
            Model {
                name: "PlayerInfo".to_owned(),
                source: PathBuf::from("src/models/player.js"),
                line: 20,
                codecs: vec!["edge".to_owned()],
                fields: vec![Field {
                    name: "Level".to_owned(),
                    network_type: "u32".to_owned(),
                    codecs: vec!["edge".to_owned()],
                    line: 21,
                }],
            },
        ])
        .expect("build");

        let locations: BTreeMap<String, ModelLocation> = ["Player", "PlayerInfo"]
            .into_iter()
            .map(|name| {
                (
                    name.to_owned(),
                    ModelLocation {
                        specifier: "../src/models/player".to_owned(),
                    },
                )
            })
            .collect();
        let model = schema.model("Player").expect("model");
        codec_file(
            model,
            &model.messages[0],
            &Imports {
                locations: &locations,
            },
        )
    }

    #[test]
    fn a_primitive_reads_and_writes_the_model_directly() {
        let text = generated(&[("Id", "u32")]);
        assert!(text.contains("writer.writeU32(value.Id);"), "{text}");
        assert!(
            text.contains("value.Id = reader.fieldAbsent() ? 0 : reader.readU32();"),
            "{text}"
        );
    }

    #[test]
    fn no_type_annotations_no_dto_no_mapper() {
        let text = generated(&[("Id", "u32"), ("Name", "string")]);
        assert!(text.contains("static encode(writer, value) {"), "{text}");
        assert!(text.contains("static decode(reader, value) {"), "{text}");
        for forbidden in [
            "PlayerDTO",
            "PlayerWire",
            "PlayerMapper",
            "writer: Writer",
            "value: Player",
        ] {
            assert!(!text.contains(forbidden), "{forbidden} in\n{text}");
        }
    }

    #[test]
    fn the_top_level_model_is_never_imported() {
        // Nothing in Player's own edge codec constructs a `Player`, and a
        // JS function parameter carries no type - so unlike TypeScript,
        // nothing here ever needs to import it.
        let text = generated(&[("Id", "u32")]);
        assert!(!text.contains("import { Player"), "{text}");
    }

    #[test]
    fn a_nested_model_is_imported_because_it_is_constructed() {
        let text = generated(&[("Info", "PlayerInfo")]);
        assert!(
            text.contains("import { PlayerInfo } from \"../src/models/player.js\";\n"),
            "{text}"
        );
        assert!(
            text.contains("import { PlayerInfoEdgeCodec } from \"./player_info_edge.js\";\n"),
            "{text}"
        );
        assert!(
            text.contains("if (value.Info === undefined || value.Info === null) {"),
            "{text}"
        );
        assert!(text.contains("value.Info = new PlayerInfo();"), "{text}");
    }

    #[test]
    fn an_array_counts_first_then_loops_strictly() {
        let text = generated(&[("Tags", "Array<string>")]);
        assert!(
            text.contains("writer.writeArrayCount(value.Tags.length);"),
            "{text}"
        );
        assert!(text.contains("const elements = [];"), "{text}");
        assert!(
            text.contains("elements.push(reader.readString());"),
            "{text}"
        );
    }

    #[test]
    fn a_64_bit_field_is_a_bigint_literal_zero() {
        let text = generated(&[("Seq", "u64")]);
        assert!(
            text.contains("value.Seq = reader.fieldAbsent() ? 0n : reader.readU64();"),
            "{text}"
        );
    }

    #[test]
    fn nested_arrays_are_refused_rather_than_generated_wrong() {
        let model = Model {
            name: "Grid".to_owned(),
            source: PathBuf::from("src/models/grid.js"),
            line: 1,
            codecs: vec!["edge".to_owned()],
            fields: vec![Field {
                name: "Rows".to_owned(),
                network_type: "Array<Array<u8>>".to_owned(),
                codecs: vec!["edge".to_owned()],
                line: 2,
            }],
        };
        let schema = Schema::build(&[model]).expect("build");
        let error = check_no_nested_arrays(&schema.models[0]).expect_err("refused");
        assert!(error.contains("Array<Array<T>>"), "{error}");
    }

    #[test]
    fn a_codec_file_is_named_like_the_other_backends() {
        assert_eq!(file_name("Player", "edge"), "player_edge.js");
    }

    #[test]
    fn jsdoc_types_spell_arrays_and_models() {
        assert_eq!(jsdoc_type(&WireType::U32), "number");
        assert_eq!(jsdoc_type(&WireType::U64), "bigint");
        assert_eq!(
            jsdoc_type(&WireType::Array(Box::new(WireType::Str))),
            "string[]"
        );
        assert_eq!(jsdoc_type(&WireType::Model("Player".to_owned())), "Player");
    }

    #[test]
    fn the_file_carries_the_headers_the_brief_asks_for() {
        let text = generated(&[("Id", "u32")]);
        assert!(
            text.starts_with("// GENERATED BY cyclonec\n// DO NOT EDIT MANUALLY\n"),
            "{text}"
        );
        assert!(text.contains("// source: src/models/player.js\n"), "{text}");
        assert!(text.contains("// fingerprint: sha256:"), "{text}");
    }

    #[test]
    fn the_constants_are_generated_not_hand_written() {
        let text = generated(&[("Id", "u32")]);
        assert!(
            text.contains("static MESSAGE_NAME = \"Player.edge\";"),
            "{text}"
        );
        assert!(text.contains("static MESSAGE_ID = 0x"), "{text}");
        assert!(text.contains("static FINGERPRINT = 0x"), "{text}");
        assert!(
            text.contains("n;"),
            "fingerprint is a bigint literal:\n{text}"
        );
    }
}
