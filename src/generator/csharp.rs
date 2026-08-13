//! One message → one C# file.
//!
//! The C# counterpart of [`super::go`] and [`super::rust`]. Same rule, same
//! shape: a message is walked once, a field at a time, and each field appends
//! one statement. Nothing is analysed on the way: a primitive is a table
//! lookup, and anything else is another model's name spelled into a call.
//!
//! # What C# forces that Go and Rust do not
//!
//! A C# property is not an addressable storage location, so it cannot be
//! passed `ref` directly - unlike a Rust struct field or a Go struct field,
//! both of which are. A nested model field is therefore decoded through a
//! local: read the existing value out, decode into it by `ref`, assign it
//! back (see [`decode_field`]). This preserves the same "leaves fields this
//! codec does not carry alone" guarantee the Rust and Go backends have -
//! including recursively, into the nested model - and it costs nothing when
//! the member turns out to be a plain field instead of a property, which is
//! also legal C# for a Cyclone model.
//!
//! C# has no `import`; a codec file needs nothing added to a `.csproj` to
//! reach a model in a different namespace, because a fully-qualified name
//! (`Models.Player`) compiles without a `using` directive. So unlike
//! [`super::go`], this backend never writes an import block - it always
//! spells a cross-namespace reference out in full, and a bare reference
//! whenever the model is declared in the same namespace this run generates
//! into, or in no namespace at all (C#'s global namespace, reachable
//! unqualified from anywhere that does not shadow it).
//!
//! # The decoder, and RFC-0002 §9.1
//!
//! Identical policy to [`super::rust`] and [`super::go`]: a bare field asks
//! `reader.FieldAbsent()` before it reads, taking its zero value when the
//! stream already ended; array *elements* are read strictly, once the count
//! says they exist; a nested model is decoded through its own codec
//! unconditionally, which asks the same question of every one of *its*
//! fields.
//!
//! # A deliberate gap: `Array<Array<T>>`
//!
//! Refused with a clear error rather than generated wrong, the same choice
//! [`super::go`] makes and for the same reason: the element-type table this
//! generator's whole knowledge of C# types lives in has no entry for
//! `Array<T>` itself, only for what `T` can be. Nesting one array inside
//! another is rare enough in practice that refusing it outright, clearly, was
//! judged better than the extra generator complexity a correct
//! implementation would need.

use crate::ir::{Field, Message, Model, WireType};
use crate::model::snake_case;

use super::codec_type_name;

/// The C# namespace every generated file in one run shares, derived from
/// `--out`'s own directory name - the C# counterpart of
/// [`super::go::package_name_from_out`], PascalCased because that is the
/// .NET convention for a namespace segment where Go's is all lowercase.
pub fn namespace_from_out(out: &std::path::Path) -> String {
    let basename = out
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Generated");
    sanitize_namespace(basename)
}

/// A C# identifier is letters, digits and `_`, may not start with a digit,
/// and may not be a reserved word. `--out`'s basename rarely needs any of
/// this, but a generator that trusts a directory name unchecked is a
/// generator that picks today to find out `--out 2fast` does not compile.
fn sanitize_namespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut capitalize = true;

    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            if capitalize {
                out.extend(character.to_uppercase());
                capitalize = false;
            } else {
                out.push(character);
            }
        } else {
            capitalize = true;
        }
    }

    if out.is_empty() {
        return "Generated".to_owned();
    }
    if out.starts_with(|character: char| character.is_ascii_digit()) {
        out.insert_str(0, "Ns");
    }
    if CSHARP_KEYWORDS.contains(&out.to_lowercase().as_str()) {
        out.push_str("Namespace");
    }
    out
}

const CSHARP_KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "base",
    "bool",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "checked",
    "class",
    "const",
    "continue",
    "decimal",
    "default",
    "delegate",
    "do",
    "double",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "fixed",
    "float",
    "for",
    "foreach",
    "goto",
    "if",
    "implicit",
    "in",
    "int",
    "interface",
    "internal",
    "is",
    "lock",
    "long",
    "namespace",
    "new",
    "null",
    "object",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "ref",
    "return",
    "sbyte",
    "sealed",
    "short",
    "sizeof",
    "stackalloc",
    "static",
    "string",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "uint",
    "ulong",
    "unchecked",
    "unsafe",
    "ushort",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
];

/// The runtime method each primitive maps to, and the C# type it reads or
/// writes.
///
/// This table is the whole of the generator's type knowledge. Every name in
/// it comes from RFC-0002's Reader / Writer interface, spelled the way
/// [`super::csharp_runtime`] spells it; a name that is not in it is another
/// model, and the call is spelled and left for the C# compiler to resolve.
fn primitive(ty: &WireType) -> Option<(&'static str, &'static str, &'static str)> {
    // (writer method, reader method, C# type)
    Some(match ty {
        WireType::Bool => ("WriteBool", "ReadBool", "bool"),
        WireType::I8 => ("WriteI8", "ReadI8", "sbyte"),
        WireType::U8 => ("WriteU8", "ReadU8", "byte"),
        WireType::I16 => ("WriteI16", "ReadI16", "short"),
        WireType::U16 => ("WriteU16", "ReadU16", "ushort"),
        WireType::I32 => ("WriteI32", "ReadI32", "int"),
        WireType::U32 => ("WriteU32", "ReadU32", "uint"),
        WireType::I64 => ("WriteI64", "ReadI64", "long"),
        WireType::U64 => ("WriteU64", "ReadU64", "ulong"),
        WireType::F32 => ("WriteF32", "ReadF32", "float"),
        WireType::F64 => ("WriteF64", "ReadF64", "double"),
        WireType::Str => ("WriteString", "ReadString", "string"),
        WireType::Bytes => ("WriteBytes", "ReadBytes", "byte[]"),
        WireType::Array(_) | WireType::Model(_) => return None,
    })
}

/// The C# zero-value expression for a field the stream ended before
/// (RFC-0002 §9.1). Only ever called for a primitive: an array's absence is
/// its own zero element count, and a model field has no zero expression of
/// its own - see [`decode_field`].
fn zero(ty: &WireType) -> &'static str {
    match ty {
        WireType::Bool => "false",
        WireType::Str => "\"\"",
        WireType::Bytes => "System.Array.Empty<byte>()",
        WireType::F32 => "0f",
        WireType::F64 => "0d",
        WireType::Array(_) => unreachable!("an array's absence is handled by decode_field"),
        WireType::Model(_) => unreachable!("a model field is decoded through its own codec"),
        _ => "0",
    }
}

/// The generated file name: `Player` + `edge` → `player_edge.cs`.
pub fn file_name(model: &str, codec: &str) -> String {
    format!("{}_{}.cs", snake_case(model), snake_case(codec))
}

/// Escapes text embedded inside an XML doc comment's `<c>...</c>`.
///
/// A wire type's own spelling is the only thing here that can contain `<` or
/// `>` (`Array<T>`), and left unescaped it reads as a nested XML element,
/// which is malformed XML doc - `csc` accepts it but warns (CS1570) on every
/// one, which is not a warning a generated file's reader can act on.
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Where every model this run parsed can be reached from inside a generated
/// C# file: `None` for a model declared in no namespace at all (C#'s global
/// namespace), `Some(namespace)` for one declared in one.
pub struct Imports<'a> {
    pub locations: &'a std::collections::BTreeMap<String, Option<String>>,
    /// The namespace the generated file itself opens. A model declared in
    /// this same namespace is spelled bare, the same as one in the global
    /// namespace.
    pub own_namespace: &'a str,
}

impl Imports<'_> {
    /// How a model's type is spelled in generated code: bare if it shares
    /// this run's namespace (or has none), fully qualified otherwise. Never
    /// a `using` - see the module docs.
    fn qualify(&self, model: &str) -> String {
        match self.locations.get(model) {
            Some(Some(namespace)) if namespace != self.own_namespace => {
                format!("{namespace}.{model}")
            }
            // Same namespace, no namespace at all, or a model this run never
            // parsed (a hand-written type): spelled bare, left for the C#
            // compiler to resolve - the same "leave it to the host compiler"
            // policy `super::rust` and `super::go` apply.
            _ => model.to_owned(),
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
                    "model '{}' field '{}': the C# backend does not support `Array<Array<T>>` \
                     - split '{}' into two codecs, or flatten the field",
                    model.name, field.name, field.name,
                ));
            }
        }
    }
    Ok(())
}

/// Renders one codec file: the header, the namespace, the codec type, its
/// constants, and its `Encode` / `Decode`.
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
    out.push_str(&format!("namespace {namespace}\n{{\n\n"));

    let name = codec_type_name(&model.name, &message.codec);
    let model_type = imports.qualify(&model.name);

    out.push_str(&format!(
        "/// <summary>The <c>{}</c> codec for <see cref=\"{}\"/>, generated from its Cyclone \
         attributes.</summary>\n",
        message.codec, model.name
    ));
    if message.fields.is_empty() {
        out.push_str(
            "/// <remarks>This codec carries no fields: it encodes to zero bytes.</remarks>\n",
        );
    } else {
        out.push_str("/// <remarks>\n/// The wire layout, in declaration order (RFC-0002 §5.1):\n");
        out.push_str("/// <list type=\"number\">\n");
        for field in &message.fields {
            out.push_str(&format!(
                "/// <item><description><c>{}</c>: <c>{}</c></description></item>\n",
                field.name,
                xml_escape(&field.ty.spelling())
            ));
        }
        out.push_str("/// </list>\n/// </remarks>\n");
    }
    out.push_str(&format!("public static class {name}\n{{\n"));

    out.push_str("    /// <summary>This message's name: <c>Model.codec</c>.</summary>\n");
    out.push_str(&format!(
        "    public const string MessageName = \"{}\";\n\n",
        message.name
    ));
    out.push_str(
        "    /// <summary>This message's stable id, derived from its name alone.</summary>\n",
    );
    out.push_str(&format!(
        "    public const uint MessageId = 0x{:08X};\n\n",
        message.id
    ));
    out.push_str(
        "    /// <summary>This message's wire-contract fingerprint - the same value\n    \
         /// <c>Handshake.cs</c> publishes, and the one a peer compares against.</summary>\n",
    );
    out.push_str(&format!(
        "    public const ulong Fingerprint = 0x{:016X};\n\n",
        message.fingerprint.u64()
    ));

    // -------------------------------------------------------------- encode
    out.push_str(&format!(
        "    /// <summary>Writes the <c>{}</c> fields of <paramref name=\"value\"/>, in \
         declaration order.</summary>\n",
        message.codec
    ));
    out.push_str(&format!(
        "    public static void Encode(Writer writer, {model_type} value)\n    {{\n"
    ));
    for field in &message.fields {
        out.push_str("        ");
        encode_field(&mut out, field, &message.codec);
        out.push('\n');
    }
    out.push_str("    }\n\n");

    // -------------------------------------------------------------- decode
    out.push_str(&format!(
        "    /// <summary>Reads the <c>{}</c> fields into <paramref name=\"value\"/>, in \
         declaration order.</summary>\n",
        message.codec
    ));
    out.push_str(
        "    /// <remarks>\n    \
         /// Fields this codec does not carry are left as they were, which is what lets one\n    \
         /// model be split across several codecs.\n    \
         ///\n    \
         /// A field the stream ended before takes its zero value (RFC-0002 §9.1); a field\n    \
         /// the stream ended inside is an error. Bytes left over after the last field\n    \
         /// belong to a newer writer's model and are ignored.\n    \
         /// </remarks>\n",
    );
    out.push_str(&format!(
        "    public static void Decode(ref Reader reader, ref {model_type} value)\n    {{\n"
    ));
    for field in &message.fields {
        decode_field(&mut out, field, &message.codec, imports);
    }
    out.push_str("    }\n");

    out.push_str("}\n\n}\n");

    out
}

// =================================================================== encoding

fn encode_field(out: &mut String, field: &Field, codec: &str) {
    let place = format!("value.{}", field.name);

    if let WireType::Array(element_type) = &field.ty {
        out.push_str(&format!("writer.WriteArrayCount({place}.Count);\n"));
        out.push_str("        foreach (var element in ");
        out.push_str(&place);
        out.push_str(")\n        {\n            ");
        encode_scalar(out, element_type, "element", codec);
        out.push_str("\n        }");
        return;
    }

    encode_scalar(out, &field.ty, &place, codec);
}

/// Writes one non-array value - a bare field, or an array element (never an
/// array itself: `check_no_nested_arrays` has already refused that case).
fn encode_scalar(out: &mut String, ty: &WireType, place: &str, codec: &str) {
    match primitive(ty) {
        Some((writer_method, ..)) => {
            out.push_str(&format!("writer.{writer_method}({place});"));
        }
        None => {
            let nested = codec_type_name(
                ty.model_name().expect("a non-primitive, non-array type"),
                codec,
            );
            out.push_str(&format!("{nested}.Encode(writer, {place});"));
        }
    }
}

// =================================================================== decoding

fn decode_field(out: &mut String, field: &Field, codec: &str, imports: &Imports<'_>) {
    let place = format!("value.{}", field.name);

    // A nested model needs no absence check of its own: its codec asks the
    // same question of every one of its fields, so an absent nested model
    // zeroes them all without reading a byte. A C# property is not
    // addressable, so the existing value is read out, decoded into by `ref`,
    // and assigned back - see the module docs.
    if let Some(name) = as_model(&field.ty) {
        let nested = codec_type_name(name, codec);
        let local = local_name(&field.name);
        out.push_str(&format!("        var {local} = {place};\n"));
        out.push_str(&format!(
            "        {nested}.Decode(ref reader, ref {local});\n"
        ));
        out.push_str(&format!("        {place} = {local};\n"));
        return;
    }

    if let WireType::Array(element_type) = &field.ty {
        let local = local_name(&field.name);
        let count_local = format!("{local}Count");
        let list_local = format!("{local}List");
        let element_type_csharp = element_type_name(element_type, imports);

        out.push_str(&format!(
            "        int {count_local} = reader.FieldAbsent() ? 0 : reader.ReadArrayCount();\n"
        ));
        out.push_str(&format!(
            "        var {list_local} = new System.Collections.Generic.List<{element_type_csharp}>({count_local});\n"
        ));
        out.push_str(&format!(
            "        for (int i = 0; i < {count_local}; i++)\n        {{\n"
        ));
        decode_element_into(out, element_type, &list_local, codec, imports);
        out.push_str("        }\n");
        out.push_str(&format!("        {place} = {list_local};\n"));
        return;
    }

    let (_, reader_method, _) = primitive(&field.ty).expect("models and arrays handled above");
    out.push_str(&format!(
        "        {place} = reader.FieldAbsent() ? {} : reader.{reader_method}();\n",
        zero(&field.ty),
    ));
}

/// Decodes one array element - strictly, unlike a field: the count already
/// promised this element exists, so a stream that ends here is truncated,
/// not skewed - and appends it to `list_local`.
fn decode_element_into(
    out: &mut String,
    ty: &WireType,
    list_local: &str,
    codec: &str,
    imports: &Imports<'_>,
) {
    match as_model(ty) {
        Some(name) => {
            let csharp_type = imports.qualify(name);
            let nested = codec_type_name(name, codec);
            out.push_str(&format!("            var element = new {csharp_type}();\n"));
            out.push_str(&format!(
                "            {nested}.Decode(ref reader, ref element);\n"
            ));
            out.push_str(&format!("            {list_local}.Add(element);\n"));
        }
        None => {
            let (_, reader_method, _) = primitive(ty).expect("models handled above");
            out.push_str(&format!(
                "            {list_local}.Add(reader.{reader_method}());\n"
            ));
        }
    }
}

/// `T`'s C# type name for `Array<T>`'s `List<T>` local - the primitive
/// table's own spelling, or a qualified model reference.
fn element_type_name(ty: &WireType, imports: &Imports<'_>) -> String {
    match primitive(ty) {
        Some((_, _, csharp_type)) => csharp_type.to_owned(),
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

/// A local variable name that cannot collide with `writer`, `reader`,
/// `value`, or another field's own locals in the same method.
fn local_name(field_name: &str) -> String {
    let mut chars = field_name.chars();
    let mut local = match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>(),
        None => String::new(),
    };
    local.push_str(chars.as_str());
    local.push_str("Value");
    local
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::{check_no_nested_arrays, codec_file, file_name, namespace_from_out, Imports};
    use crate::ir::Schema;
    use crate::model::{Field, Model};

    fn generated(fields: &[(&str, &str)]) -> String {
        let schema = Schema::build(&[
            Model {
                name: "Player".to_owned(),
                source: PathBuf::from("Models/Player.cs"),
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
                source: PathBuf::from("Models/Player.cs"),
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

        // Colocated with the generated namespace, so every model reference in
        // these tests is bare - the common case, and the one most of them
        // are about. Qualification across namespaces has its own tests
        // below.
        let locations: BTreeMap<String, Option<String>> = ["Player", "PlayerInfo"]
            .into_iter()
            .map(|name| (name.to_owned(), Some("Game.Generated".to_owned())))
            .collect();
        let model = schema.model("Player").expect("model");
        codec_file(
            model,
            &model.messages[0],
            "Game.Generated",
            &Imports {
                locations: &locations,
                own_namespace: "Game.Generated",
            },
        )
    }

    #[test]
    fn a_primitive_reads_and_writes_the_model_directly() {
        let text = generated(&[("Id", "u32")]);
        assert!(text.contains("writer.WriteU32(value.Id);"), "{text}");
        assert!(
            text.contains("value.Id = reader.FieldAbsent() ? 0 : reader.ReadU32();"),
            "{text}"
        );
    }

    #[test]
    fn no_dto_no_mapper_no_intermediate_anything() {
        let text = generated(&[("Id", "u32"), ("Name", "string")]);
        assert!(
            text.contains("public static void Encode(Writer writer, Player value)"),
            "{text}"
        );
        assert!(
            text.contains("public static void Decode(ref Reader reader, ref Player value)"),
            "{text}"
        );
        for forbidden in ["PlayerDTO", "PlayerWire", "PlayerMapper"] {
            assert!(!text.contains(forbidden), "{forbidden} in\n{text}");
        }
    }

    #[test]
    fn a_string_zeroes_to_an_empty_string() {
        let text = generated(&[("Name", "string")]);
        assert!(text.contains("writer.WriteString(value.Name);"), "{text}");
        assert!(
            text.contains("value.Name = reader.FieldAbsent() ? \"\" : reader.ReadString();"),
            "{text}"
        );
    }

    #[test]
    fn a_nested_model_calls_the_same_codec_through_a_local() {
        let text = generated(&[("Info", "PlayerInfo")]);
        assert!(
            text.contains("PlayerInfoEdgeCodec.Encode(writer, value.Info);"),
            "{text}"
        );
        assert!(text.contains("var infoValue = value.Info;"), "{text}");
        assert!(
            text.contains("PlayerInfoEdgeCodec.Decode(ref reader, ref infoValue);"),
            "{text}"
        );
        assert!(text.contains("value.Info = infoValue;"), "{text}");
    }

    #[test]
    fn an_array_counts_first_then_loops_strictly() {
        let text = generated(&[("Tags", "Array<string>")]);
        assert!(
            text.contains("writer.WriteArrayCount(value.Tags.Count);"),
            "{text}"
        );
        assert!(
            text.contains("foreach (var element in value.Tags)"),
            "{text}"
        );
        assert!(text.contains("writer.WriteString(element);"), "{text}");
        assert!(
            text.contains(
                "int tagsValueCount = reader.FieldAbsent() ? 0 : reader.ReadArrayCount();"
            ),
            "{text}"
        );
        assert!(text.contains("List<string>(tagsValueCount);"), "{text}");
        assert!(
            text.contains("tagsValueList.Add(reader.ReadString());"),
            "{text}"
        );
        assert!(text.contains("value.Tags = tagsValueList;"), "{text}");
    }

    #[test]
    fn an_array_of_models_creates_a_fresh_element_each_iteration() {
        let text = generated(&[("Roster", "Array<PlayerInfo>")]);
        assert!(text.contains("var element = new PlayerInfo();"), "{text}");
        assert!(
            text.contains("PlayerInfoEdgeCodec.Decode(ref reader, ref element);"),
            "{text}"
        );
        assert!(text.contains("rosterValueList.Add(element);"), "{text}");
    }

    #[test]
    fn a_model_in_the_same_namespace_is_spelled_bare() {
        let text = generated(&[("Info", "PlayerInfo")]);
        assert!(
            text.contains("public static void Encode(Writer writer, Player value)"),
            "{text}"
        );
        assert!(!text.contains("Game.Models.Player"), "{text}");
    }

    #[test]
    fn a_model_in_a_different_namespace_is_fully_qualified() {
        let schema = Schema::build(&[Model {
            name: "Player".to_owned(),
            source: PathBuf::from("Models/Player.cs"),
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
        let locations: BTreeMap<String, Option<String>> =
            [("Player".to_owned(), Some("Game.Models".to_owned()))]
                .into_iter()
                .collect();
        let model = schema.model("Player").expect("model");
        let text = codec_file(
            model,
            &model.messages[0],
            "Game.Generated",
            &Imports {
                locations: &locations,
                own_namespace: "Game.Generated",
            },
        );
        assert!(
            text.contains("public static void Encode(Writer writer, Game.Models.Player value)"),
            "{text}"
        );
        assert!(
            !text.contains("using "),
            "no using directive is ever needed:\n{text}"
        );
    }

    #[test]
    fn a_model_in_no_namespace_at_all_is_spelled_bare() {
        let schema = Schema::build(&[Model {
            name: "Player".to_owned(),
            source: PathBuf::from("Player.cs"),
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
        let locations: BTreeMap<String, Option<String>> =
            [("Player".to_owned(), None)].into_iter().collect();
        let model = schema.model("Player").expect("model");
        let text = codec_file(
            model,
            &model.messages[0],
            "Game.Generated",
            &Imports {
                locations: &locations,
                own_namespace: "Game.Generated",
            },
        );
        assert!(
            text.contains("public static void Encode(Writer writer, Player value)"),
            "{text}"
        );
    }

    #[test]
    fn nested_arrays_are_refused_rather_than_generated_wrong() {
        let model = Model {
            name: "Grid".to_owned(),
            source: PathBuf::from("Models/Grid.cs"),
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
            "Generated"
        );
        assert_eq!(namespace_from_out(std::path::Path::new("gen")), "Gen");
        assert_eq!(namespace_from_out(std::path::Path::new("2fast")), "Ns2fast");
        assert_eq!(
            namespace_from_out(std::path::Path::new("my-service")),
            "MyService"
        );
        assert_eq!(
            namespace_from_out(std::path::Path::new("class")),
            "ClassNamespace"
        );
    }

    #[test]
    fn a_codec_file_is_named_like_the_other_backends() {
        assert_eq!(file_name("Player", "edge"), "player_edge.cs");
        assert_eq!(
            file_name("PlayerInfo", "orange_pi"),
            "player_info_orange_pi.cs"
        );
    }

    #[test]
    fn a_codec_with_no_fields_still_compiles() {
        let text = generated(&[]);
        assert!(
            text.contains("public static void Encode(Writer writer, Player value)\n    {\n    }"),
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
        assert!(text.contains("// source: Models/Player.cs\n"), "{text}");
        assert!(text.contains("// model: Player\n"), "{text}");
        assert!(text.contains("// codec: edge\n"), "{text}");
        assert!(text.contains("// fingerprint: sha256:"), "{text}");
        assert!(text.contains("namespace Game.Generated\n"), "{text}");
    }

    #[test]
    fn the_constants_are_generated_not_hand_written() {
        let text = generated(&[("Id", "u32")]);
        assert!(
            text.contains("public const string MessageName = \"Player.edge\";"),
            "{text}"
        );
        assert!(text.contains("public const uint MessageId = 0x"), "{text}");
        assert!(
            text.contains("public const ulong Fingerprint = 0x"),
            "{text}"
        );
    }
}
