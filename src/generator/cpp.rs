//! One message → one C++ header.
//!
//! The C++ counterpart of [`super::rust`], [`super::go`] and [`super::csharp`].
//! Same rule, same shape: a message is walked once, a field at a time, and
//! each field appends one statement. Nothing is analysed on the way: a
//! primitive is a table lookup, and anything else is another model's name
//! spelled into a call.
//!
//! # Header-only, on purpose
//!
//! There is no `.cpp` half. Every method a generated `struct` declares is
//! defined inside the struct body, which makes it implicitly `inline` -
//! `#include`d from as many translation units as a project likes, with no
//! separate compilation unit to add to a build and no risk of an ODR
//! violation. This is the same reason [`super::gdscript`] settled on one
//! `class_name` per file: the shape that needs the least from the surrounding
//! build system wins.
//!
//! # What C++ needs that C#, Go and Rust do not
//!
//! A C++ codec is a physical `#include` away from the model it encodes, not
//! only a name in scope: unlike C#'s "a fully-qualified name compiles with no
//! `using`" or Go's "one `import` reaches a whole package", the header that
//! declares `DeviceState` has to be named explicitly by every generated file
//! that touches it. [`ModelLocation::include`] is where that path lives, read
//! from the model's own source location; [`ModelLocation::namespace`] is the
//! (optional) C++ namespace the model's own source opens, read the same way
//! [`super::csharp::namespace_name`] reads C#'s - and unlike C#, a namespaced
//! reference is always written with a leading `::` (`::Game::Models::Player`),
//! because C++ - unlike C# - has no rule against a self-referential qualified
//! name, so there is no "same namespace" special case to get right and no
//! "own namespace" to track at all.
//!
//! # A generated model's fields are plain members, never properties
//!
//! Which means, unlike [`super::csharp`], a nested model's field is always
//! addressable and can always be passed by reference directly - the same
//! simplicity [`super::rust`] and [`super::go`] have, and C#'s `var
//! local = value.Field; ...; value.Field = local;` workaround is never
//! needed here.
//!
//! # The decoder, and RFC-0002 §9.1
//!
//! Identical policy to every other backend: a bare field asks
//! `reader.field_absent()` before it reads, taking its zero value when the
//! stream already ended; array *elements* are read strictly, once the count
//! says they exist; a nested model is decoded through its own codec
//! unconditionally, which asks the same question of every one of *its*
//! fields.
//!
//! # A deliberate gap: `Array<Array<T>>`
//!
//! Refused with a clear error rather than generated wrong - the same choice
//! [`super::go`], [`super::csharp`] and [`super::gdscript`] make, and for the
//! same reason: the element-type table this generator's whole knowledge of
//! C++ types lives in has no entry for `Array<T>` itself, only for what `T`
//! can be.

use std::collections::BTreeSet;

use crate::ir::{Field, Message, Model, WireType};
use crate::model::snake_case;

use super::codec_type_name;

/// The C++ namespace every generated file in one run shares, derived from
/// `--out`'s own directory name - the C++ counterpart of
/// [`super::go::package_name_from_out`] and [`super::csharp::namespace_from_out`],
/// lowercase because that is the more common convention for a C++ namespace
/// (`std`, `boost`, ...) where C#'s is PascalCase.
pub fn namespace_from_out(out: &std::path::Path) -> String {
    let basename = out
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("generated");
    sanitize_namespace(basename)
}

/// A C++ identifier is letters, digits and `_`, may not start with a digit,
/// and may not be a reserved word. `--out`'s basename rarely needs any of
/// this, but a generator that trusts a directory name unchecked is a
/// generator that picks today to find out `--out 2fast` does not compile.
fn sanitize_namespace(text: &str) -> String {
    let mut out: String = text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect();

    if out.is_empty() {
        return "generated".to_owned();
    }
    if out.starts_with(|character: char| character.is_ascii_digit()) {
        out.insert_str(0, "ns");
    }
    if CPP_KEYWORDS.contains(&out.as_str()) {
        out.push_str("_ns");
    }
    out
}

const CPP_KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "asm",
    "auto",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "class",
    "concept",
    "const",
    "consteval",
    "constexpr",
    "continue",
    "decltype",
    "default",
    "delete",
    "do",
    "double",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "false",
    "float",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "nullptr",
    "operator",
    "private",
    "protected",
    "public",
    "register",
    "requires",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "template",
    "this",
    "thread_local",
    "throw",
    "true",
    "try",
    "typedef",
    "typeid",
    "typename",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
    "wchar_t",
    "while",
];

/// The runtime method each primitive maps to, and the C++ type it reads or
/// writes.
///
/// This table is the whole of the generator's type knowledge. Every name in
/// it comes from RFC-0002's Reader / Writer interface, spelled the way
/// [`super::cpp_runtime`] spells it; a name that is not in it is another
/// model, and the call is spelled and left for the C++ compiler to resolve.
fn primitive(ty: &WireType) -> Option<(&'static str, &'static str, &'static str)> {
    // (writer method, reader method, C++ type)
    Some(match ty {
        WireType::Bool => ("write_bool", "read_bool", "bool"),
        WireType::I8 => ("write_i8", "read_i8", "std::int8_t"),
        WireType::U8 => ("write_u8", "read_u8", "std::uint8_t"),
        WireType::I16 => ("write_i16", "read_i16", "std::int16_t"),
        WireType::U16 => ("write_u16", "read_u16", "std::uint16_t"),
        WireType::I32 => ("write_i32", "read_i32", "std::int32_t"),
        WireType::U32 => ("write_u32", "read_u32", "std::uint32_t"),
        WireType::I64 => ("write_i64", "read_i64", "std::int64_t"),
        WireType::U64 => ("write_u64", "read_u64", "std::uint64_t"),
        WireType::F32 => ("write_f32", "read_f32", "float"),
        WireType::F64 => ("write_f64", "read_f64", "double"),
        WireType::Str => ("write_string", "read_string", "std::string"),
        WireType::Bytes => ("write_bytes", "read_bytes", "std::vector<std::uint8_t>"),
        WireType::Array(_) | WireType::Model(_) => return None,
    })
}

/// The C++ zero-value expression for a field the stream ended before
/// (RFC-0002 §9.1). Only ever called for a primitive: an array's absence is
/// its own zero element count, and a model field has no zero expression of
/// its own - see [`decode_field`].
fn zero(ty: &WireType) -> &'static str {
    match ty {
        WireType::Bool => "false",
        WireType::Str => "std::string()",
        WireType::Bytes => "std::vector<std::uint8_t>()",
        WireType::F32 => "0.0f",
        WireType::F64 => "0.0",
        WireType::Array(_) => unreachable!("an array's absence is handled by decode_field"),
        WireType::Model(_) => unreachable!("a model field is decoded through its own codec"),
        _ => "0",
    }
}

/// The generated file name: `Player` + `edge` → `player_edge.hpp`.
pub fn file_name(model: &str, codec: &str) -> String {
    format!("{}_{}.hpp", snake_case(model), snake_case(codec))
}

/// Where one model's C++ type is declared.
pub struct ModelLocation {
    /// The `#include` path of the header the model's own source lives in,
    /// e.g. `src/models/device_state.hpp` - always the model's own source
    /// path exactly as `--src` and the file system gave it (the same path
    /// `schema.json`'s own `source` field records), and never affected by
    /// `--model-path`: the physical header a generated file has to name is
    /// not something any override can change, only where the compiler is
    /// told to look for it (a `-I` search path rooted wherever that source
    /// path is itself rooted from - typically the project root, where
    /// `cyclone.toml` lives - which is the project's concern, not this
    /// generator's).
    pub include: String,
    /// The namespace the model's own source opens, if any - `--model-path`
    /// overrides this uniformly for every model, the same as it does for
    /// C#'s namespace and Go's import path.
    pub namespace: Option<String>,
}

/// Where every model this run parsed can be reached from inside a generated
/// C++ file.
pub struct Imports<'a> {
    pub locations: &'a std::collections::BTreeMap<String, ModelLocation>,
}

impl Imports<'_> {
    /// How a model's type is spelled in generated code: always fully
    /// qualified with a leading `::`, whether or not it has a namespace -
    /// `::Game::Models::Player`, or bare `::Player` for one in no namespace
    /// at all. Unlike Go's or C#'s `qualify`, this needs no "is it the same
    /// namespace as the one this file opens" case: a leading `::` always
    /// compiles, from anywhere, so there is nothing to get wrong by always
    /// writing one.
    fn qualify(&self, model: &str) -> String {
        match self
            .locations
            .get(model)
            .and_then(|location| location.namespace.as_deref())
        {
            Some(namespace) => format!("::{namespace}::{model}"),
            // No namespace, or a model this run never parsed (a hand-written
            // type): spelled bare with a leading `::`, left for the C++
            // compiler to resolve - the same "leave it to the host compiler"
            // policy every other backend applies.
            None => format!("::{model}"),
        }
    }
}

/// Refuses `Array<Array<T>>` before any text is generated - see the module
/// docs' "deliberate gap".
///
/// # Errors
///
/// A field whose type nests one `Array` inside another.
pub fn check_no_nested_arrays(model: &Model) -> Result<(), String> {
    for field in &model.fields {
        if let WireType::Array(element) = &field.ty {
            if matches!(element.as_ref(), WireType::Array(_)) {
                return Err(format!(
                    "model '{}' field '{}': the C++ backend does not support `Array<Array<T>>` \
                     - split '{}' into two codecs, or flatten the field",
                    model.name, field.name, field.name,
                ));
            }
        }
    }
    Ok(())
}

/// Renders one codec file: the header, the `#include`s, the codec type, its
/// constants, and its `encode` / `decode`.
pub fn codec_file(
    model: &Model,
    message: &Message,
    namespace: &str,
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
    out.push_str("#pragma once\n\n");

    write_includes(&mut out, model, message, imports);

    out.push_str(&format!("namespace {namespace} {{\n\n"));

    let name = codec_type_name(&model.name, &message.codec);
    let model_type = imports.qualify(&model.name);

    out.push_str(&format!(
        "/// The {:?} codec for {}, generated from its Cyclone markers.\n",
        message.codec, model.name
    ));
    if message.fields.is_empty() {
        out.push_str("///\n/// This codec carries no fields: it encodes to zero bytes.\n");
    } else {
        out.push_str("///\n/// The wire layout, in declaration order (RFC-0002 §5.1):\n///\n");
        for (index, field) in message.fields.iter().enumerate() {
            out.push_str(&format!(
                "///  {index}. `{}`: `{}`\n",
                field.name,
                field.ty.spelling()
            ));
        }
    }
    out.push_str(&format!("struct {name} {{\n"));

    out.push_str("    /// This message's name: `Model.codec`.\n");
    out.push_str(&format!(
        "    static constexpr const char* kMessageName = {:?};\n\n",
        message.name
    ));
    out.push_str("    /// This message's stable id, derived from its name alone.\n");
    out.push_str(&format!(
        "    static constexpr std::uint32_t kMessageId = 0x{:08X}u;\n\n",
        message.id
    ));
    out.push_str(
        "    /// This message's wire-contract fingerprint - the same value\n    \
         /// handshake.hpp publishes, and the one a peer compares against.\n",
    );
    out.push_str(&format!(
        "    static constexpr std::uint64_t kFingerprint = 0x{:016X}ULL;\n\n",
        message.fingerprint.u64()
    ));

    // -------------------------------------------------------------- encode
    out.push_str(&format!(
        "    /// Writes the {:?} fields of `value`, in declaration order.\n",
        message.codec
    ));
    if message.fields.is_empty() {
        out.push_str(&format!(
            "    static void encode(Writer&, const {model_type}&) {{}}\n\n"
        ));
    } else {
        out.push_str(&format!(
            "    static void encode(Writer& writer, const {model_type}& value) {{\n"
        ));
        for field in &message.fields {
            encode_field(&mut out, field, &message.codec);
        }
        out.push_str("    }\n\n");
    }

    // -------------------------------------------------------------- decode
    out.push_str(&format!(
        "    /// Reads the {:?} fields into `value`, in declaration order.\n",
        message.codec
    ));
    out.push_str(
        "    ///\n    \
         /// Fields this codec does not carry are left as they were, which is what lets one\n    \
         /// model be split across several codecs.\n    \
         ///\n    \
         /// A field the stream ended before takes its zero value (RFC-0002 §9.1); a field\n    \
         /// the stream ended inside is an error. Bytes left over after the last field\n    \
         /// belong to a newer writer's model and are ignored.\n",
    );
    if message.fields.is_empty() {
        out.push_str(&format!(
            "    static DecodeError decode(Reader&, {model_type}&) {{ return DecodeError{{}}; }}\n"
        ));
    } else {
        out.push_str(&format!(
            "    static DecodeError decode(Reader& reader, {model_type}& value) {{\n"
        ));
        for field in &message.fields {
            decode_field(&mut out, field, &message.codec, imports);
        }
        out.push_str("        return DecodeError{};\n");
        out.push_str("    }\n");
    }

    out.push_str("};\n\n");
    out.push_str(&format!("}}  // namespace {namespace}\n"));

    out
}

/// The `#include` block at the top of a codec file: the runtime, the model
/// this codec encodes and every model an array element spells out by name,
/// and the codec headers of every nested model this one calls.
fn write_includes(out: &mut String, model: &Model, message: &Message, imports: &Imports<'_>) {
    out.push_str("#include <cstddef>\n");
    out.push_str("#include <cstdint>\n");
    out.push_str("#include <string>\n");
    out.push_str("#include <vector>\n\n");
    out.push_str("#include \"runtime.hpp\"\n");

    let mut spelled: BTreeSet<&str> = BTreeSet::new();
    spelled.insert(&model.name);
    for field in &message.fields {
        super::spelled_types(&field.ty, &mut spelled);
    }

    let mut includes: BTreeSet<&str> = BTreeSet::new();
    for name in &spelled {
        if let Some(location) = imports.locations.get(*name) {
            includes.insert(location.include.as_str());
        }
    }
    for include in includes {
        out.push_str(&format!("#include \"{include}\"\n"));
    }

    // The codecs this one calls for its nested models.
    let mut codecs: BTreeSet<String> = BTreeSet::new();
    for field in &message.fields {
        let Some(name) = field.ty.model_name() else {
            continue;
        };
        // A model that references itself is in this very file already.
        if name == model.name {
            continue;
        }
        codecs.insert(file_name(name, &message.codec));
    }
    for include in codecs {
        out.push_str(&format!("#include \"{include}\"\n"));
    }

    out.push('\n');
}

// =================================================================== encoding

fn encode_field(out: &mut String, field: &Field, codec: &str) {
    let place = format!("value.{}", field.name);

    if let WireType::Array(element_type) = &field.ty {
        out.push_str(&format!(
            "        writer.write_array_count({place}.size());\n"
        ));
        out.push_str(&format!("        for (const auto& element : {place}) {{\n"));
        encode_scalar(out, element_type, "element", codec, "            ");
        out.push_str("        }\n");
        return;
    }

    encode_scalar(out, &field.ty, &place, codec, "        ");
}

/// Writes one non-array value - a bare field, or an array element (never an
/// array itself: `check_no_nested_arrays` has already refused that case).
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
            out.push_str(&format!("{pad}{nested}::encode(writer, {place});\n"));
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
        out.push_str(&format!(
            "        if (DecodeError error = {nested}::decode(reader, {place}); !error.ok()) \
             return error;\n"
        ));
        return;
    }

    if let WireType::Array(element_type) = &field.ty {
        let element_type_cpp = element_type_name(element_type, imports);

        out.push_str("        {\n");
        out.push_str("            std::size_t count = 0;\n");
        out.push_str("            if (!reader.field_absent()) {\n");
        out.push_str(
            "                if (DecodeError error = reader.read_array_count(count); \
             !error.ok()) return error;\n",
        );
        out.push_str("            }\n");
        out.push_str(&format!(
            "            std::vector<{element_type_cpp}> elements;\n"
        ));
        out.push_str("            elements.reserve(count);\n");
        out.push_str("            for (std::size_t i = 0; i < count; ++i) {\n");
        decode_element_into(out, element_type, "elements", codec, imports);
        out.push_str("            }\n");
        out.push_str(&format!("            {place} = std::move(elements);\n"));
        out.push_str("        }\n");
        return;
    }

    let (_, reader_method, _) = primitive(&field.ty).expect("models and arrays handled above");
    out.push_str("        if (reader.field_absent()) {\n");
    out.push_str(&format!("            {place} = {};\n", zero(&field.ty)));
    out.push_str("        } else {\n");
    out.push_str(&format!(
        "            if (DecodeError error = reader.{reader_method}({place}); !error.ok()) \
         return error;\n"
    ));
    out.push_str("        }\n");
}

/// Decodes one array element - strictly, unlike a field: the count already
/// promised this element exists, so a stream that ends here is truncated, not
/// skewed - and appends it to `elements_local`.
fn decode_element_into(
    out: &mut String,
    ty: &WireType,
    elements_local: &str,
    codec: &str,
    imports: &Imports<'_>,
) {
    match as_model(ty) {
        Some(name) => {
            let nested = codec_type_name(name, codec);
            let cpp_type = imports.qualify(name);
            out.push_str(&format!("                {cpp_type} element{{}};\n"));
            out.push_str(&format!(
                "                if (DecodeError error = {nested}::decode(reader, element); \
                 !error.ok()) return error;\n"
            ));
            out.push_str(&format!(
                "                {elements_local}.push_back(std::move(element));\n"
            ));
        }
        None => {
            let (_, reader_method, cpp_type) = primitive(ty).expect("models handled above");
            out.push_str(&format!("                {cpp_type} element{{}};\n"));
            out.push_str(&format!(
                "                if (DecodeError error = reader.{reader_method}(element); \
                 !error.ok()) return error;\n"
            ));
            out.push_str(&format!(
                "                {elements_local}.push_back(std::move(element));\n"
            ));
        }
    }
}

/// `T`'s C++ type name for `Array<T>`'s `std::vector<T>` local - the
/// primitive table's own spelling, or a qualified model reference.
fn element_type_name(ty: &WireType, imports: &Imports<'_>) -> String {
    match primitive(ty) {
        Some((_, _, cpp_type)) => cpp_type.to_owned(),
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::{
        check_no_nested_arrays, codec_file, file_name, namespace_from_out, Imports, ModelLocation,
    };
    use crate::ir::Schema;
    use crate::model::{Field, Model};

    fn generated(fields: &[(&str, &str)]) -> String {
        let schema = Schema::build(&[
            Model {
                name: "Player".to_owned(),
                source: PathBuf::from("models/player.hpp"),
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
                source: PathBuf::from("models/player.hpp"),
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
                        include: "models/player.hpp".to_owned(),
                        namespace: None,
                    },
                )
            })
            .collect();
        let model = schema.model("Player").expect("model");
        codec_file(
            model,
            &model.messages[0],
            "generated",
            &Imports {
                locations: &locations,
            },
        )
    }

    #[test]
    fn a_primitive_reads_and_writes_the_model_directly() {
        let text = generated(&[("Id", "u32")]);
        assert!(text.contains("writer.write_u32(value.Id);"), "{text}");
        assert!(
            text.contains(
                "if (reader.field_absent()) {\n            value.Id = 0;\n        } else {"
            ),
            "{text}"
        );
        assert!(
            text.contains("reader.read_u32(value.Id); !error.ok()) return error;"),
            "{text}"
        );
    }

    #[test]
    fn no_dto_no_mapper_no_intermediate_anything() {
        let text = generated(&[("Id", "u32"), ("Name", "string")]);
        assert!(
            text.contains("static void encode(Writer& writer, const ::Player& value)"),
            "{text}"
        );
        assert!(
            text.contains("static DecodeError decode(Reader& reader, ::Player& value)"),
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
    fn a_string_zeroes_to_an_empty_string() {
        let text = generated(&[("Name", "string")]);
        assert!(text.contains("writer.write_string(value.Name);"), "{text}");
        assert!(text.contains("value.Name = std::string();"), "{text}");
    }

    #[test]
    fn a_nested_model_calls_the_same_codec_by_reference_directly() {
        let text = generated(&[("Info", "PlayerInfo")]);
        assert!(
            text.contains("PlayerInfoEdgeCodec::encode(writer, value.Info);"),
            "{text}"
        );
        assert!(
            text.contains(
                "if (DecodeError error = PlayerInfoEdgeCodec::decode(reader, value.Info); \
                 !error.ok()) return error;"
            ),
            "{text}"
        );
        // No C#-style local round-trip: the field is addressable directly.
        assert!(!text.contains("var infoValue"), "{text}");
    }

    #[test]
    fn an_array_counts_first_then_loops_strictly() {
        let text = generated(&[("Tags", "Array<string>")]);
        assert!(
            text.contains("writer.write_array_count(value.Tags.size());"),
            "{text}"
        );
        assert!(
            text.contains("for (const auto& element : value.Tags) {"),
            "{text}"
        );
        assert!(text.contains("writer.write_string(element);"), "{text}");
        assert!(text.contains("std::size_t count = 0;"), "{text}");
        assert!(
            text.contains("std::vector<std::string> elements;"),
            "{text}"
        );
        assert!(text.contains("std::string element{};"), "{text}");
        assert!(
            text.contains("elements.push_back(std::move(element));"),
            "{text}"
        );
        assert!(text.contains("value.Tags = std::move(elements);"), "{text}");
    }

    #[test]
    fn an_array_of_models_creates_a_fresh_element_each_iteration() {
        let text = generated(&[("Roster", "Array<PlayerInfo>")]);
        assert!(text.contains("::PlayerInfo element{};"), "{text}");
        assert!(
            text.contains(
                "if (DecodeError error = PlayerInfoEdgeCodec::decode(reader, element); \
                 !error.ok()) return error;"
            ),
            "{text}"
        );
    }

    #[test]
    fn nested_arrays_are_refused_rather_than_generated_wrong() {
        let model = Model {
            name: "Grid".to_owned(),
            source: PathBuf::from("models/grid.hpp"),
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
    fn a_namespace_is_derived_from_outs_basename() {
        assert_eq!(
            namespace_from_out(std::path::Path::new("src/generated")),
            "generated"
        );
        assert_eq!(namespace_from_out(std::path::Path::new("gen")), "gen");
        assert_eq!(namespace_from_out(std::path::Path::new("2fast")), "ns2fast");
        assert_eq!(
            namespace_from_out(std::path::Path::new("my-service")),
            "myservice"
        );
        assert_eq!(
            namespace_from_out(std::path::Path::new("class")),
            "class_ns"
        );
    }

    #[test]
    fn a_codec_file_is_named_like_the_other_backends() {
        assert_eq!(file_name("Player", "edge"), "player_edge.hpp");
        assert_eq!(
            file_name("PlayerInfo", "orange_pi"),
            "player_info_orange_pi.hpp"
        );
    }

    #[test]
    fn a_codec_with_no_fields_still_compiles() {
        let text = generated(&[]);
        assert!(
            text.contains("static void encode(Writer&, const ::Player&) {}"),
            "{text}"
        );
        assert!(
            text.contains(
                "static DecodeError decode(Reader&, ::Player&) { return DecodeError{}; }"
            ),
            "{text}"
        );
    }

    #[test]
    fn the_file_carries_the_headers_the_brief_asks_for() {
        let text = generated(&[("Id", "u32")]);
        assert!(
            text.starts_with("// GENERATED BY cyclonec\n// DO NOT EDIT MANUALLY\n"),
            "{text}"
        );
        assert!(text.contains("// source: models/player.hpp\n"), "{text}");
        assert!(text.contains("// model: Player\n"), "{text}");
        assert!(text.contains("// codec: edge\n"), "{text}");
        assert!(text.contains("// fingerprint: sha256:"), "{text}");
        assert!(text.contains("#pragma once\n"), "{text}");
        assert!(text.contains("namespace generated {\n"), "{text}");
        assert!(text.contains("}  // namespace generated\n"), "{text}");
    }

    #[test]
    fn the_constants_are_generated_not_hand_written() {
        let text = generated(&[("Id", "u32")]);
        assert!(
            text.contains("static constexpr const char* kMessageName = \"Player.edge\";"),
            "{text}"
        );
        assert!(
            text.contains("static constexpr std::uint32_t kMessageId = 0x"),
            "{text}"
        );
        assert!(
            text.contains("static constexpr std::uint64_t kFingerprint = 0x"),
            "{text}"
        );
        assert!(text.contains("ULL;"), "{text}");
    }

    #[test]
    fn a_model_in_a_different_namespace_is_fully_qualified_with_a_leading_scope() {
        let schema = Schema::build(&[Model {
            name: "Player".to_owned(),
            source: PathBuf::from("models/player.hpp"),
            line: 1,
            codecs: vec!["edge".to_owned()],
            fields: vec![Field {
                name: "Id".to_owned(),
                network_type: "u32".to_owned(),
                codecs: vec!["edge".to_owned()],
                line: 2,
            }],
        }])
        .expect("build");
        let locations: BTreeMap<String, ModelLocation> = [(
            "Player".to_owned(),
            ModelLocation {
                include: "models/player.hpp".to_owned(),
                namespace: Some("Game::Models".to_owned()),
            },
        )]
        .into_iter()
        .collect();
        let model = schema.model("Player").expect("model");
        let text = codec_file(
            model,
            &model.messages[0],
            "generated",
            &Imports {
                locations: &locations,
            },
        );
        assert!(
            text.contains(
                "static void encode(Writer& writer, const ::Game::Models::Player& value)"
            ),
            "{text}"
        );
        assert!(text.contains("#include \"models/player.hpp\"\n"), "{text}");
    }

    #[test]
    fn a_bare_nested_model_includes_its_codec_header() {
        let text = generated(&[("Info", "PlayerInfo")]);
        assert!(
            text.contains("#include \"player_info_edge.hpp\"\n"),
            "{text}"
        );
    }
}
