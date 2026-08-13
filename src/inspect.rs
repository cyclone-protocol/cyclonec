//! `cyclone-inspect` - a packet, read through a schema.
//!
//! ```bash
//! cyclone-inspect --schema .cyclone/schema.json --message Player --file packet.bin
//! ```
//!
//! The schema is named, never guessed. A tool that inferred which message a
//! buffer holds would be inventing the one thing Cyclone's wire format
//! deliberately does not carry - there is no tag, no id, no length in front of
//! a payload to infer from, and a plausible-looking wrong answer is worse than
//! no answer.
//!
//! It decodes by exactly the rules a generated decoder follows, including
//! RFC-0002 §9.1: a field the stream ended before is reported as absent, a
//! field the stream ended inside is an error, and bytes past the last field are
//! reported as a newer writer's and left alone.

use std::path::PathBuf;

use crate::fingerprint::Fingerprint;
use crate::ir::{Message, Schema, WireType};

/// Where the bytes come from.
#[derive(Debug, Clone)]
pub enum Input {
    File(PathBuf),
    /// Hex digits, with any spacing: `64 00 00 00`, `0x64,0x00`, `64000000`.
    Hex(String),
}

/// What to inspect.
#[derive(Debug, Clone)]
pub struct Options {
    pub schema: PathBuf,
    /// `Player`, or `Player.edge`.
    pub message: String,
    /// Needed only when the model declares more than one codec and the message
    /// was named without one.
    pub codec: Option<String>,
    pub input: Input,
    /// A fingerprint the message is expected to have: `sha256:…` or `0x…`.
    pub expect: Option<String>,
}

/// The usage text.
pub const USAGE: &str = "\
cyclone-inspect - decode a Cyclone packet through a schema

USAGE:
    cyclone-inspect --schema <SCHEMA> --message <NAME> (--file <PATH> | --hex <HEX>)

OPTIONS:
        --schema <PATH>   .cyclone/schema.json. Required - never guessed.
        --message <NAME>  `Player`, or `Player.edge` to name the codec too.
        --codec <NAME>    The codec, if --message did not name one.
        --file <PATH>     A binary file holding one message's payload.
        --hex <HEX>       The payload as hex digits, spacing ignored.
        --expect <FP>     Fail unless the message's fingerprint is this, as
                          `sha256:…` or `0x…`. Checks the packet was read
                          through the schema it was written by.
    -h, --help            Print this message

EXAMPLES:
    cyclone-inspect --schema .cyclone/schema.json --message Player --file packet.bin
    cyclone-inspect --schema .cyclone/schema.json --message Player.edge --hex '64000000 0000 2841'
";

/// Parses `cyclone-inspect`'s command line.
///
/// # Errors
///
/// A missing or unknown option, or no input.
pub fn parse(
    argv: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Option<Options>, String> {
    let mut schema = None;
    let mut message = None;
    let mut codec = None;
    let mut file = None;
    let mut hex = None;
    let mut expect = None;

    let arguments: Vec<String> = argv
        .into_iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .ok_or_else(|| format!("{argument} needs a value"))
        };
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "--schema" => schema = Some(PathBuf::from(value()?)),
            "--message" => message = Some(value()?),
            "--codec" => codec = Some(value()?),
            "--file" => file = Some(PathBuf::from(value()?)),
            "--hex" => hex = Some(value()?),
            "--expect" => expect = Some(value()?),
            other => return Err(format!("unknown option `{other}`")),
        }
    }

    let input = match (file, hex) {
        (Some(path), None) => Input::File(path),
        (None, Some(text)) => Input::Hex(text),
        (Some(_), Some(_)) => {
            return Err("--file and --hex are two ways to say one thing".to_owned())
        }
        (None, None) => return Err("no input: pass --file <PATH> or --hex <HEX>".to_owned()),
    };

    Ok(Some(Options {
        schema: schema
            .ok_or("--schema is required: say which schema to read the packet through")?,
        message: message.ok_or("--message is required: say which message the packet holds")?,
        codec,
        input,
        expect,
    }))
}

/// Decodes the packet and renders the report.
///
/// # Errors
///
/// A schema that cannot be read, a message that is not in it, a packet that
/// cannot be read, or a byte stream that does not satisfy the Specification.
pub fn run(options: &Options) -> Result<String, String> {
    let text = std::fs::read_to_string(&options.schema)
        .map_err(|error| format!("{}: {error}", options.schema.display()))?;
    let schema = crate::schema::from_json(&text)
        .map_err(|problem| format!("{}: {problem}", options.schema.display()))?;

    let message = find_message(&schema, &options.message, options.codec.as_deref())?;

    if let Some(expected) = &options.expect {
        check_fingerprint(message, expected)?;
    }

    let bytes = match &options.input {
        Input::File(path) => {
            std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?
        }
        Input::Hex(text) => from_hex(text)?,
    };

    render(&schema, message, &bytes)
}

/// Finds `Player`, `Player.edge`, or `Player` + `--codec edge`.
fn find_message<'a>(
    schema: &'a Schema,
    name: &str,
    codec: Option<&str>,
) -> Result<&'a Message, String> {
    let (model_name, codec) = match (name.split_once('.'), codec) {
        (Some((model, codec)), None) => (model, Some(codec.to_owned())),
        (Some((model, codec)), Some(flag)) if codec == flag => (model, Some(flag.to_owned())),
        (Some(_), Some(_)) => {
            return Err(format!(
                "--message {name} and --codec name two different codecs"
            ))
        }
        (None, codec) => (name, codec.map(str::to_owned)),
    };

    let model = schema.model(model_name).ok_or_else(|| {
        let known: Vec<&str> = schema.models.iter().map(|model| &*model.name).collect();
        format!(
            "no model '{model_name}' in this schema. It has: {}",
            known.join(", ")
        )
    })?;

    match codec {
        Some(codec) => model
            .messages
            .iter()
            .find(|message| message.codec == codec)
            .ok_or_else(|| {
                format!(
                    "model '{model_name}' has no '{codec}' codec. It declares: {}",
                    model.codecs.join(", ")
                )
            }),
        None => match model.messages.len() {
            1 => Ok(&model.messages[0]),
            0 => Err(format!(
                "model '{model_name}' declares no codec, so it has no wire format"
            )),
            _ => Err(format!(
                "model '{model_name}' declares {} codecs ({}); name one with --codec",
                model.messages.len(),
                model.codecs.join(", ")
            )),
        },
    }
}

fn check_fingerprint(message: &Message, expected: &str) -> Result<(), String> {
    let matches = if let Some(hex) = expected.strip_prefix("0x").or(expected.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
            .map_err(|_| format!("`{expected}` is not a 64-bit hex fingerprint"))?
            == message.fingerprint.u64()
    } else {
        Fingerprint::parse(expected)? == message.fingerprint
    };

    if matches {
        return Ok(());
    }
    Err(format!(
        "fingerprint mismatch: {} is {} (0x{:016X}), not {expected}",
        message.name,
        message.fingerprint.tagged(),
        message.fingerprint.u64(),
    ))
}

/// Hex digits with any spacing, `0x` prefixes and commas.
fn from_hex(text: &str) -> Result<Vec<u8>, String> {
    let digits: String = text
        .replace("0x", " ")
        .replace("0X", " ")
        .chars()
        .filter(|character| !character.is_whitespace() && *character != ',' && *character != '_')
        .collect();

    if !digits.len().is_multiple_of(2) {
        return Err(format!(
            "hex input has {} digits; a byte takes two",
            digits.len()
        ));
    }
    (0..digits.len() / 2)
        .map(|index| {
            u8::from_str_radix(&digits[index * 2..index * 2 + 2], 16)
                .map_err(|_| format!("`{}` is not a hex byte", &digits[index * 2..index * 2 + 2]))
        })
        .collect()
}

// ================================================================= the decoder

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn take(&mut self, count: usize, what: &str) -> Result<&'a [u8], String> {
        if count > self.remaining() {
            return Err(format!(
                "offset {}: {what} needs {count} bytes, {} remain - the packet is truncated",
                self.at,
                self.remaining()
            ));
        }
        let bytes = &self.bytes[self.at..self.at + count];
        self.at += count;
        Ok(bytes)
    }
}

/// Decodes `bytes` as `message` and renders the report.
fn render(schema: &Schema, message: &Message, bytes: &[u8]) -> Result<String, String> {
    let mut out = String::with_capacity(1024);
    out.push_str(&format!("{}\n", message.name));
    out.push_str(&format!(
        "fingerprint: {} (0x{:016X})\n",
        message.fingerprint.tagged(),
        message.fingerprint.u64()
    ));
    out.push_str(&format!("message id : 0x{:08X}\n", message.id));
    out.push_str(&format!("payload    : {} bytes\n", bytes.len()));
    out.push_str(&"-".repeat(52));
    out.push('\n');

    let mut cursor = Cursor { bytes, at: 0 };
    let width = message
        .fields
        .iter()
        .map(|field| field.name.len())
        .max()
        .unwrap_or(4)
        .max(7);

    fields(&mut out, schema, message, &mut cursor, width, 0)?;

    if cursor.remaining() > 0 {
        blank_line(&mut out);
        out.push_str(&format!(
            "{} trailing byte(s) at offset {}: fields of a newer model this schema does not\n\
             know. RFC-0002 §9.1 - not an error, and a decoder ignores them.\n",
            cursor.remaining(),
            cursor.at
        ));
        out.push_str(&format!("  bytes: {}\n", hex(&bytes[cursor.at..])));
    }

    Ok(out)
}

/// Every field of one message, at one nesting depth.
fn fields(
    out: &mut String,
    schema: &Schema,
    message: &Message,
    cursor: &mut Cursor<'_>,
    width: usize,
    depth: usize,
) -> Result<(), String> {
    let pad = "  ".repeat(depth);

    for field in &message.fields {
        // RFC-0002 §9.1: nothing left at a field boundary means the writer's
        // model stopped here, and this field never arrived.
        if cursor.remaining() == 0 {
            out.push_str(&format!(
                "{pad}{:width$} : {} = absent (the stream ended before this field)\n",
                field.name,
                field.ty.spelling()
            ));
            continue;
        }

        let offset = cursor.at;
        match &field.ty {
            WireType::Model(name) => {
                out.push_str(&format!(
                    "{pad}{:width$} : {} =\n",
                    field.name,
                    field.ty.spelling()
                ));
                let nested = nested_message(schema, name, &message.codec)?;
                fields(out, schema, nested, cursor, width, depth + 1)?;
            }
            WireType::Array(element) => {
                let count = read_u32(cursor, "an array count")? as usize;
                out.push_str(&format!(
                    "{pad}{:width$} : {} = {count} element(s)\n",
                    field.name,
                    field.ty.spelling()
                ));
                out.push_str(&format!("{pad}{:width$}   offset: {offset}\n", ""));
                for index in 0..count {
                    let element_offset = cursor.at;
                    match element.as_ref() {
                        WireType::Model(name) => {
                            out.push_str(&format!("{pad}  [{index}] =\n"));
                            let nested = nested_message(schema, name, &message.codec)?;
                            fields(out, schema, nested, cursor, width, depth + 2)?;
                        }
                        element => {
                            let value = value(cursor, element)?;
                            out.push_str(&format!(
                                "{pad}  [{index}] = {value}   offset: {element_offset}, bytes: {}\n",
                                hex(&cursor.bytes[element_offset..cursor.at])
                            ));
                        }
                    }
                }
            }
            primitive => {
                let value = value(cursor, primitive)?;
                out.push_str(&format!(
                    "{pad}{:width$} : {} = {value}\n",
                    field.name,
                    field.ty.spelling()
                ));
                out.push_str(&format!("{pad}{:width$}   offset: {offset}\n", ""));
                out.push_str(&format!(
                    "{pad}{:width$}   bytes: {}\n",
                    "",
                    hex(&cursor.bytes[offset..cursor.at])
                ));
            }
        }
        blank_line(out);
    }

    Ok(())
}

/// One blank line between fields, and never two - a nested model's own last
/// field already left one behind it.
fn blank_line(out: &mut String) {
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
}

fn nested_message<'a>(schema: &'a Schema, model: &str, codec: &str) -> Result<&'a Message, String> {
    schema
        .model(model)
        .and_then(|model| model.messages.iter().find(|message| message.codec == codec))
        .ok_or_else(|| {
            format!(
                "this packet contains a '{model}' with the '{codec}' codec, which this schema \
                 does not describe - it cannot be decoded any further"
            )
        })
}

fn read_u32(cursor: &mut Cursor<'_>, what: &str) -> Result<u32, String> {
    let bytes = cursor.take(4, what)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// One primitive, decoded and formatted.
fn value(cursor: &mut Cursor<'_>, ty: &WireType) -> Result<String, String> {
    Ok(match ty {
        WireType::Bool => match cursor.take(1, "a bool")?[0] {
            0x00 => "false".to_owned(),
            0x01 => "true".to_owned(),
            other => {
                return Err(format!(
                "offset {}: invalid bool 0x{other:02X} - RFC-0002 §2.4 allows only 0x00 and 0x01",
                cursor.at - 1
            ))
            }
        },
        WireType::I8 => (cursor.take(1, "an i8")?[0] as i8).to_string(),
        WireType::U8 => cursor.take(1, "a u8")?[0].to_string(),
        WireType::I16 => {
            let bytes = cursor.take(2, "an i16")?;
            i16::from_le_bytes([bytes[0], bytes[1]]).to_string()
        }
        WireType::U16 => {
            let bytes = cursor.take(2, "a u16")?;
            u16::from_le_bytes([bytes[0], bytes[1]]).to_string()
        }
        WireType::I32 => (read_u32(cursor, "an i32")? as i32).to_string(),
        WireType::U32 => read_u32(cursor, "a u32")?.to_string(),
        WireType::I64 => (read_u64(cursor, "an i64")? as i64).to_string(),
        WireType::U64 => read_u64(cursor, "a u64")?.to_string(),
        WireType::F32 => format_float(f32::from_bits(read_u32(cursor, "an f32")?) as f64),
        WireType::F64 => format_float(f64::from_bits(read_u64(cursor, "an f64")?)),
        WireType::Str => {
            let length = read_u32(cursor, "a string length")? as usize;
            let bytes = cursor.take(length, "a string")?;
            match std::str::from_utf8(bytes) {
                Ok(text) => format!("{text:?} ({length} bytes)"),
                Err(_) => {
                    return Err(format!(
                        "offset {}: a string that is not valid UTF-8",
                        cursor.at - length
                    ))
                }
            }
        }
        WireType::Bytes => {
            let length = read_u32(cursor, "a bytes length")? as usize;
            let bytes = cursor.take(length, "a bytes blob")?;
            format!("{} ({length} bytes)", hex(bytes))
        }
        WireType::Array(_) | WireType::Model(_) => {
            unreachable!("composites are handled by the caller")
        }
    })
}

fn read_u64(cursor: &mut Cursor<'_>, what: &str) -> Result<u64, String> {
    let bytes = cursor.take(8, what)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

/// A float, printed so that a whole number still looks like a float and `-0.0`
/// still looks like `-0.0`.
fn format_float(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == value.trunc() && value.is_finite() && value.abs() < 1e15 {
        // `{:.1}` keeps the sign of -0.0, which is a distinct value on the wire
        // (RFC-0002 §2.3) and worth being able to see.
        return format!("{value:.1}");
    }
    format!("{value}")
}

/// Bytes as `64 00 00 00`, shortened in the middle if there are many.
fn hex(bytes: &[u8]) -> String {
    const MAX: usize = 16;
    let shown: Vec<String> = bytes
        .iter()
        .take(MAX)
        .map(|byte| format!("{byte:02X}"))
        .collect();
    if bytes.len() > MAX {
        return format!("{} … ({} bytes)", shown.join(" "), bytes.len());
    }
    shown.join(" ")
}

#[cfg(test)]
mod tests {
    use super::{format_float, from_hex, hex};

    #[test]
    fn hex_input_ignores_spacing() {
        assert_eq!(from_hex("64 00 00 00").expect("hex"), [0x64, 0, 0, 0]);
        assert_eq!(from_hex("0x64,0x00").expect("hex"), [0x64, 0]);
        assert_eq!(from_hex("6400").expect("hex"), [0x64, 0]);
        assert!(from_hex("640").is_err());
        assert!(from_hex("zz").is_err());
    }

    #[test]
    fn bytes_are_shown_in_upper_case_pairs() {
        assert_eq!(hex(&[0x64, 0x00, 0xAC, 0x41]), "64 00 AC 41");
    }

    #[test]
    fn a_whole_float_still_looks_like_a_float() {
        assert_eq!(format_float(20.0), "20.0");
        assert_eq!(format_float(10.5), "10.5");
        assert_eq!(format_float(-0.0), "-0.0");
    }
}
