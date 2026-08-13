//! One message → one Go file.
//!
//! The Go counterpart of [`super::rust`]. Same rule, same shape: a message is
//! walked once, a field at a time, and each field appends one statement.
//! Nothing is analysed on the way: a primitive is a table lookup, and
//! anything else is another model's name spelled into a call.
//!
//! # What Go forces that Rust does not
//!
//! Go has no exceptions, so `Decode` returns `error` and every read is
//! followed by an explicit check. And Go compiles by *package*, not by file:
//! every generated file shares one `package` clause with its siblings, so
//! (unlike [`super::rust`], where each codec is its own module) codecs never
//! import one another - only the model types they name need an `import`, and
//! only when that model's package differs from the generated one.
//!
//! # The decoder, and RFC-0002 §9.1
//!
//! Identical policy to [`super::rust`]: a bare field asks `r.FieldAbsent()`
//! before it reads, taking its zero value when the stream already ended;
//! array *elements* are read strictly, once the count says they exist; a
//! nested model is decoded through its own codec unconditionally, which asks
//! the same question of every one of *its* fields.
//!
//! # A known gap
//!
//! `Array<Array<T>>` is refused with a clear error rather than generated
//! wrong. `cyclonec_old`'s Go backend never handled it correctly either (it
//! quietly emitted a call to a codec named after the inner `Array<...>`
//! spelling, which cannot exist) - this backend reports it instead of
//! reproducing that bug. Nesting one array inside another is rare enough in
//! practice that refusing it outright, clearly, was judged better than the
//! extra generator complexity a correct implementation would need.

use std::collections::BTreeSet;
use std::path::Path;

use crate::ir::{Field, Message, Model, WireType};
use crate::model::snake_case;

use super::codec_type_name;

/// The Go package name every generated file in one run shares, derived from
/// `--out`'s own directory name - the same convention virtually every Go code
/// generator follows, so that `go/generated/` reads as `package generated`
/// without anyone having to say so twice.
pub fn package_name_from_out(out: &Path) -> String {
    let basename = out
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("generated");
    sanitize_package_name(basename)
}

/// Go identifiers are ASCII letters, digits and `_`, may not start with a
/// digit, and may not be a keyword; a package name conventionally avoids
/// underscores too. `--out`'s basename rarely needs any of this, but a
/// generator that trusts a directory name unchecked is a generator that picks
/// today to find out `--out 2fast` does not compile.
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

/// The runtime method each primitive maps to, and its Go spelling.
///
/// This table is the whole of the generator's type knowledge. Every name in
/// it comes from RFC-0002's Reader / Writer interface, spelled the way
/// [`super::go_runtime`] spells it; a name that is not in it is another
/// model, and the call is spelled and left for the Go compiler to resolve.
fn primitive(ty: &WireType) -> Option<(&'static str, &'static str, &'static str)> {
    // (writer method, reader method, Go type)
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

/// The Go zero-value expression for a field the stream ended before
/// (RFC-0002 §9.1). Never called for [`WireType::Model`] - a nested model has
/// no zero expression of its own; it is decoded through its own codec, which
/// zeroes its fields by asking the same question of each of them.
fn zero(ty: &WireType) -> &'static str {
    match ty {
        WireType::Bool => "false",
        WireType::Str => "\"\"",
        WireType::Bytes | WireType::Array(_) => "nil",
        WireType::Model(_) => unreachable!("a model field is decoded through its own codec"),
        _ => "0",
    }
}

/// The generated file name: `Player` + `edge` → `player_edge.go`.
pub fn file_name(model: &str, codec: &str) -> String {
    format!("{}_{}.go", snake_case(model), snake_case(codec))
}

/// Where one model's type is declared, in Go's own terms.
pub struct ModelLocation {
    /// The import path of the package the model's source lives in, e.g.
    /// `github.com/acme/game/internal/models`.
    pub import_path: String,
    /// The name that package declares in its own `package` clause - not
    /// necessarily the last segment of `import_path`, which is why this is
    /// read from source rather than derived from the path.
    pub package: String,
}

/// Where every model this run parsed can be reached from inside a generated
/// Go file.
pub struct Imports<'a> {
    pub locations: &'a std::collections::BTreeMap<String, ModelLocation>,
    /// The import path the generated package itself resolves to. A model
    /// whose own import path is equal to this one is declared *in* the
    /// generated package - referencing it through an `import` would be a
    /// self-import, which Go refuses to compile, so it is spelled bare
    /// instead.
    pub own_import_path: &'a str,
}

impl Imports<'_> {
    /// How a model's type is spelled in generated code: bare if it is
    /// colocated with the generated package, package-qualified otherwise.
    fn qualify(&self, model: &str) -> String {
        match self.locations.get(model) {
            Some(location) if location.import_path == self.own_import_path => model.to_owned(),
            Some(location) => format!("{}.{model}", location.package),
            // A model this run never parsed (a hand-written type, one from
            // another package `cyclonec` was not pointed at): left for the Go
            // compiler to resolve, spelled bare - the same "leave it to the
            // host compiler" policy `super::rust` applies.
            None => model.to_owned(),
        }
    }
}

/// Refuses `Array<Array<T>>` before any text is generated - see the module
/// docs' "known gap".
///
/// # Errors
///
/// A field whose type nests one `Array` inside another.
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

/// Renders one codec file: the header, the package clause, its imports, the
/// codec type, its constants, and its `Encode` / `Decode`.
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

    // -------------------------------------------------------------- encode
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

    // -------------------------------------------------------------- decode
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

/// The `import` block at the top of a codec file - exactly what the file
/// names and nothing else, so a package Go would refuse to compile (an
/// unused import is an error, not a warning) is never emitted.
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

// =================================================================== encoding

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

/// Writes one non-array value - a bare field, or an array element (never an
/// array itself: `check_no_nested_arrays` has already refused that case).
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

// =================================================================== decoding

fn decode_field(out: &mut String, field: &Field, codec: &str, imports: &Imports<'_>) {
    let place = format!("value.{}", field.name);

    // A nested model needs no absence check of its own: its codec asks the
    // same question of every one of its fields, so an absent nested model
    // zeroes them all without reading a byte.
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

/// Declares and reads `var {var} {type}`, strictly - no absence check, since
/// an array element is only ever read because the count already promised it
/// exists (a stream that ends here is truncated, not skewed).
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

/// The Go type name of an array element - the primitive table's own spelling,
/// or a qualified model reference.
fn element_type_name(ty: &WireType, imports: &Imports<'_>) -> String {
    match primitive(ty) {
        Some((_, _, go_type)) => go_type.to_owned(),
        None => imports.qualify(ty.model_name().expect("a non-primitive, non-array type")),
    }
}

/// The model name of a bare model type - `Array<T>` is not one, because an
/// array is decoded element by element and only *its elements* may be
/// models.
fn as_model(ty: &WireType) -> Option<&str> {
    match ty {
        WireType::Model(name) => Some(name),
        _ => None,
    }
}

/// A local variable name that cannot collide with `w`, `r`, `value`, `err`, or
/// another field's own locals in the same method: the field name with its
/// first character lower-cased.
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
        // Same package: no import for the nested codec's own type.
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
