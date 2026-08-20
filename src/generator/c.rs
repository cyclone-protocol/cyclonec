use std::collections::{BTreeMap, BTreeSet};

use crate::ir::{Field, Message, Model, Schema, WireType};
use crate::model::snake_case;
use crate::schema::hex64;

use super::codec_type_name;

pub const ARRAYS_FILE_NAME: &str = "arrays.h";

pub fn file_name(model: &str, codec: &str) -> String {
    format!("{}_{}.h", snake_case(model), snake_case(codec))
}

pub fn free_file_name(model: &str) -> String {
    format!("{}_cyclone.h", snake_case(model))
}

pub struct ModelLocation {
    pub include: String,
}

pub struct Imports<'a> {
    pub locations: &'a BTreeMap<String, ModelLocation>,
}

pub fn check_no_nested_arrays(model: &Model) -> Result<(), String> {
    for field in &model.fields {
        if let WireType::Array(element) = &field.ty {
            if matches!(element.as_ref(), WireType::Array(_)) {
                return Err(format!(
                    "model '{}' field '{}': the C backend does not support `Array<Array<T>>` - \
                     split '{}' into two codecs, or flatten the field",
                    model.name, field.name, field.name,
                ));
            }
        }
    }
    Ok(())
}

fn primitive(ty: &WireType) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match ty {
        WireType::Bool => (
            "cyclone_writer_write_bool",
            "cyclone_reader_read_bool",
            "bool",
        ),
        WireType::I8 => (
            "cyclone_writer_write_i8",
            "cyclone_reader_read_i8",
            "int8_t",
        ),
        WireType::U8 => (
            "cyclone_writer_write_u8",
            "cyclone_reader_read_u8",
            "uint8_t",
        ),
        WireType::I16 => (
            "cyclone_writer_write_i16",
            "cyclone_reader_read_i16",
            "int16_t",
        ),
        WireType::U16 => (
            "cyclone_writer_write_u16",
            "cyclone_reader_read_u16",
            "uint16_t",
        ),
        WireType::I32 => (
            "cyclone_writer_write_i32",
            "cyclone_reader_read_i32",
            "int32_t",
        ),
        WireType::U32 => (
            "cyclone_writer_write_u32",
            "cyclone_reader_read_u32",
            "uint32_t",
        ),
        WireType::I64 => (
            "cyclone_writer_write_i64",
            "cyclone_reader_read_i64",
            "int64_t",
        ),
        WireType::U64 => (
            "cyclone_writer_write_u64",
            "cyclone_reader_read_u64",
            "uint64_t",
        ),
        WireType::F32 => (
            "cyclone_writer_write_f32",
            "cyclone_reader_read_f32",
            "float",
        ),
        WireType::F64 => (
            "cyclone_writer_write_f64",
            "cyclone_reader_read_f64",
            "double",
        ),
        WireType::Str => (
            "cyclone_writer_write_string",
            "cyclone_reader_read_string",
            "const char *",
        ),
        WireType::Bytes => (
            "cyclone_writer_write_bytes",
            "cyclone_reader_read_bytes",
            "CycloneBytes",
        ),
        WireType::Array(_) | WireType::Model(_) => return None,
    })
}

fn zero(ty: &WireType) -> &'static str {
    match ty {
        WireType::Bool => "false",
        WireType::Str => "NULL",
        WireType::F32 => "0.0f",
        WireType::F64 => "0.0",
        WireType::Bytes => unreachable!("zeroed field-by-field in decode_field"),
        WireType::Array(_) => unreachable!("zeroed field-by-field in decode_field"),
        WireType::Model(_) => unreachable!("a model field is decoded through its own codec"),
        _ => "0",
    }
}

pub fn array_type_name(element: &WireType) -> String {
    match element {
        WireType::Model(name) => format!("CycloneArray_{name}"),
        other => format!("CycloneArray_{}", array_element_key(other)),
    }
}

fn array_element_key(ty: &WireType) -> &'static str {
    match ty {
        WireType::Bool => "bool",
        WireType::I8 => "i8",
        WireType::U8 => "u8",
        WireType::I16 => "i16",
        WireType::U16 => "u16",
        WireType::I32 => "i32",
        WireType::U32 => "u32",
        WireType::I64 => "i64",
        WireType::U64 => "u64",
        WireType::F32 => "f32",
        WireType::F64 => "f64",
        WireType::Str => "string",
        WireType::Bytes => "bytes",
        WireType::Array(_) | WireType::Model(_) => {
            unreachable!("nested arrays are refused; a model has its own arm")
        }
    }
}

fn element_c_type(ty: &WireType) -> String {
    match ty {
        WireType::Model(name) => struct_type(name),
        other => primitive(other)
            .map(|(_, _, c_type)| c_type.to_owned())
            .unwrap_or_default(),
    }
}

fn struct_type(model: &str) -> String {
    format!("struct {model}")
}

fn as_model(ty: &WireType) -> Option<&str> {
    match ty {
        WireType::Model(name) => Some(name),
        _ => None,
    }
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
    out.push_str("#pragma once\n\n");

    write_codec_includes(&mut out, model, message, imports);

    let name = codec_type_name(&model.name, &message.codec);
    let model_type = struct_type(&model.name);

    out.push_str(&format!(
        "// The {:?} codec for {}, generated from its Cyclone markers.\n",
        message.codec, model.name
    ));
    if message.fields.is_empty() {
        out.push_str("//\n// This codec carries no fields: it encodes to zero bytes.\n");
    } else {
        out.push_str("//\n// The wire layout, in declaration order (RFC-0002 SS5.1):\n//\n");
        for (index, field) in message.fields.iter().enumerate() {
            out.push_str(&format!(
                "//  {index}. `{}`: `{}`\n",
                field.name,
                field.ty.spelling()
            ));
        }
    }
    out.push('\n');

    out.push_str(&format!(
        "static const char *const {name}_MESSAGE_NAME = {:?};\n",
        message.name
    ));
    out.push_str(&format!(
        "static const uint32_t {name}_MESSAGE_ID = 0x{:08X}u;\n",
        message.id
    ));
    out.push_str(&format!(
        "static const uint64_t {name}_FINGERPRINT = {}ULL;\n\n",
        hex64(message.fingerprint.u64())
    ));

    out.push_str(&format!(
        "// Writes the {:?} fields of `*value`, in declaration order. Returns `false` \
         (buffer\n// untouched beyond what was already written) if growing the writer's \
         buffer failed.\n",
        message.codec
    ));
    out.push_str(&format!(
        "static inline bool {name}_encode(CycloneWriter *writer, const {model_type} *value) {{\n"
    ));
    if message.fields.is_empty() {
        out.push_str("    (void)writer;\n    (void)value;\n    return true;\n");
    } else {
        for field in &message.fields {
            encode_field(&mut out, field, &message.codec);
        }
        out.push_str("    return true;\n");
    }
    out.push_str("}\n\n");

    out.push_str(&format!(
        "// Reads the {:?} fields into `*value`, in declaration order.\n\
         //\n\
         // Fields this codec does not carry are left as they were, which is what lets one\n\
         // model be split across several codecs.\n\
         //\n\
         // A field the stream ended before takes its zero value (RFC-0002 SS9.1); a field\n\
         // the stream ended inside is an error. Bytes left over after the last field\n\
         // belong to a newer writer's model and are ignored.\n\
         //\n\
         // `*value` must not already hold data from a previous, unfreed decode - see\n\
         // runtime.h's module docs.\n",
        message.codec
    ));
    out.push_str(&format!(
        "static inline CycloneDecodeError {name}_decode(CycloneReader *reader, {model_type} *value) {{\n"
    ));
    if message.fields.is_empty() {
        out.push_str("    (void)reader;\n    (void)value;\n    return cyclone_decode_ok();\n");
    } else {
        for field in &message.fields {
            decode_field(&mut out, field, &message.codec);
        }
        out.push_str("    return cyclone_decode_ok();\n");
    }
    out.push_str("}\n");

    out
}

fn write_codec_includes(out: &mut String, model: &Model, message: &Message, imports: &Imports<'_>) {
    out.push_str(
        "#include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n\
         #include <stdlib.h>\n#include <string.h>\n\n",
    );
    out.push_str("#include \"runtime.h\"\n");

    if message
        .fields
        .iter()
        .any(|field| matches!(field.ty, WireType::Array(_)))
    {
        out.push_str(&format!("#include \"{ARRAYS_FILE_NAME}\"\n"));
    }

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

    let mut codecs: BTreeSet<String> = BTreeSet::new();
    let mut free_includes: BTreeSet<String> = BTreeSet::new();
    for field in &message.fields {
        let Some(name) = field.ty.model_name() else {
            continue;
        };
        if matches!(field.ty, WireType::Array(_)) {
            free_includes.insert(free_file_name(name));
        }
        if name == model.name {
            continue;
        }
        codecs.insert(file_name(name, &message.codec));
    }
    for include in codecs {
        out.push_str(&format!("#include \"{include}\"\n"));
    }
    for include in free_includes {
        out.push_str(&format!("#include \"{include}\"\n"));
    }

    out.push('\n');
}

fn encode_field(out: &mut String, field: &Field, codec: &str) {
    let place = format!("value->{}", field.name);

    if let WireType::Array(element_type) = &field.ty {
        out.push_str(&format!(
            "    if (!cyclone_writer_write_array_count(writer, {place}.count)) return false;\n"
        ));
        out.push_str(&format!(
            "    for (size_t i = 0; i < {place}.count; ++i) {{\n"
        ));
        let element = format!("{place}.items[i]");
        encode_scalar(out, element_type, &element, codec, "        ");
        out.push_str("    }\n");
        return;
    }

    encode_scalar(out, &field.ty, &place, codec, "    ");
}

fn encode_scalar(out: &mut String, ty: &WireType, place: &str, codec: &str, pad: &str) {
    match primitive(ty) {
        Some((writer_fn, ..)) => {
            let arg = if matches!(ty, WireType::Bytes) {
                format!("&{place}")
            } else {
                place.to_owned()
            };
            out.push_str(&format!(
                "{pad}if (!{writer_fn}(writer, {arg})) return false;\n"
            ));
        }
        None => {
            let nested = codec_type_name(
                ty.model_name().expect("a non-primitive, non-array type"),
                codec,
            );
            out.push_str(&format!(
                "{pad}if (!{nested}_encode(writer, &{place})) return false;\n"
            ));
        }
    }
}

fn decode_field(out: &mut String, field: &Field, codec: &str) {
    let place = format!("value->{}", field.name);

    if let Some(name) = as_model(&field.ty) {
        let nested = codec_type_name(name, codec);
        out.push_str(&format!(
            "    {{\n        CycloneDecodeError error = {nested}_decode(reader, &{place});\n        \
             if (!cyclone_decode_error_ok(&error)) return error;\n    }}\n"
        ));
        return;
    }

    if let WireType::Array(element_type) = &field.ty {
        decode_array_field(out, &place, element_type, codec);
        return;
    }

    if matches!(field.ty, WireType::Bytes) {
        out.push_str("    if (cyclone_reader_field_absent(reader)) {\n");
        out.push_str(&format!(
            "        {place}.data = NULL;\n        {place}.len = 0;\n"
        ));
        out.push_str("    } else {\n");
        out.push_str(&format!(
            "        CycloneDecodeError error = cyclone_reader_read_bytes(reader, &{place});\n        \
             if (!cyclone_decode_error_ok(&error)) return error;\n"
        ));
        out.push_str("    }\n");
        return;
    }

    let (_, reader_fn, _) = primitive(&field.ty).expect("models, arrays and bytes handled above");
    out.push_str("    if (cyclone_reader_field_absent(reader)) {\n");
    out.push_str(&format!("        {place} = {};\n", zero(&field.ty)));
    out.push_str("    } else {\n");
    out.push_str(&format!(
        "        CycloneDecodeError error = {reader_fn}(reader, &{place});\n        \
         if (!cyclone_decode_error_ok(&error)) return error;\n"
    ));
    out.push_str("    }\n");
}

fn decode_array_field(out: &mut String, place: &str, element_type: &WireType, codec: &str) {
    let array_type = array_type_name(element_type);
    let elem_c_type = element_c_type(element_type);

    out.push_str("    {\n");
    out.push_str("        size_t count = 0;\n");
    out.push_str("        if (!cyclone_reader_field_absent(reader)) {\n");
    out.push_str(
        "            CycloneDecodeError error = cyclone_reader_read_array_count(reader, \
         &count);\n            if (!cyclone_decode_error_ok(&error)) return error;\n",
    );
    out.push_str("        }\n");
    out.push_str(&format!("        {array_type} array;\n"));
    out.push_str("        array.items = NULL;\n");
    out.push_str("        array.count = 0;\n");
    out.push_str("        if (count > 0) {\n");
    out.push_str(&format!(
        "            array.items = ({elem_c_type} *)calloc(count, sizeof({elem_c_type}));\n"
    ));
    out.push_str("            if (array.items == NULL) {\n");
    out.push_str(
        "                CycloneDecodeError error = cyclone_decode_ok();\n                \
         error.kind = CYCLONE_DECODE_OUT_OF_MEMORY;\n                return error;\n",
    );
    out.push_str("            }\n");
    out.push_str("            array.count = count;\n");
    out.push_str("        }\n");
    out.push_str("        for (size_t i = 0; i < count; ++i) {\n");
    decode_element_into(out, element_type, "array.items[i]", codec);
    out.push_str("        }\n");
    out.push_str(&format!("        {place} = array;\n"));
    out.push_str("    }\n");
}

fn decode_element_into(out: &mut String, ty: &WireType, element_place: &str, codec: &str) {
    match as_model(ty) {
        Some(name) => {
            let nested = codec_type_name(name, codec);
            out.push_str(&format!(
                "            CycloneDecodeError error = {nested}_decode(reader, &{element_place});\n"
            ));
        }
        None => {
            let (_, reader_fn, _) = primitive(ty).expect("models handled above");
            out.push_str(&format!(
                "            CycloneDecodeError error = {reader_fn}(reader, &{element_place});\n"
            ));
        }
    }
    out.push_str("            if (!cyclone_decode_error_ok(&error)) {\n");
    free_array_value(out, ty, "array", "                ");
    out.push_str("                return error;\n");
    out.push_str("            }\n");
}

fn free_array_value(out: &mut String, element: &WireType, target: &str, pad: &str) {
    match as_model(element) {
        Some(name) => {
            out.push_str(&format!(
                "{pad}for (size_t j = 0; j < {target}.count; ++j) {{\n"
            ));
            out.push_str(&format!("{pad}    {name}_free(&{target}.items[j]);\n"));
            out.push_str(&format!("{pad}}}\n"));
            out.push_str(&format!("{pad}free({target}.items);\n"));
            out.push_str(&format!("{pad}{target}.items = NULL;\n"));
            out.push_str(&format!("{pad}{target}.count = 0;\n"));
        }
        None => {
            let array_type = array_type_name(element);
            out.push_str(&format!("{pad}{array_type}_free(&{target});\n"));
        }
    }
}

pub fn free_file(model: &Model, imports: &Imports<'_>) -> String {
    let mut out = super::Header {
        source: Some(&model.source),
        model: Some(&model.name),
        codec: None,
        fingerprint: Some(model.fingerprint.tagged()),
        note: None,
    }
    .render();
    out.push_str("#pragma once\n\n");

    write_free_includes(&mut out, model, imports);

    let model_type = struct_type(&model.name);
    out.push_str(&format!(
        "// Releases every heap allocation a decoded {0} owns: every `string`, `bytes`\n\
         // and `Array<T>` field, and every nested model field, recursively - whichever\n\
         // of {1}'s codecs actually populated them.\n\
         //\n\
         // Safe on a freshly zero-initialized {0} ({0} value = {{0}};), on one this\n\
         // model's decode functions have populated, or on one already freed - never on\n\
         // one still holding data from a *previous, unfreed* decode (see runtime.h's\n\
         // module docs).\n",
        model_type, model.name
    ));
    out.push_str(&format!(
        "static inline void {}_free({} *value) {{\n",
        model.name, model_type
    ));

    let owning_fields: Vec<&Field> = model
        .fields
        .iter()
        .filter(|field| !field.codecs.is_empty())
        .collect();
    if owning_fields.iter().any(|field| owns_memory(&field.ty)) {
        for field in &owning_fields {
            free_field_stmt(&mut out, field);
        }
    } else {
        out.push_str("    (void)value;\n");
    }
    out.push_str("}\n");

    out
}

fn owns_memory(ty: &WireType) -> bool {
    matches!(
        ty,
        WireType::Str | WireType::Bytes | WireType::Array(_) | WireType::Model(_)
    )
}

fn free_field_stmt(out: &mut String, field: &Field) {
    let place = format!("value->{}", field.name);
    match &field.ty {
        WireType::Str => {
            out.push_str(&format!(
                "    free((void *){place});\n    {place} = NULL;\n"
            ));
        }
        WireType::Bytes => {
            out.push_str(&format!("    cyclone_bytes_free(&{place});\n"));
        }
        WireType::Array(element) => {
            free_array_value(out, element, &place, "    ");
        }
        WireType::Model(name) => {
            out.push_str(&format!("    {name}_free(&{place});\n"));
        }
        _ => {}
    }
}

fn write_free_includes(out: &mut String, model: &Model, imports: &Imports<'_>) {
    out.push_str("#include <stddef.h>\n#include <stdlib.h>\n\n");
    out.push_str("#include \"runtime.h\"\n");

    let owning_fields: Vec<&Field> = model
        .fields
        .iter()
        .filter(|field| !field.codecs.is_empty())
        .collect();

    if owning_fields
        .iter()
        .any(|field| matches!(field.ty, WireType::Array(_)))
    {
        out.push_str(&format!("#include \"{ARRAYS_FILE_NAME}\"\n"));
    }

    let mut nested_models: BTreeSet<&str> = BTreeSet::new();
    for field in &owning_fields {
        match &field.ty {
            WireType::Model(name) => {
                nested_models.insert(name.as_str());
            }
            WireType::Array(element) => {
                if let Some(name) = element.model_name() {
                    nested_models.insert(name);
                }
            }
            _ => {}
        }
    }

    let mut struct_includes: BTreeSet<&str> = BTreeSet::new();
    if let Some(location) = imports.locations.get(model.name.as_str()) {
        struct_includes.insert(location.include.as_str());
    }
    for name in &nested_models {
        if let Some(location) = imports.locations.get(*name) {
            struct_includes.insert(location.include.as_str());
        }
    }
    for include in struct_includes {
        out.push_str(&format!("#include \"{include}\"\n"));
    }

    for name in nested_models {
        out.push_str(&format!("#include \"{}\"\n", free_file_name(name)));
    }

    out.push('\n');
}

fn distinct_array_elements(schema: &Schema) -> BTreeMap<String, WireType> {
    let mut elements = BTreeMap::new();
    for model in &schema.models {
        for field in model.fields.iter().filter(|field| !field.codecs.is_empty()) {
            if let WireType::Array(element) = &field.ty {
                elements
                    .entry(array_type_name(element))
                    .or_insert_with(|| (**element).clone());
            }
        }
    }
    elements
}

pub fn arrays_file(schema: &Schema, _imports: &Imports<'_>) -> String {
    let mut out = super::Header {
        note: Some(
            "Owned array types for every distinct Array<T> element type this schema's\n\
             fields use - plain C has no generic growable container of its own, so this\n\
             is the C counterpart of a std::vector<T> for exactly the T's this project\n\
             needs. #include this wherever a model declares an Array<T> field.\n\
             \n\
             A model element type's `items` is declared as a pointer only (`struct X\n\
             *items`), which needs no more than `struct X`'s tag to exist - so this file\n\
             never has to #include any model's own header, and can never become part of\n\
             a #include cycle with one. Freeing an Array<T> of a model is generated\n\
             inline at each call site instead of as a function here, for exactly that\n\
             reason - see generator::c::free_array_value's doc comment.",
        ),
        ..super::Header::default()
    }
    .render();
    out.push_str("#pragma once\n\n");
    out.push_str("#include <stddef.h>\n#include <stdlib.h>\n\n");
    out.push_str("#include \"runtime.h\"\n\n");

    let elements = distinct_array_elements(schema);

    if elements.is_empty() {
        out.push_str("// No model in this schema declares an Array<T> field.\n");
        return out;
    }

    for (name, ty) in &elements {
        let elem_c_type = element_c_type(ty);
        out.push_str(&format!(
            "typedef struct {name} {{\n    {elem_c_type} *items;\n    size_t count;\n}} {name};\n\n"
        ));

        if as_model(ty).is_some() {
            continue;
        }

        out.push_str(&format!(
            "static inline void {name}_free({name} *array) {{\n"
        ));
        match ty {
            WireType::Str => {
                out.push_str(
                    "    for (size_t i = 0; i < array->count; ++i) {\n        \
                     free((void *)array->items[i]);\n    }\n",
                );
            }
            WireType::Bytes => {
                out.push_str(
                    "    for (size_t i = 0; i < array->count; ++i) {\n        \
                     cyclone_bytes_free(&array->items[i]);\n    }\n",
                );
            }
            _ => {}
        }
        out.push_str(
            "    free(array->items);\n    array->items = NULL;\n    array->count = 0;\n}\n\n",
        );
    }

    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::{
        arrays_file, check_no_nested_arrays, codec_file, file_name, free_file, free_file_name,
        Imports, ModelLocation,
    };
    use crate::ir::Schema;
    use crate::model::{Field, Model};

    fn schema_with_player_info() -> (Schema, BTreeMap<String, ModelLocation>) {
        let schema = Schema::build(&[
            Model {
                name: "Player".to_owned(),
                source: PathBuf::from("models/player.h"),
                line: 1,
                codecs: vec!["edge".to_owned()],
                fields: vec![],
            },
            Model {
                name: "PlayerInfo".to_owned(),
                source: PathBuf::from("models/player.h"),
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
                        include: "models/player.h".to_owned(),
                    },
                )
            })
            .collect();
        (schema, locations)
    }

    fn generated(fields: &[(&str, &str)]) -> String {
        let (mut schema, locations) = schema_with_player_info();
        schema.models[0].fields = fields
            .iter()
            .map(|(name, ty)| crate::ir::Field {
                name: (*name).to_owned(),
                ty: crate::ir::WireType::parse(ty).expect("parse"),
                codecs: vec!["edge".to_owned()],
            })
            .collect();
        schema.models[0].messages[0].fields = schema.models[0].fields.clone();

        let model = schema.model("Player").expect("model").clone();
        codec_file(
            &model,
            &model.messages[0],
            &Imports {
                locations: &locations,
            },
        )
    }

    #[test]
    fn a_primitive_reads_and_writes_through_the_pointer() {
        let text = generated(&[("Id", "u32")]);
        assert!(
            text.contains("if (!cyclone_writer_write_u32(writer, value->Id)) return false;"),
            "{text}"
        );
        assert!(
            text.contains("if (cyclone_reader_field_absent(reader)) {\n        value->Id = 0;"),
            "{text}"
        );
        assert!(
            text.contains("cyclone_reader_read_u32(reader, &value->Id);"),
            "{text}"
        );
    }

    #[test]
    fn no_dto_no_mapper_no_intermediate_anything() {
        let text = generated(&[("Id", "u32"), ("Name", "string")]);
        assert!(
            text.contains(
                "static inline bool PlayerEdgeCodec_encode(CycloneWriter *writer, const struct \
                 Player *value)"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "static inline CycloneDecodeError PlayerEdgeCodec_decode(CycloneReader *reader, \
                 struct Player *value)"
            ),
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
    fn a_string_field_is_a_const_char_star_zeroing_to_null() {
        let text = generated(&[("Name", "string")]);
        assert!(
            text.contains("cyclone_writer_write_string(writer, value->Name)"),
            "{text}"
        );
        assert!(text.contains("value->Name = NULL;"), "{text}");
        assert!(
            text.contains("cyclone_reader_read_string(reader, &value->Name);"),
            "{text}"
        );
    }

    #[test]
    fn a_bytes_field_is_passed_by_address() {
        let text = generated(&[("Payload", "bytes")]);
        assert!(
            text.contains("cyclone_writer_write_bytes(writer, &value->Payload)"),
            "{text}"
        );
        assert!(
            text.contains("value->Payload.data = NULL;\n        value->Payload.len = 0;"),
            "{text}"
        );
    }

    #[test]
    fn a_nested_model_calls_the_same_codec_by_address_directly() {
        let text = generated(&[("Info", "PlayerInfo")]);
        assert!(
            text.contains("PlayerInfoEdgeCodec_encode(writer, &value->Info)"),
            "{text}"
        );
        assert!(
            text.contains(
                "CycloneDecodeError error = PlayerInfoEdgeCodec_decode(reader, &value->Info);"
            ),
            "{text}"
        );
    }

    #[test]
    fn an_array_counts_first_then_loops_strictly() {
        let text = generated(&[("Tags", "Array<string>")]);
        assert!(
            text.contains(
                "if (!cyclone_writer_write_array_count(writer, value->Tags.count)) return false;"
            ),
            "{text}"
        );
        assert!(
            text.contains("for (size_t i = 0; i < value->Tags.count; ++i) {"),
            "{text}"
        );
        assert!(
            text.contains("cyclone_writer_write_string(writer, value->Tags.items[i])"),
            "{text}"
        );
        assert!(text.contains("CycloneArray_string array;"), "{text}");
        assert!(
            text.contains("array.items = (const char * *)calloc(count, sizeof(const char *));"),
            "{text}"
        );
        assert!(text.contains("value->Tags = array;"), "{text}");
    }

    #[test]
    fn an_array_of_models_frees_inline_on_a_failing_element_not_via_a_centralized_function() {
        let text = generated(&[("Roster", "Array<PlayerInfo>")]);
        assert!(
            text.contains(
                "array.items = (struct PlayerInfo *)calloc(count, sizeof(struct PlayerInfo));"
            ),
            "{text}"
        );
        assert!(
            text.contains("PlayerInfoEdgeCodec_decode(reader, &array.items[i]);"),
            "{text}"
        );
        assert!(!text.contains("CycloneArray_PlayerInfo_free"), "{text}");
        assert!(text.contains("PlayerInfo_free(&array.items[j]);"), "{text}");
    }

    #[test]
    fn nested_arrays_are_refused_rather_than_generated_wrong() {
        let model = Model {
            name: "Grid".to_owned(),
            source: PathBuf::from("models/grid.h"),
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
        assert_eq!(file_name("Player", "edge"), "player_edge.h");
        assert_eq!(
            file_name("PlayerInfo", "orange_pi"),
            "player_info_orange_pi.h"
        );
        assert_eq!(free_file_name("Player"), "player_cyclone.h");
    }

    #[test]
    fn a_codec_with_no_fields_still_compiles() {
        let text = generated(&[]);
        assert!(
            text.contains(
                "static inline bool PlayerEdgeCodec_encode(CycloneWriter *writer, const struct \
                 Player *value) {\n    (void)writer;\n    (void)value;\n    return true;\n}"
            ),
            "{text}"
        );
        assert!(
            text.contains("(void)reader;\n    (void)value;\n    return cyclone_decode_ok();"),
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
        assert!(text.contains("// source: models/player.h\n"), "{text}");
        assert!(text.contains("// model: Player\n"), "{text}");
        assert!(text.contains("// codec: edge\n"), "{text}");
        assert!(text.contains("// fingerprint: sha256:"), "{text}");
        assert!(text.contains("#pragma once\n"), "{text}");
    }

    #[test]
    fn the_constants_are_generated_not_hand_written() {
        let text = generated(&[("Id", "u32")]);
        assert!(
            text.contains(
                "static const char *const PlayerEdgeCodec_MESSAGE_NAME = \"Player.edge\";"
            ),
            "{text}"
        );
        assert!(
            text.contains("static const uint32_t PlayerEdgeCodec_MESSAGE_ID = 0x"),
            "{text}"
        );
        assert!(
            text.contains("static const uint64_t PlayerEdgeCodec_FINGERPRINT = 0x"),
            "{text}"
        );
        assert!(text.contains("ULL;"), "{text}");
    }

    #[test]
    fn a_bare_nested_model_includes_its_own_codec_header() {
        let text = generated(&[("Info", "PlayerInfo")]);
        assert!(text.contains("#include \"player_info_edge.h\"\n"), "{text}");
    }

    #[test]
    fn a_model_field_is_always_spelled_struct_name_never_bare() {
        let text = generated(&[("Info", "PlayerInfo")]);
        assert!(text.contains("const struct Player *value"), "{text}");
        assert!(text.contains("struct Player *value"), "{text}");
    }

    #[test]
    fn a_free_file_releases_every_owning_field_and_nothing_else() {
        let (schema, locations) = schema_with_player_info();
        let model = schema.model("PlayerInfo").expect("model");
        let text = free_file(
            model,
            &Imports {
                locations: &locations,
            },
        );
        assert!(
            text.contains("static inline void PlayerInfo_free(struct PlayerInfo *value) {"),
            "{text}"
        );
        assert!(text.contains("(void)value;"), "{text}");
    }

    #[test]
    fn a_free_file_frees_a_string_with_a_const_cast_and_a_bytes_field_via_the_runtime_helper() {
        let schema = Schema::build(&[Model {
            name: "Player".to_owned(),
            source: PathBuf::from("models/player.h"),
            line: 1,
            codecs: vec!["edge".to_owned()],
            fields: vec![
                Field {
                    name: "Name".to_owned(),
                    network_type: "string".to_owned(),
                    codecs: vec!["edge".to_owned()],
                    line: 2,
                },
                Field {
                    name: "Payload".to_owned(),
                    network_type: "bytes".to_owned(),
                    codecs: vec!["edge".to_owned()],
                    line: 3,
                },
            ],
        }])
        .expect("build");
        let locations: BTreeMap<String, ModelLocation> = [(
            "Player".to_owned(),
            ModelLocation {
                include: "models/player.h".to_owned(),
            },
        )]
        .into_iter()
        .collect();
        let model = schema.model("Player").expect("model");
        let text = free_file(
            model,
            &Imports {
                locations: &locations,
            },
        );
        assert!(
            text.contains("free((void *)value->Name);\n    value->Name = NULL;"),
            "{text}"
        );
        assert!(
            text.contains("cyclone_bytes_free(&value->Payload);"),
            "{text}"
        );
    }

    #[test]
    fn arrays_file_generates_one_wrapper_and_free_per_distinct_element_type() {
        let (schema, locations) = schema_with_player_info();
        let mut schema = schema;
        schema.models[0].fields = vec![
            crate::ir::Field {
                name: "Scores".to_owned(),
                ty: crate::ir::WireType::parse("Array<u32>").expect("parse"),
                codecs: vec!["edge".to_owned()],
            },
            crate::ir::Field {
                name: "Tags".to_owned(),
                ty: crate::ir::WireType::parse("Array<string>").expect("parse"),
                codecs: vec!["edge".to_owned()],
            },
        ];
        let text = arrays_file(
            &schema,
            &Imports {
                locations: &locations,
            },
        );
        assert!(text.contains("typedef struct CycloneArray_u32"), "{text}");
        assert!(
            text.contains("static inline void CycloneArray_u32_free"),
            "{text}"
        );
        assert!(
            text.contains("typedef struct CycloneArray_string"),
            "{text}"
        );
        assert!(
            text.contains("static inline void CycloneArray_string_free"),
            "{text}"
        );
    }

    #[test]
    fn an_empty_schema_still_produces_a_valid_arrays_file() {
        let schema = Schema::build(&[]).expect("build");
        let locations = BTreeMap::new();
        let text = arrays_file(
            &schema,
            &Imports {
                locations: &locations,
            },
        );
        assert!(
            text.contains("No model in this schema declares an Array<T> field."),
            "{text}"
        );
    }
}
