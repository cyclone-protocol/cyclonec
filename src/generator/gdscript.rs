use crate::ir::{Field, Message, Model, WireType};
use crate::model::snake_case;

use super::codec_type_name;

#[derive(Default)]
pub struct Header<'a> {
    pub source: Option<&'a str>,
    pub model: Option<&'a str>,
    pub codec: Option<&'a str>,
    pub fingerprint: Option<String>,
    pub note: Option<&'a str>,
}

impl Header<'_> {
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str(super::GDSCRIPT_MARKER);
        out.push('\n');
        out.push_str("# DO NOT EDIT MANUALLY\n");
        if let Some(source) = self.source {
            out.push_str(&format!("# source: {source}\n"));
        }
        if let Some(model) = self.model {
            out.push_str(&format!("# model: {model}\n"));
        }
        if let Some(codec) = self.codec {
            out.push_str(&format!("# codec: {codec}\n"));
        }
        if let Some(fingerprint) = &self.fingerprint {
            out.push_str(&format!("# fingerprint: {fingerprint}\n"));
        }
        out.push_str(&format!(
            "# cyclonec-version: {}\n",
            env!("CARGO_PKG_VERSION")
        ));
        out.push_str(super::GDSCRIPT_TIMESTAMP_PREFIX);
        out.push_str(&crate::timestamp::now_utc());
        out.push('\n');
        if let Some(note) = self.note {
            out.push_str("#\n");
            for line in note.lines() {
                if line.is_empty() {
                    out.push_str("#\n");
                } else {
                    out.push_str(&format!("# {line}\n"));
                }
            }
        }
        out.push('\n');
        out
    }
}

fn primitive(ty: &WireType) -> Option<(&'static str, &'static str)> {
    Some(match ty {
        WireType::Bool => ("write_bool", "read_bool"),
        WireType::I8 => ("write_i8", "read_i8"),
        WireType::U8 => ("write_u8", "read_u8"),
        WireType::I16 => ("write_i16", "read_i16"),
        WireType::U16 => ("write_u16", "read_u16"),
        WireType::I32 => ("write_i32", "read_i32"),
        WireType::U32 => ("write_u32", "read_u32"),
        WireType::I64 => ("write_i64", "read_i64"),
        WireType::U64 => ("write_u64", "read_u64"),
        WireType::F32 => ("write_f32", "read_f32"),
        WireType::F64 => ("write_f64", "read_f64"),
        WireType::Str => ("write_string", "read_string"),
        WireType::Bytes => ("write_bytes", "read_bytes"),
        WireType::Array(_) | WireType::Model(_) => return None,
    })
}

fn zero(ty: &WireType) -> &'static str {
    match ty {
        WireType::Bool => "false",
        WireType::Str => "\"\"",
        WireType::Bytes => "PackedByteArray()",
        WireType::F32 | WireType::F64 => "0.0",
        WireType::Array(_) => unreachable!("an array's absence is handled by decode_field"),
        WireType::Model(_) => unreachable!("a model field is decoded through its own codec"),
        _ => "0",
    }
}

pub fn u32_literal(value: u32) -> String {
    format!("0x{value:08X}")
}

pub fn u64_literal(value: u64) -> String {
    let high = (value >> 32) as u32;
    let low = value as u32;
    if high == 0 {
        u32_literal(low)
    } else {
        format!("(0x{high:08X} << 32) | 0x{low:08X}")
    }
}

pub fn file_name(model: &str, codec: &str) -> String {
    format!("{}_{}.gd", snake_case(model), snake_case(codec))
}

pub fn check_no_nested_arrays(model: &Model) -> Result<(), String> {
    for field in &model.fields {
        if let WireType::Array(element) = &field.ty {
            if matches!(element.as_ref(), WireType::Array(_)) {
                return Err(format!(
                    "model '{}' field '{}': the GDScript backend does not support \
                     `Array<Array<T>>` - split '{}' into two codecs, or flatten the field",
                    model.name, field.name, field.name,
                ));
            }
        }
    }
    Ok(())
}

pub fn codec_file(model: &Model, message: &Message) -> String {
    let mut out = Header {
        source: Some(&model.source),
        model: Some(&model.name),
        codec: Some(&message.codec),
        fingerprint: Some(message.fingerprint.tagged()),
        note: None,
    }
    .render();

    let name = codec_type_name(&model.name, &message.codec);
    out.push_str(&format!("class_name {name}\n\n"));

    out.push_str(&format!(
        "# {name} is the {:?} codec for {}, generated from its Cyclone directives.\n",
        message.codec, model.name
    ));
    if message.fields.is_empty() {
        out.push_str("#\n# This codec carries no fields: it encodes to zero bytes.\n");
    } else {
        out.push_str("#\n# The wire layout, in declaration order (RFC-0002 §5.1):\n#\n");
        for (index, field) in message.fields.iter().enumerate() {
            out.push_str(&format!(
                "#  {index}. `{}`: `{}`\n",
                field.name,
                field.ty.spelling()
            ));
        }
    }
    out.push('\n');

    out.push_str("# This message's name: Model.codec.\n");
    out.push_str(&format!("const MESSAGE_NAME := {:?}\n\n", message.name));
    out.push_str("# This message's stable id, derived from its name alone.\n");
    out.push_str(&format!(
        "const MESSAGE_ID: int = {}\n\n",
        u32_literal(message.id)
    ));
    out.push_str(
        "# This message's wire-contract fingerprint - the same value handshake.gd publishes,\n\
         # and the one a peer compares against.\n",
    );
    out.push_str(&format!(
        "const FINGERPRINT: int = {}\n\n",
        u64_literal(message.fingerprint.u64())
    ));

    out.push_str(&format!(
        "# Writes the {:?} fields of value, in declaration order.\n",
        message.codec
    ));
    out.push_str(&format!(
        "static func encode(writer: CycloneRuntime.Writer, value: {}) -> void:\n",
        model.name
    ));
    if message.fields.is_empty() {
        out.push_str("\tpass\n");
    } else {
        for field in &message.fields {
            encode_field(&mut out, field, &message.codec);
        }
    }
    out.push('\n');

    out.push_str(&format!(
        "# Reads the {:?} fields into value, in declaration order.\n",
        message.codec
    ));
    out.push_str(
        "#\n\
         # Fields this codec does not carry are left as they were, which is what lets one\n\
         # model be split across several codecs.\n\
         #\n\
         # A field the stream ended before takes its zero value (RFC-0002 §9.1); a field\n\
         # the stream ended inside is an error. Bytes left over after the last field\n\
         # belong to a newer writer's model and are ignored.\n",
    );
    out.push_str(&format!(
        "static func decode(reader: CycloneRuntime.Reader, value: {}) -> CycloneRuntime.DecodeError:\n",
        model.name
    ));
    if message.fields.is_empty() {
        out.push_str("\treturn null\n");
    } else {
        for field in &message.fields {
            decode_field(&mut out, field, &message.codec);
        }
        out.push_str("\treturn null\n");
    }

    out
}

fn encode_field(out: &mut String, field: &Field, codec: &str) {
    let place = format!("value.{}", field.name);

    if let WireType::Array(element_type) = &field.ty {
        out.push_str(&format!("\twriter.write_array_count({place}.size())\n"));
        out.push_str(&format!("\tfor {}_element in {place}:\n", field.name));
        encode_scalar(
            out,
            element_type,
            &format!("{}_element", field.name),
            codec,
            "\t\t",
        );
        return;
    }

    encode_scalar(out, &field.ty, &place, codec, "\t");
}

fn encode_scalar(out: &mut String, ty: &WireType, place: &str, codec: &str, pad: &str) {
    match primitive(ty) {
        Some((writer_method, _)) => {
            out.push_str(&format!("{pad}writer.{writer_method}({place})\n"));
        }
        None => {
            let nested = codec_type_name(
                ty.model_name().expect("a non-primitive, non-array type"),
                codec,
            );
            out.push_str(&format!("{pad}{nested}.encode(writer, {place})\n"));
        }
    }
}

fn decode_field(out: &mut String, field: &Field, codec: &str) {
    let place = format!("value.{}", field.name);
    let name = &field.name;

    if let Some(model_name) = as_model(&field.ty) {
        let nested = codec_type_name(model_name, codec);
        out.push_str(&format!(
            "\tvar {name}_error = {nested}.decode(reader, {place})\n"
        ));
        out.push_str(&format!(
            "\tif {name}_error != null:\n\t\treturn {name}_error\n"
        ));
        return;
    }

    if let WireType::Array(element_type) = &field.ty {
        out.push_str(&format!("\tvar {name}_count := 0\n"));
        out.push_str("\tif not reader.field_absent():\n");
        out.push_str(&format!(
            "\t\tvar {name}_count_result := reader.read_array_count()\n"
        ));
        out.push_str(&format!("\t\tif {name}_count_result[1] != null:\n"));
        out.push_str(&format!("\t\t\treturn {name}_count_result[1]\n"));
        out.push_str(&format!("\t\t{name}_count = {name}_count_result[0]\n"));
        out.push_str(&format!("\tvar {name}_elements := []\n"));
        out.push_str(&format!("\tfor {name}_i in range({name}_count):\n"));
        decode_element(out, element_type, name, codec);
        out.push_str(&format!("\tvalue.{name} = {name}_elements\n"));
        return;
    }

    let (_, reader_method) = primitive(&field.ty).expect("models and arrays handled above");
    out.push_str("\tif reader.field_absent():\n");
    out.push_str(&format!("\t\t{place} = {}\n", zero(&field.ty)));
    out.push_str("\telse:\n");
    out.push_str(&format!(
        "\t\tvar {name}_result = reader.{reader_method}()\n"
    ));
    out.push_str(&format!("\t\tif {name}_result[1] != null:\n"));
    out.push_str(&format!("\t\t\treturn {name}_result[1]\n"));
    out.push_str(&format!("\t\t{place} = {name}_result[0]\n"));
}

fn decode_element(out: &mut String, ty: &WireType, name: &str, codec: &str) {
    match as_model(ty) {
        Some(model_name) => {
            let nested = codec_type_name(model_name, codec);
            out.push_str(&format!("\t\tvar {name}_element = {model_name}.new()\n"));
            out.push_str(&format!(
                "\t\tvar {name}_element_error = {nested}.decode(reader, {name}_element)\n"
            ));
            out.push_str(&format!("\t\tif {name}_element_error != null:\n"));
            out.push_str(&format!("\t\t\treturn {name}_element_error\n"));
            out.push_str(&format!("\t\t{name}_elements.append({name}_element)\n"));
        }
        None => {
            let (_, reader_method) = primitive(ty).expect("models handled above");
            out.push_str(&format!(
                "\t\tvar {name}_element_result = reader.{reader_method}()\n"
            ));
            out.push_str(&format!("\t\tif {name}_element_result[1] != null:\n"));
            out.push_str(&format!("\t\t\treturn {name}_element_result[1]\n"));
            out.push_str(&format!(
                "\t\t{name}_elements.append({name}_element_result[0])\n"
            ));
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
    use std::path::PathBuf;

    use super::{check_no_nested_arrays, codec_file, file_name};
    use crate::ir::Schema;
    use crate::model::{Field, Model};

    fn generated(fields: &[(&str, &str)]) -> String {
        let schema = Schema::build(&[
            Model {
                name: "Player".to_owned(),
                source: PathBuf::from("models/player.gd"),
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
                source: PathBuf::from("models/player.gd"),
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

        let model = schema.model("Player").expect("model");
        codec_file(model, &model.messages[0])
    }

    #[test]
    fn a_primitive_reads_and_writes_the_model_directly() {
        let text = generated(&[("id", "u32")]);
        assert!(text.contains("writer.write_u32(value.id)"), "{text}");
        assert!(
            text.contains("if reader.field_absent():\n\t\tvalue.id = 0\n\telse:"),
            "{text}"
        );
        assert!(text.contains("value.id = id_result[0]"), "{text}");
    }

    #[test]
    fn no_dto_no_mapper_no_intermediate_anything() {
        let text = generated(&[("id", "u32"), ("name", "string")]);
        assert!(
            text.contains(
                "static func encode(writer: CycloneRuntime.Writer, value: Player) -> void:"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "static func decode(reader: CycloneRuntime.Reader, value: Player) -> CycloneRuntime.DecodeError:"
            ),
            "{text}"
        );
        for forbidden in ["PlayerDTO", "PlayerWire", "PlayerMapper"] {
            assert!(!text.contains(forbidden), "{forbidden} in\n{text}");
        }
    }

    #[test]
    fn a_string_zeroes_to_an_empty_string() {
        let text = generated(&[("name", "string")]);
        assert!(text.contains("writer.write_string(value.name)"), "{text}");
        assert!(text.contains("value.name = \"\""), "{text}");
    }

    #[test]
    fn a_nested_model_calls_the_same_codec_bare_and_asks_no_question_of_its_own() {
        let text = generated(&[("info", "PlayerInfo")]);
        assert!(
            text.contains("PlayerInfoEdgeCodec.encode(writer, value.info)"),
            "{text}"
        );
        assert!(
            text.contains("var info_error = PlayerInfoEdgeCodec.decode(reader, value.info)"),
            "{text}"
        );
        assert!(
            text.contains("if info_error != null:\n\t\treturn info_error"),
            "{text}"
        );
    }

    #[test]
    fn an_array_counts_first_then_loops_strictly() {
        let text = generated(&[("tags", "Array<string>")]);
        assert!(
            text.contains("writer.write_array_count(value.tags.size())"),
            "{text}"
        );
        assert!(text.contains("for tags_element in value.tags:"), "{text}");
        assert!(text.contains("writer.write_string(tags_element)"), "{text}");
        assert!(text.contains("if not reader.field_absent():"), "{text}");
        assert!(
            text.contains("var tags_count_result := reader.read_array_count()"),
            "{text}"
        );
        assert!(text.contains("for tags_i in range(tags_count):"), "{text}");
        assert!(text.contains("value.tags = tags_elements"), "{text}");
    }

    #[test]
    fn an_array_of_models_creates_a_fresh_element_each_iteration() {
        let text = generated(&[("roster", "Array<PlayerInfo>")]);
        assert!(
            text.contains("var roster_element = PlayerInfo.new()"),
            "{text}"
        );
        assert!(
            text.contains(
                "var roster_element_error = PlayerInfoEdgeCodec.decode(reader, roster_element)"
            ),
            "{text}"
        );
        assert!(
            text.contains("roster_elements.append(roster_element)"),
            "{text}"
        );
    }

    #[test]
    fn nested_arrays_are_refused_rather_than_generated_wrong() {
        let model = Model {
            name: "Grid".to_owned(),
            source: PathBuf::from("models/grid.gd"),
            line: 1,
            codecs: vec!["edge".to_owned()],
            fields: vec![Field {
                name: "rows".to_owned(),
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
        assert_eq!(file_name("Player", "edge"), "player_edge.gd");
        assert_eq!(
            file_name("PlayerInfo", "orange_pi"),
            "player_info_orange_pi.gd"
        );
    }

    #[test]
    fn a_codec_with_no_fields_still_compiles() {
        let text = generated(&[]);
        assert!(
            text.contains(
                "static func encode(writer: CycloneRuntime.Writer, value: Player) -> void:\n\tpass\n"
            ),
            "{text}"
        );
        assert!(text.contains("return null\n"), "{text}");
    }

    #[test]
    fn the_file_carries_the_headers_the_brief_asks_for() {
        let text = generated(&[("id", "u32")]);
        assert!(
            text.starts_with("# GENERATED BY cyclonec\n# DO NOT EDIT MANUALLY\n"),
            "{text}"
        );
        assert!(text.contains("# source: models/player.gd\n"), "{text}");
        assert!(text.contains("# model: Player\n"), "{text}");
        assert!(text.contains("# codec: edge\n"), "{text}");
        assert!(text.contains("# fingerprint: sha256:"), "{text}");
        assert!(text.contains("class_name PlayerEdgeCodec\n"), "{text}");
    }

    #[test]
    fn the_constants_are_generated_not_hand_written() {
        let text = generated(&[("id", "u32")]);
        assert!(
            text.contains("const MESSAGE_NAME := \"Player.edge\""),
            "{text}"
        );
        assert!(text.contains("const MESSAGE_ID: int = 0x"), "{text}");
        assert!(text.contains("const FINGERPRINT: int ="), "{text}");
    }

    #[test]
    fn a_64_bit_value_with_its_top_bit_set_is_split_not_a_bare_wide_literal() {
        assert_eq!(super::u64_literal(0x0000_0000_0000_002A), "0x0000002A");
        assert_eq!(
            super::u64_literal(0xFFFF_FFFF_FFFF_FFFF),
            "(0xFFFFFFFF << 32) | 0xFFFFFFFF"
        );
        assert_eq!(
            super::u64_literal(0x8000_0000_0000_0001),
            "(0x80000000 << 32) | 0x00000001"
        );
    }
}
