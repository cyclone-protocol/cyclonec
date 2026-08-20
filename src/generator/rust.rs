use crate::ir::{Field, Message, Model, WireType};
use crate::model::snake_case;

fn primitive(ty: &WireType) -> Option<(&'static str, &'static str, bool)> {
    Some(match ty {
        WireType::Bool => ("write_bool", "read_bool", false),
        WireType::I8 => ("write_i8", "read_i8", false),
        WireType::U8 => ("write_u8", "read_u8", false),
        WireType::I16 => ("write_i16", "read_i16", false),
        WireType::U16 => ("write_u16", "read_u16", false),
        WireType::I32 => ("write_i32", "read_i32", false),
        WireType::U32 => ("write_u32", "read_u32", false),
        WireType::I64 => ("write_i64", "read_i64", false),
        WireType::U64 => ("write_u64", "read_u64", false),
        WireType::F32 => ("write_f32", "read_f32", false),
        WireType::F64 => ("write_f64", "read_f64", false),
        WireType::Str => ("write_string", "read_string", true),
        WireType::Bytes => ("write_bytes", "read_bytes", true),
        WireType::Array(_) | WireType::Model(_) => return None,
    })
}

fn zero(ty: &WireType) -> String {
    match ty {
        WireType::Bool => "false".to_owned(),
        WireType::F32 => "0.0f32".to_owned(),
        WireType::F64 => "0.0f64".to_owned(),
        WireType::Str => "::std::string::String::new()".to_owned(),
        WireType::Bytes | WireType::Array(_) => "::std::vec::Vec::new()".to_owned(),
        WireType::Model(_) => "::core::default::Default::default()".to_owned(),
        integer => format!("0{}", integer.spelling()),
    }
}

pub use super::codec_type_name;

pub fn module_name(model: &str, codec: &str) -> String {
    format!("{}_{}", snake_case(model), snake_case(codec))
}

pub fn file_name(model: &str, codec: &str) -> String {
    format!("{}.rs", module_name(model, codec))
}

pub struct Imports<'a> {
    pub types: &'a std::collections::BTreeMap<String, String>,
    pub root: &'a str,
}

pub fn codec_file(model: &Model, message: &Message, imports: &Imports<'_>) -> String {
    let mut out = super::Header {
        source: Some(&model.source),
        model: Some(&model.name),
        codec: Some(&message.codec),
        fingerprint: Some(message.fingerprint.tagged()),
        note: None,
    }
    .render();
    out.push_str(super::FILE_ATTRIBUTES);

    write_imports(&mut out, model, message, imports);

    let name = codec_type_name(&model.name, &message.codec);

    out.push_str(&format!(
        "/// The `{}` codec for [`{}`], generated from its Cyclone attributes.\n",
        message.codec, model.name
    ));
    out.push_str("///\n");
    if message.fields.is_empty() {
        out.push_str("/// This codec carries no fields: it encodes to zero bytes.\n");
    } else {
        out.push_str("/// The wire layout, in declaration order (RFC-0002 §5.1):\n");
        out.push_str("///\n");
        for (index, field) in message.fields.iter().enumerate() {
            out.push_str(&format!(
                "/// {index}. `{}`: `{}`\n",
                field.name,
                field.ty.spelling()
            ));
        }
    }
    out.push_str(&format!("pub struct {name};\n\n"));

    out.push_str("#[allow(dead_code)]\n");
    out.push_str(&format!("impl {name} {{\n"));
    out.push_str("    /// This message's name: `Model.codec`.\n");
    out.push_str(&format!(
        "    pub const MESSAGE_NAME: &'static str = \"{}\";\n\n",
        message.name
    ));
    out.push_str("    /// This message's stable id, derived from its name alone.\n");
    out.push_str(&format!(
        "    pub const MESSAGE_ID: u32 = 0x{:08X};\n\n",
        message.id
    ));
    out.push_str("    /// This message's wire-contract fingerprint - the same value\n");
    out.push_str("    /// `handshake.rs` publishes, and the one a peer compares against.\n");
    out.push_str(&format!(
        "    pub const FINGERPRINT: u64 = 0x{:016X};\n\n",
        message.fingerprint.u64()
    ));

    let unused = if message.fields.is_empty() { "_" } else { "" };
    out.push_str(&format!(
        "    /// Writes the `{}` fields of `value`, in declaration order.\n",
        message.codec
    ));
    out.push_str(&format!(
        "    pub fn encode({unused}writer: &mut Writer, {unused}value: &{}) {{\n",
        model.name
    ));
    for field in &message.fields {
        encode_field(&mut out, field, &message.codec);
    }
    out.push_str("    }\n\n");

    out.push_str(&format!(
        "    /// Reads the `{}` fields into `value`, in declaration order.\n",
        message.codec
    ));
    out.push_str("    ///\n");
    out.push_str("    /// Fields this codec does not carry are left as they were, which is what\n");
    out.push_str("    /// lets one model be split across several codecs.\n");
    out.push_str("    ///\n");
    out.push_str("    /// A field the stream ended before takes its zero value (RFC-0002 §9.1);\n");
    out.push_str("    /// a field the stream ended *inside* is an error. Bytes left over after\n");
    out.push_str("    /// the last field belong to a newer writer's model and are ignored.\n");
    out.push_str(&format!(
        "    pub fn decode({unused}reader: &mut Reader, {unused}value: &mut {}) \
         -> ::core::result::Result<(), DecodeError> {{\n",
        model.name
    ));
    for field in &message.fields {
        decode_field(&mut out, field, &message.codec);
    }
    out.push_str("        ::core::result::Result::Ok(())\n");
    out.push_str("    }\n");
    out.push_str("}\n");

    out
}

fn write_imports(out: &mut String, model: &Model, message: &Message, imports: &Imports<'_>) {
    use std::collections::BTreeSet;

    let root = imports.root;
    out.push_str(&format!(
        "use {root}::runtime::{{DecodeError, Reader, Writer}};\n"
    ));

    let mut spelled: BTreeSet<&str> = BTreeSet::new();
    spelled.insert(&model.name);
    for field in &message.fields {
        super::spelled_types(&field.ty, &mut spelled);
    }

    for name in &spelled {
        if let Some(path) = imports.types.get(*name) {
            out.push_str(&format!("use {path};\n"));
        }
    }

    let mut codecs: BTreeSet<String> = BTreeSet::new();
    for field in &message.fields {
        let Some(name) = field.ty.model_name() else {
            continue;
        };
        if name == model.name || !imports.types.contains_key(name) {
            continue;
        }
        codecs.insert(format!(
            "use {root}::{}::{};\n",
            module_name(name, &message.codec),
            codec_type_name(name, &message.codec)
        ));
    }
    for line in codecs {
        out.push_str(&line);
    }

    out.push('\n');
}

fn encode_field(out: &mut String, field: &Field, codec: &str) {
    let place = format!("value.{}", field.name);
    encode_value(out, &field.ty, &place, false, codec, 2, 0);
}

fn encode_value(
    out: &mut String,
    ty: &WireType,
    place: &str,
    borrowed: bool,
    codec: &str,
    indent: usize,
    depth: usize,
) {
    let pad = "    ".repeat(indent);

    if let WireType::Array(element_type) = ty {
        let element = element_variable(depth);
        out.push_str(&format!("{pad}writer.write_array_count({place}.len());\n"));
        out.push_str(&format!("{pad}for {element} in {place}.iter() {{\n"));
        encode_value(
            out,
            element_type,
            &element,
            true,
            codec,
            indent + 1,
            depth + 1,
        );
        out.push_str(&format!("{pad}}}\n"));
        return;
    }

    match primitive(ty) {
        Some((writer_method, _, by_reference)) => {
            let argument = match (by_reference, borrowed) {
                (true, false) => format!("&{place}"),
                (false, true) => format!("*{place}"),
                _ => place.to_owned(),
            };
            out.push_str(&format!("{pad}writer.{writer_method}({argument});\n"));
        }
        None => {
            let nested = codec_type_name(
                ty.model_name().expect("a non-primitive, non-array type"),
                codec,
            );
            let argument = if borrowed {
                place.to_owned()
            } else {
                format!("&{place}")
            };
            out.push_str(&format!("{pad}{nested}::encode(writer, {argument});\n"));
        }
    }
}

fn decode_field(out: &mut String, field: &Field, codec: &str) {
    let place = format!("value.{}", field.name);

    if let Some(name) = as_model(&field.ty) {
        let nested = codec_type_name(name, codec);
        out.push_str(&format!(
            "        {nested}::decode(reader, &mut {place})?;\n"
        ));
        return;
    }

    if let WireType::Array(element_type) = &field.ty {
        out.push_str("        {\n");
        out.push_str(
            "            let count = if reader.field_absent() { 0 } \
             else { reader.read_array_count()? };\n",
        );
        out.push_str(
            "            let mut elements = ::std::vec::Vec::with_capacity(count.min(4096));\n",
        );
        out.push_str("            for _ in 0..count {\n");
        out.push_str(&format!(
            "                elements.push({});\n",
            decode_element(element_type, codec)
        ));
        out.push_str("            }\n");
        out.push_str(&format!("            {place} = elements;\n"));
        out.push_str("        }\n");
        return;
    }

    let (_, reader_method, _) = primitive(&field.ty).expect("models and arrays handled above");
    out.push_str(&format!(
        "        {place} = if reader.field_absent() {{ {} }} else {{ reader.{reader_method}()? }};\n",
        zero(&field.ty),
    ));
}

fn decode_element(ty: &WireType, codec: &str) -> String {
    if let Some(name) = as_model(ty) {
        let nested = codec_type_name(name, codec);
        return format!(
            "{{ let mut element = <{name} as ::core::default::Default>::default(); \
             {nested}::decode(reader, &mut element)?; element }}"
        );
    }

    if let WireType::Array(element_type) = ty {
        return format!(
            "{{ let count = reader.read_array_count()?; \
             let mut inner = ::std::vec::Vec::with_capacity(count.min(4096)); \
             for _ in 0..count {{ inner.push({}); }} inner }}",
            decode_element(element_type, codec)
        );
    }

    let (_, reader_method, _) = primitive(ty).expect("models and arrays handled above");
    format!("reader.{reader_method}()?")
}

fn as_model(ty: &WireType) -> Option<&str> {
    match ty {
        WireType::Model(name) => Some(name),
        _ => None,
    }
}

fn element_variable(depth: usize) -> String {
    if depth == 0 {
        "element".to_owned()
    } else {
        format!("element_{depth}")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{codec_file, file_name, Imports};
    use crate::ir::Schema;
    use crate::model::{Field, Model};

    fn generated(fields: &[(&str, &str)]) -> String {
        let schema = Schema::build(&[
            Model {
                name: "Player".to_owned(),
                source: PathBuf::from("src/models/player.rs"),
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
                source: PathBuf::from("src/models/player.rs"),
                line: 20,
                codecs: vec!["edge".to_owned()],
                fields: vec![Field {
                    name: "level".to_owned(),
                    network_type: "u32".to_owned(),
                    codecs: vec!["edge".to_owned()],
                    line: 21,
                }],
            },
        ])
        .expect("build");

        let types = ["Player", "PlayerInfo"]
            .into_iter()
            .map(|name| (name.to_owned(), format!("crate::models::player::{name}")))
            .collect();
        let model = schema.model("Player").expect("model");
        codec_file(
            model,
            &model.messages[0],
            &Imports {
                types: &types,
                root: "super",
            },
        )
    }

    #[test]
    fn a_primitive_reads_and_writes_the_model_directly() {
        let text = generated(&[("id", "u32")]);
        assert!(text.contains("writer.write_u32(value.id);"), "{text}");
        assert!(
            text.contains(
                "value.id = if reader.field_absent() { 0u32 } else { reader.read_u32()? };"
            ),
            "{text}"
        );
    }

    #[test]
    fn no_dto_no_mapper_no_intermediate_anything() {
        let text = generated(&[("id", "u32"), ("name", "string")]);
        assert!(
            text.contains("pub fn encode(writer: &mut Writer, value: &Player)"),
            "{text}"
        );
        assert!(
            text.contains("pub fn decode(reader: &mut Reader, value: &mut Player)"),
            "{text}"
        );
        for forbidden in [
            "PlayerDTO",
            "PlayerWire",
            "PlayerMapper",
            "struct PlayerData",
        ] {
            assert!(!text.contains(forbidden), "{forbidden} in\n{text}");
        }
    }

    #[test]
    fn a_string_is_borrowed_to_the_writer_and_zeroes_to_an_empty_string() {
        let text = generated(&[("name", "string")]);
        assert!(text.contains("writer.write_string(&value.name);"), "{text}");
        assert!(text.contains("::std::string::String::new()"), "{text}");
    }

    #[test]
    fn a_nested_model_calls_the_same_codec_and_asks_no_question_of_its_own() {
        let text = generated(&[("info", "PlayerInfo")]);
        assert!(
            text.contains("PlayerInfoEdgeCodec::encode(writer, &value.info);"),
            "{text}"
        );
        assert!(
            text.contains("PlayerInfoEdgeCodec::decode(reader, &mut value.info)?;"),
            "{text}"
        );
    }

    #[test]
    fn an_array_counts_first_then_loops() {
        let text = generated(&[("tags", "Array<string>")]);
        assert!(
            text.contains("writer.write_array_count(value.tags.len());"),
            "{text}"
        );
        assert!(
            text.contains("for element in value.tags.iter() {"),
            "{text}"
        );
        assert!(text.contains("writer.write_string(element);"), "{text}");
        assert!(
            text.contains(
                "let count = if reader.field_absent() { 0 } else { reader.read_array_count()? };"
            ),
            "{text}"
        );
        assert!(
            text.contains("elements.push(reader.read_string()?);"),
            "{text}"
        );
    }

    #[test]
    fn an_array_of_primitives_derefs_its_loop_variable() {
        let text = generated(&[("scores", "Array<u32>")]);
        assert!(text.contains("writer.write_u32(*element);"), "{text}");
    }

    #[test]
    fn a_nested_array_names_its_inner_loop_variable_apart() {
        let text = generated(&[("grid", "Array<Array<u8>>")]);
        assert!(
            text.contains("for element in value.grid.iter() {"),
            "{text}"
        );
        assert!(text.contains("for element_1 in element.iter() {"), "{text}");
        assert!(text.contains("writer.write_u8(*element_1);"), "{text}");
    }

    #[test]
    fn two_arrays_in_one_codec_do_not_collide() {
        let text = generated(&[("tags", "Array<string>"), ("scores", "Array<u32>")]);
        assert_eq!(text.matches("let mut elements").count(), 2, "{text}");
        assert_eq!(text.matches("        {\n").count(), 2, "{text}");
    }

    #[test]
    fn the_file_carries_the_headers_the_brief_asks_for() {
        let text = generated(&[("id", "u32")]);
        assert!(
            text.starts_with("// GENERATED BY cyclonec\n// DO NOT EDIT MANUALLY\n"),
            "{text}"
        );
        assert!(text.contains("// source: src/models/player.rs\n"), "{text}");
        assert!(text.contains("// model: Player\n"), "{text}");
        assert!(text.contains("// codec: edge\n"), "{text}");
        assert!(text.contains("// fingerprint: sha256:"), "{text}");
    }

    #[test]
    fn the_constants_are_generated_not_hand_written() {
        let text = generated(&[("id", "u32")]);
        assert!(
            text.contains("pub const MESSAGE_NAME: &'static str = \"Player.edge\";"),
            "{text}"
        );
        assert!(text.contains("pub const MESSAGE_ID: u32 = 0x"), "{text}");
        assert!(text.contains("pub const FINGERPRINT: u64 = 0x"), "{text}");
    }

    #[test]
    fn a_codec_file_is_a_module_that_imports_what_it_names() {
        assert_eq!(file_name("Player", "edge"), "player_edge.rs");
        assert_eq!(
            file_name("PlayerInfo", "orange_pi"),
            "player_info_orange_pi.rs"
        );

        let text = generated(&[("info", "PlayerInfo"), ("roster", "Array<PlayerInfo>")]);
        assert!(
            text.contains("use super::runtime::{DecodeError, Reader, Writer};\n"),
            "{text}"
        );
        assert!(
            text.contains("use crate::models::player::Player;\n"),
            "{text}"
        );
        assert!(
            text.contains("use crate::models::player::PlayerInfo;\n"),
            "{text}"
        );
        assert!(
            text.contains("use super::player_info_edge::PlayerInfoEdgeCodec;\n"),
            "{text}"
        );
    }

    #[test]
    fn a_bare_nested_model_imports_its_codec_and_not_its_type() {
        let text = generated(&[("info", "PlayerInfo")]);
        assert!(
            text.contains("use super::player_info_edge::PlayerInfoEdgeCodec;\n"),
            "{text}"
        );
        assert!(
            !text.contains("use crate::models::player::PlayerInfo;\n"),
            "the type is never spelled, so importing it would only warn:\n{text}"
        );
    }

    #[test]
    fn a_codec_with_no_fields_still_compiles() {
        let text = generated(&[]);
        assert!(
            text.contains("pub fn encode(_writer: &mut Writer, _value: &Player)"),
            "{text}"
        );
    }
}
