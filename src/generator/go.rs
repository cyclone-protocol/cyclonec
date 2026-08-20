use std::collections::BTreeSet;
use std::path::Path;

use crate::ir::{Field, Message, Model, WireType};
use crate::model::snake_case;

use super::codec_type_name;

pub fn package_name_from_out(out: &Path) -> String {
    let basename = out
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("generated");
    sanitize_package_name(basename)
}

fn sanitize_package_name(text: &str) -> String {
    let mut out: String = text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect();

    if out.is_empty() {
        return "generated".to_owned();
    }
    if out.starts_with(|character: char| character.is_ascii_digit()) {
        out.insert_str(0, "pkg");
    }
    if GO_KEYWORDS.contains(&out.as_str()) {
        out.push_str("pkg");
    }
    out
}

const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];

fn primitive(ty: &WireType) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match ty {
        WireType::Bool => ("WriteBool", "ReadBool", "bool"),
        WireType::I8 => ("WriteI8", "ReadI8", "int8"),
        WireType::U8 => ("WriteU8", "ReadU8", "uint8"),
        WireType::I16 => ("WriteI16", "ReadI16", "int16"),
        WireType::U16 => ("WriteU16", "ReadU16", "uint16"),
        WireType::I32 => ("WriteI32", "ReadI32", "int32"),
        WireType::U32 => ("WriteU32", "ReadU32", "uint32"),
        WireType::I64 => ("WriteI64", "ReadI64", "int64"),
        WireType::U64 => ("WriteU64", "ReadU64", "uint64"),
        WireType::F32 => ("WriteF32", "ReadF32", "float32"),
        WireType::F64 => ("WriteF64", "ReadF64", "float64"),
        WireType::Str => ("WriteString", "ReadString", "string"),
        WireType::Bytes => ("WriteBytes", "ReadBytes", "[]byte"),
        WireType::Array(_) | WireType::Model(_) => return None,
    })
}

fn zero(ty: &WireType) -> &'static str {
    match ty {
        WireType::Bool => "false",
        WireType::Str => "\"\"",
        WireType::Bytes | WireType::Array(_) => "nil",
        WireType::Model(_) => unreachable!("a model field is decoded through its own codec"),
        _ => "0",
    }
}

pub fn file_name(model: &str, codec: &str) -> String {
    format!("{}_{}.go", snake_case(model), snake_case(codec))
}

pub struct ModelLocation {
    pub import_path: String,
    pub package: String,
}

pub struct Imports<'a> {
    pub locations: &'a std::collections::BTreeMap<String, ModelLocation>,
    pub own_import_path: &'a str,
}

impl Imports<'_> {
    fn qualify(&self, model: &str) -> String {
        match self.locations.get(model) {
            Some(location) if location.import_path == self.own_import_path => model.to_owned(),
            Some(location) => format!("{}.{model}", location.package),
            None => model.to_owned(),
        }
    }
}

pub fn check_no_nested_arrays(model: &Model) -> Result<(), String> {
    for field in &model.fields {
        if let WireType::Array(element) = &field.ty {
            if matches!(element.as_ref(), WireType::Array(_)) {
                return Err(format!(
                    "model '{}' field '{}': the Go backend does not support `Array<Array<T>>` \
                     - split '{}' into two codecs, or flatten the field",
                    model.name, field.name, field.name,
                ));
            }
        }
    }
    Ok(())
}

pub fn codec_file(
    model: &Model,
    message: &Message,
    package: &str,
    imports: &Imports<'_>,
) -> String {
    let mut out = super::Header {
        source: Some(&model.source),
        model: Some(&model.name),
        codec: Some(&message.codec),
        fingerprint: Some(message.fingerprint.tagged()),
        note: None,
    }
    .render();
    out.push_str(&format!("package {package}\n\n"));

    write_imports(&mut out, model, message, imports);

    let name = codec_type_name(&model.name, &message.codec);
    let model_type = imports.qualify(&model.name);

    out.push_str(&format!(
        "// {name} is the {:?} codec for {}, generated from its Cyclone attributes.\n",
        message.codec, model.name
    ));
    if message.fields.is_empty() {
        out.push_str("//\n// This codec carries no fields: it encodes to zero bytes.\n");
    } else {
        out.push_str("//\n// The wire layout, in declaration order (RFC-0002 §5.1):\n//\n");
        for (index, field) in message.fields.iter().enumerate() {
            out.push_str(&format!(
                "//  {index}. `{}`: `{}`\n",
                field.name,
                field.ty.spelling()
            ));
        }
    }
    out.push_str(&format!("type {name} struct{{}}\n\n"));

    out.push_str(&format!(
        "// {name}MessageName is this message's name: Model.codec.\nconst {name}MessageName = {:?}\n\n",
        message.name
    ));
    out.push_str(&format!(
        "// {name}MessageID is this message's stable id, derived from its name alone.\n\
         const {name}MessageID uint32 = 0x{:08X}\n\n",
        message.id
    ));
    out.push_str(&format!(
        "// {name}Fingerprint is this message's wire-contract fingerprint - the same value\n\
         // handshake.go publishes, and the one a peer compares against.\n\
         const {name}Fingerprint uint64 = 0x{:016X}\n\n",
        message.fingerprint.u64()
    ));

    out.push_str(&format!(
        "// Encode writes the {:?} fields of value, in declaration order.\n",
        message.codec
    ));
    out.push_str(&format!(
        "func ({name}) Encode(w *Writer, value *{model_type}) {{\n"
    ));
    for field in &message.fields {
        encode_field(&mut out, field, &message.codec);
    }
    out.push_str("}\n\n");

    out.push_str(&format!(
        "// Decode reads the {:?} fields into value, in declaration order.\n",
        message.codec
    ));
    out.push_str(
        "//\n\
         // Fields this codec does not carry are left as they were, which is what lets one\n\
         // model be split across several codecs.\n\
         //\n\
         // A field the stream ended before takes its zero value (RFC-0002 §9.1); a field\n\
         // the stream ended inside is an error. Bytes left over after the last field\n\
         // belong to a newer writer's model and are ignored.\n",
    );
    out.push_str(&format!(
        "func ({name}) Decode(r *Reader, value *{model_type}) error {{\n"
    ));
    if message.fields.is_empty() {
        out.push_str("\treturn nil\n");
    } else {
        out.push_str("\tvar err error\n\n");
        for field in &message.fields {
            decode_field(&mut out, field, &message.codec, imports);
        }
        out.push_str("\treturn nil\n");
    }
    out.push_str("}\n");

    out
}

fn write_imports(out: &mut String, model: &Model, message: &Message, imports: &Imports<'_>) {
    let mut spelled: BTreeSet<&str> = BTreeSet::new();
    spelled.insert(&model.name);
    for field in &message.fields {
        super::spelled_types(&field.ty, &mut spelled);
    }

    let mut paths: BTreeSet<&str> = BTreeSet::new();
    for name in &spelled {
        if let Some(location) = imports.locations.get(*name) {
            if location.import_path != imports.own_import_path {
                paths.insert(&location.import_path);
            }
        }
    }

    if paths.is_empty() {
        return;
    }
    out.push_str("import (\n");
    for path in paths {
        out.push_str(&format!("\t{path:?}\n"));
    }
    out.push_str(")\n\n");
}

fn encode_field(out: &mut String, field: &Field, codec: &str) {
    let place = format!("value.{}", field.name);

    if let WireType::Array(element_type) = &field.ty {
        out.push_str(&format!("\tw.WriteArrayCount(len({place}))\n"));
        out.push_str(&format!("\tfor _, element := range {place} {{\n"));
        encode_scalar(out, element_type, "element", codec, "\t\t");
        out.push_str("\t}\n");
        return;
    }

    encode_scalar(out, &field.ty, &place, codec, "\t");
}

fn encode_scalar(out: &mut String, ty: &WireType, place: &str, codec: &str, pad: &str) {
    match primitive(ty) {
        Some((writer_method, ..)) => {
            out.push_str(&format!("{pad}w.{writer_method}({place})\n"));
        }
        None => {
            let nested = codec_type_name(
                ty.model_name().expect("a non-primitive, non-array type"),
                codec,
            );
            out.push_str(&format!("{pad}({nested}{{}}).Encode(w, &{place})\n"));
        }
    }
}

fn decode_field(out: &mut String, field: &Field, codec: &str, imports: &Imports<'_>) {
    let place = format!("value.{}", field.name);

    if let Some(name) = as_model(&field.ty) {
        let nested = codec_type_name(name, codec);
        out.push_str(&format!("\terr = ({nested}{{}}).Decode(r, &{place})\n"));
        out.push_str("\tif err != nil {\n\t\treturn err\n\t}\n");
        return;
    }

    if let WireType::Array(element_type) = &field.ty {
        let local = local_name(&field.name);
        let count_local = format!("{local}Count");
        let elements_local = format!("{local}Elements");
        let go_type = element_type_name(element_type, imports);

        out.push_str(&format!("\tvar {count_local} int\n"));
        out.push_str("\tif !r.FieldAbsent() {\n");
        out.push_str(&format!("\t\t{count_local}, err = r.ReadArrayCount()\n"));
        out.push_str("\t\tif err != nil {\n\t\t\treturn err\n\t\t}\n");
        out.push_str("\t}\n");
        out.push_str(&format!(
            "\t{elements_local} := make([]{go_type}, 0, {count_local})\n"
        ));
        out.push_str(&format!("\tfor i := 0; i < {count_local}; i++ {{\n"));
        decode_scalar(out, element_type, "element", codec, imports, "\t\t");
        out.push_str(&format!(
            "\t\t{elements_local} = append({elements_local}, element)\n"
        ));
        out.push_str("\t}\n");
        out.push_str(&format!("\t{place} = {elements_local}\n"));
        return;
    }

    let (_, reader_method, _) = primitive(&field.ty).expect("models and arrays handled above");
    out.push_str("\tif r.FieldAbsent() {\n");
    out.push_str(&format!("\t\t{place} = {}\n", zero(&field.ty)));
    out.push_str("\t} else {\n");
    out.push_str(&format!("\t\t{place}, err = r.{reader_method}()\n"));
    out.push_str("\t\tif err != nil {\n\t\t\treturn err\n\t\t}\n");
    out.push_str("\t}\n");
}

fn decode_scalar(
    out: &mut String,
    ty: &WireType,
    var: &str,
    codec: &str,
    imports: &Imports<'_>,
    pad: &str,
) {
    match as_model(ty) {
        Some(name) => {
            let go_type = imports.qualify(name);
            let nested = codec_type_name(name, codec);
            out.push_str(&format!("{pad}var {var} {go_type}\n"));
            out.push_str(&format!("{pad}err = ({nested}{{}}).Decode(r, &{var})\n"));
            out.push_str(&format!(
                "{pad}if err != nil {{\n{pad}\treturn err\n{pad}}}\n"
            ));
        }
        None => {
            let (_, reader_method, go_type) = primitive(ty).expect("models handled above");
            out.push_str(&format!("{pad}var {var} {go_type}\n"));
            out.push_str(&format!("{pad}{var}, err = r.{reader_method}()\n"));
            out.push_str(&format!(
                "{pad}if err != nil {{\n{pad}\treturn err\n{pad}}}\n"
            ));
        }
    }
}

fn element_type_name(ty: &WireType, imports: &Imports<'_>) -> String {
    match primitive(ty) {
        Some((_, _, go_type)) => go_type.to_owned(),
        None => imports.qualify(ty.model_name().expect("a non-primitive, non-array type")),
    }
}

fn as_model(ty: &WireType) -> Option<&str> {
    match ty {
        WireType::Model(name) => Some(name),
        _ => None,
    }
}

fn local_name(field_name: &str) -> String {
    let mut chars = field_name.chars();
    let mut local = match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>(),
        None => String::new(),
    };
    local.push_str(chars.as_str());
    local
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use super::{
        check_no_nested_arrays, codec_file, file_name, package_name_from_out, ModelLocation,
    };
    use crate::ir::Schema;
    use crate::model::{Field, Model};

    fn generated(fields: &[(&str, &str)]) -> String {
        let schema = Schema::build(&[
            Model {
                name: "Player".to_owned(),
                source: PathBuf::from("models/player.go"),
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
                source: PathBuf::from("models/player.go"),
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
                        import_path: "example.com/game/models".to_owned(),
                        package: "models".to_owned(),
                    },
                )
            })
            .collect();
        let model = schema.model("Player").expect("model");
        codec_file(
            model,
            &model.messages[0],
            "generated",
            &super::Imports {
                locations: &locations,
                own_import_path: "example.com/game/generated",
            },
        )
    }

    #[test]
    fn a_primitive_reads_and_writes_the_model_directly() {
        let text = generated(&[("ID", "u32")]);
        assert!(text.contains("w.WriteU32(value.ID)"), "{text}");
        assert!(
            text.contains("if r.FieldAbsent() {\n\t\tvalue.ID = 0\n\t} else {"),
            "{text}"
        );
    }

    #[test]
    fn no_dto_no_mapper_no_intermediate_anything() {
        let text = generated(&[("ID", "u32"), ("Name", "string")]);
        assert!(
            text.contains("func (PlayerEdgeCodec) Encode(w *Writer, value *models.Player)"),
            "{text}"
        );
        assert!(
            text.contains("func (PlayerEdgeCodec) Decode(r *Reader, value *models.Player) error"),
            "{text}"
        );
        for forbidden in ["PlayerDTO", "PlayerWire", "PlayerMapper"] {
            assert!(!text.contains(forbidden), "{forbidden} in\n{text}");
        }
    }

    #[test]
    fn a_string_zeroes_to_an_empty_string() {
        let text = generated(&[("Name", "string")]);
        assert!(text.contains("w.WriteString(value.Name)"), "{text}");
        assert!(text.contains("value.Name = \"\""), "{text}");
    }

    #[test]
    fn a_nested_model_calls_the_same_codec_and_asks_no_question_of_its_own() {
        let text = generated(&[("Info", "PlayerInfo")]);
        assert!(
            text.contains("(PlayerInfoEdgeCodec{}).Encode(w, &value.Info)"),
            "{text}"
        );
        assert!(
            text.contains("(PlayerInfoEdgeCodec{}).Decode(r, &value.Info)"),
            "{text}"
        );
        assert!(!text.contains("PlayerInfoEdgeCodec\""), "{text}");
    }

    #[test]
    fn an_array_counts_first_then_loops_strictly() {
        let text = generated(&[("Tags", "Array<string>")]);
        assert!(
            text.contains("w.WriteArrayCount(len(value.Tags))"),
            "{text}"
        );
        assert!(
            text.contains("for _, element := range value.Tags {"),
            "{text}"
        );
        assert!(text.contains("w.WriteString(element)"), "{text}");
        assert!(
            text.contains("if !r.FieldAbsent() {\n\t\ttagsCount, err = r.ReadArrayCount()"),
            "{text}"
        );
        assert!(text.contains("var element string"), "{text}");
        assert!(!text.contains("elements = append"), "{text}");
    }

    #[test]
    fn the_import_is_only_the_models_package_and_only_once() {
        let text = generated(&[("Info", "PlayerInfo"), ("Roster", "Array<PlayerInfo>")]);
        assert_eq!(text.matches("example.com/game/models").count(), 1, "{text}");
        assert!(
            text.contains("import (\n\t\"example.com/game/models\"\n)"),
            "{text}"
        );
    }

    #[test]
    fn a_model_colocated_with_the_generated_package_is_spelled_bare() {
        let schema = Schema::build(&[Model {
            name: "Player".to_owned(),
            source: PathBuf::from("models/player.go"),
            line: 1,
            codecs: vec!["edge".to_owned()],
            fields: vec![Field {
                name: "ID".to_owned(),
                network_type: "u32".to_owned(),
                codecs: vec!["edge".to_owned()],
                line: 2,
            }],
        }])
        .expect("build");
        let locations: BTreeMap<String, ModelLocation> = [(
            "Player".to_owned(),
            ModelLocation {
                import_path: "example.com/game/models".to_owned(),
                package: "models".to_owned(),
            },
        )]
        .into_iter()
        .collect();
        let model = schema.model("Player").expect("model");
        let text = codec_file(
            model,
            &model.messages[0],
            "models",
            &super::Imports {
                locations: &locations,
                own_import_path: "example.com/game/models",
            },
        );
        assert!(
            text.contains("func (PlayerEdgeCodec) Encode(w *Writer, value *Player)"),
            "{text}"
        );
        assert!(!text.contains("import ("), "{text}");
    }

    #[test]
    fn nested_arrays_are_refused_rather_than_generated_wrong() {
        let model = Model {
            name: "Grid".to_owned(),
            source: PathBuf::from("models/grid.go"),
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
    fn a_package_name_is_derived_from_outs_basename() {
        assert_eq!(
            package_name_from_out(Path::new("src/generated")),
            "generated"
        );
        assert_eq!(package_name_from_out(Path::new("gen")), "gen");
        assert_eq!(package_name_from_out(Path::new("2fast")), "pkg2fast");
        assert_eq!(package_name_from_out(Path::new("my-service")), "myservice");
        assert_eq!(package_name_from_out(Path::new("range")), "rangepkg");
    }

    #[test]
    fn a_codec_file_is_named_like_a_go_file() {
        assert_eq!(file_name("Player", "edge"), "player_edge.go");
        assert_eq!(
            file_name("PlayerInfo", "orange_pi"),
            "player_info_orange_pi.go"
        );
    }

    #[test]
    fn a_codec_with_no_fields_still_compiles() {
        let text = generated(&[]);
        assert!(
            text.contains("func (PlayerEdgeCodec) Encode(w *Writer, value *models.Player) {\n}"),
            "{text}"
        );
        assert!(text.contains("return nil\n}"), "{text}");
    }

    #[test]
    fn the_file_carries_the_headers_the_brief_asks_for() {
        let text = generated(&[("ID", "u32")]);
        assert!(
            text.starts_with("// GENERATED BY cyclonec\n// DO NOT EDIT MANUALLY\n"),
            "{text}"
        );
        assert!(text.contains("// source: models/player.go\n"), "{text}");
        assert!(text.contains("// model: Player\n"), "{text}");
        assert!(text.contains("// codec: edge\n"), "{text}");
        assert!(text.contains("// fingerprint: sha256:"), "{text}");
        assert!(text.contains("package generated\n"), "{text}");
    }
}
