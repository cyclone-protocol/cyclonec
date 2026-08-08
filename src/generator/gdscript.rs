//! [`Model`]s → GDScript source.
//!
//! The GDScript counterpart of [`super::rust`], [`super::csharp`] and
//! [`super::go`]. Same rule, same shape: a model is walked once, a codec at a
//! time, a field at a time, and each field appends one statement. The network
//! type is looked up in [`PRIMITIVES`], and anything not in that table is
//! another model's name, spelled into a call - `cyclonec` never checks that
//! the call resolves; Godot's own script compiler does.
//!
//! # One file, one wrapper class
//!
//! GDScript's global namespace only works through a file's own single
//! `class_name` - there is no Rust `include!`, no C# "any class in the
//! project is a bare name," no Go "same package." So every runtime type and
//! every codec this file generates is declared as its own `class Name:`
//! block *inside this one file*, one fixed wrapper name -
//! [`WRAPPER_CLASS_NAME`] - away from being reachable anywhere in the project
//! as `CycloneCodec.Writer`, `CycloneCodec.PlayerEdgeCodec`, ... with nothing
//! to `preload` - the closest GDScript comes to the other three backends'
//! "nothing to add, nothing to import" guarantee.
//!
//! **Not textually indented under `class_name`.** `class_name` names the
//! file's own already-implicit top-level class; it does not open an indented
//! block. Every `class Writer:` / `class Reader:` / `class PlayerEdgeCodec:`
//! this module emits therefore sits at column 0, as a sibling declaration in
//! the same file - Godot still exposes each one as `WRAPPER_CLASS_NAME.Name`
//! from any other script, because that is how a script's own inner classes
//! are reached, but writing them indented (as if `class_name` opened a
//! block) is a parse error caught only by actually running the generated
//! file through Godot - see the final report for how this was found.
//!
//! # Kept equivalent to the other three backends
//!
//! All four backends read the identical [`Model`] / [`Field`] shape, produce
//! the same codec name for the same schema (`DeviceState` + `edge` →
//! `DeviceStateEdgeCodec`), route fields to codecs by the same rule, and call
//! a runtime method that performs the same RFC-0002 byte layout -
//! [`super::gdscript_runtime`] is written against the same specification as
//! the other three, method for method (`write_u32`/`read_u32`, matching Go's
//! and Rust's own snake_case spelling, since GDScript's style guide is
//! snake_case for functions the same way).
//!
//! # What GDScript forces that the others do not
//!
//! GDScript has no exceptions (see [`super::gdscript_runtime`]'s module
//! docs), so `decode` returns a `DecodeError`, or `null` on success, checked
//! after every field - the same explicit-check shape Go's `error` already
//! forces here, not C#'s `throw`.
//!
//! Every codec is an ordinary class instantiated with `.new()`, never a
//! `static func` - see [`super::gdscript_runtime`] for why the "static inside
//! a nested class" question was avoided rather than guessed at. A nested
//! model field is still passed and mutated directly, with no C#-style
//! local-variable dance: a GDScript field holding another Cyclone model holds
//! an `Object`, which - like a Rust `&mut` or a Go pointer, and unlike a C#
//! property - is already a reference the callee can fill in place.

use crate::model::{pascal_case, Field, Model};

/// The runtime method each network type maps to.
///
/// Every name in this table comes from RFC-0002's Reader / Writer interface,
/// spelled the way [`super::gdscript_runtime`] spells it. A name that is not
/// in it is not looked up, resolved, or imported - it is another model, and
/// the call is spelled and left for Godot's script compiler to resolve or
/// reject.
const PRIMITIVES: &[(&str, &str, &str)] = &[
    // (network type, writer method, reader method)
    ("bool", "write_bool", "read_bool"),
    ("i8", "write_i8", "read_i8"),
    ("u8", "write_u8", "read_u8"),
    ("i16", "write_i16", "read_i16"),
    ("u16", "write_u16", "read_u16"),
    ("i32", "write_i32", "read_i32"),
    ("u32", "write_u32", "read_u32"),
    ("i64", "write_i64", "read_i64"),
    ("u64", "write_u64", "read_u64"),
    ("f32", "write_f32", "read_f32"),
    ("f64", "write_f64", "read_f64"),
    ("string", "write_string", "read_string"),
    ("bytes", "write_bytes", "read_bytes"),
];

/// The name of the generated file when the output path names a directory.
pub const DEFAULT_FILE_NAME: &str = "cyclone.codec.gd";

/// The suffix a GDScript output path is recognised by.
pub const EXTENSION: &str = "gd";

/// The one `class_name` the generated file declares. Every runtime type and
/// every codec is reached through it (`CycloneCodec.Writer`, ...) - see the
/// module header for why that is not the same as being textually nested
/// under it.
pub const WRAPPER_CLASS_NAME: &str = "CycloneCodec";

/// Renders one self-contained file: the `class_name` wrapper, the runtime,
/// then every codec every GDScript model declared.
///
/// See [`super::rust::render`] - same contract, same determinism guarantee.
pub fn render(sources: &[String], models: &[Model], file_name: &str) -> Option<String> {
    if models.iter().all(|model| model.codecs.is_empty()) {
        return None;
    }

    let mut out = String::with_capacity(16 * 1024);

    out.push_str(&format!("class_name {WRAPPER_CLASS_NAME}\n"));
    out.push_str("\n# Generated by cyclonec. Do not edit.\n");
    out.push_str("#\n");
    out.push_str("# Sources:\n");
    for source in sources {
        out.push_str(&format!("#     {source}\n"));
    }
    out.push_str("#\n");
    out.push_str("# This file is self-contained: it carries the Cyclone runtime (Writer,\n");
    out.push_str("# Reader, DecodeError, Limits) as well as every codec, nested inside this\n");
    out.push_str(&format!(
        "# file's own class_name ({WRAPPER_CLASS_NAME}), reachable project-wide with\n"
    ));
    out.push_str(&format!(
        "# nothing to preload. Add it to your project ({file_name}) and it compiles.\n"
    ));
    out.push_str("#\n");
    out.push_str(
        "# A nested model field must already hold an instance (`= PlayerInfo.new()`) before\n",
    );
    out.push_str("# decode fills its routed fields in place.\n");

    out.push_str(super::gdscript_runtime::RUNTIME);

    out.push_str("\n# ======================================================================\n");
    out.push_str("# Codecs - one per codec each model declared, in declaration order.\n");
    out.push_str("# ======================================================================\n");

    for model in models {
        for codec in &model.codecs {
            out.push('\n');
            codec_type(&mut out, model, codec);
        }
    }

    Some(out)
}

/// Writes one codec: the inner class, and its `encode` / `decode` instance
/// methods.
fn codec_type(out: &mut String, model: &Model, codec: &str) {
    let name = codec_name(&model.name, codec);
    let model_name = &model.name;

    out.push_str(&format!(
        "# {name} is the {codec:?} codec for {model_name}, generated from its Cyclone \
         attributes.\n"
    ));
    out.push_str(&format!("class {name}:\n"));

    // -------------------------------------------------------------- encode
    out.push_str(&format!(
        "\t# Writes the {codec:?} fields of value, in declaration order.\n"
    ));
    out.push_str(&format!(
        "\tfunc encode(writer: Writer, value: {model_name}) -> void:\n"
    ));
    let fields: Vec<&Field> = model.fields_in(codec).collect();
    if fields.is_empty() {
        out.push_str("\t\tpass\n");
    } else {
        for field in &fields {
            out.push_str("\t\t");
            encode_field(out, field, codec);
            out.push('\n');
        }
    }
    out.push('\n');

    // -------------------------------------------------------------- decode
    out.push_str(&format!(
        "\t# Reads the {codec:?} fields into value, in declaration order.\n"
    ));
    out.push_str(
        "\t# Fields this codec does not carry are left as they were, which is what lets one\n",
    );
    out.push_str("\t# model be split across several codecs.\n");
    out.push_str(&format!(
        "\tfunc decode(reader: Reader, value: {model_name}) -> DecodeError:\n"
    ));
    if fields.is_empty() {
        out.push_str("\t\treturn null\n");
    } else {
        for field in &fields {
            decode_field(out, field, codec);
        }
        out.push_str("\t\treturn null\n");
    }
}

/// Emits one encode statement.
fn encode_field(out: &mut String, field: &Field, codec: &str) {
    let name = &field.name;

    match primitive(&field.network_type) {
        Some((writer_method, _)) => {
            out.push_str(&format!("writer.{writer_method}(value.{name})"));
        }
        // A model. The codec carries over, so a nested model is written by the
        // same profile the outer one is being written by.
        None => {
            let nested = codec_name(&field.network_type, codec);
            out.push_str(&format!("{nested}.new().encode(writer, value.{name})"));
        }
    }
}

/// Emits the statements that decode one field, ending in a newline.
///
/// A primitive reads through the `[value, error]` pair every `Reader` method
/// returns (see [`super::gdscript_runtime`]), checked immediately - the same
/// shape Go's `if err != nil { return err }` has, adapted to GDScript's lack
/// of a multi-value destructuring assignment. A nested model's field already
/// holds the instance it decodes into, the same way a Rust `&mut` or a Go
/// pointer does - no C#-style local-variable round trip needed.
fn decode_field(out: &mut String, field: &Field, codec: &str) {
    let name = &field.name;

    match primitive(&field.network_type) {
        Some((_, reader_method)) => {
            out.push_str(&format!("\t\tvar {name}_result := reader.{reader_method}()\n"));
            out.push_str(&format!("\t\tif {name}_result[1] != null:\n"));
            out.push_str(&format!("\t\t\treturn {name}_result[1]\n"));
            out.push_str(&format!("\t\tvalue.{name} = {name}_result[0]\n"));
        }
        None => {
            let nested = codec_name(&field.network_type, codec);
            out.push_str(&format!(
                "\t\tvar {name}_error = {nested}.new().decode(reader, value.{name})\n"
            ));
            out.push_str(&format!("\t\tif {name}_error != null:\n"));
            out.push_str(&format!("\t\t\treturn {name}_error\n"));
        }
    }
}

/// The generated type name: `DeviceState` + `edge` → `DeviceStateEdgeCodec`.
fn codec_name(model: &str, codec: &str) -> String {
    format!("{model}{}Codec", pascal_case(codec))
}

/// Looks a network type up in [`PRIMITIVES`].
fn primitive(network_type: &str) -> Option<(&'static str, &'static str)> {
    PRIMITIVES
        .iter()
        .find(|(name, ..)| *name == network_type)
        .map(|(_, writer, reader)| (*writer, *reader))
}
