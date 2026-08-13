//! A JSON value, written and read by hand.
//!
//! `.cyclone/schema.json` is a **project contract**: it gets committed, diffed
//! in review, and compared against a target branch in CI. That makes two
//! properties matter more than convenience.
//!
//! *Key order is authored, not sorted or hashed.* [`Json::Object`] keeps its
//! entries in a `Vec`, so the same schema always serialises to byte-identical
//! text and a diff shows what actually changed.
//!
//! *A number is stored as the text that was written.* Round-tripping a `u64`
//! through `f64` would quietly lose the low bits of a fingerprint, so numbers
//! never become floats here at all.

use std::fmt::Write as _;

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// The literal text of a number, exactly as written or parsed.
    Number(String),
    String(String),
    Array(Vec<Json>),
    /// Entries in authored order; duplicate keys are possible but never
    /// written by this crate.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// An object from its entries, in order.
    pub fn object(entries: Vec<(&str, Json)>) -> Json {
        Json::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    /// A string value.
    pub fn string(text: impl Into<String>) -> Json {
        Json::String(text.into())
    }

    /// An integer value.
    pub fn number(value: impl std::fmt::Display) -> Json {
        Json::Number(value.to_string())
    }

    /// The value of `key`, for an object.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Number(text) => text.parse().ok(),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Object(entries) => Some(entries),
            _ => None,
        }
    }

    /// Renders the value as indented JSON, ending in a newline.
    pub fn to_pretty(&self) -> String {
        let mut out = String::with_capacity(4096);
        write_value(&mut out, self, 0);
        out.push('\n');
        out
    }
}

fn write_value(out: &mut String, value: &Json, depth: usize) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Number(text) => out.push_str(text),
        Json::String(text) => write_string(out, text),
        Json::Array(items) if items.is_empty() => out.push_str("[]"),
        Json::Array(items) => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                indent(out, depth + 1);
                write_value(out, item, depth + 1);
                if index + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(out, depth);
            out.push(']');
        }
        Json::Object(entries) if entries.is_empty() => out.push_str("{}"),
        Json::Object(entries) => {
            out.push_str("{\n");
            for (index, (key, item)) in entries.iter().enumerate() {
                indent(out, depth + 1);
                write_string(out, key);
                out.push_str(": ");
                write_value(out, item, depth + 1);
                if index + 1 < entries.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(out, depth);
            out.push('}');
        }
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", control as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

// ================================================================== the parser

/// Parses JSON text.
///
/// # Errors
///
/// A message naming what was expected and roughly where.
pub fn parse(text: &str) -> Result<Json, String> {
    let bytes = text.as_bytes();
    let mut at = 0;
    skip_whitespace(bytes, &mut at);
    let value = parse_value(text, bytes, &mut at)?;
    skip_whitespace(bytes, &mut at);
    if at != bytes.len() {
        return Err(format!("trailing text at byte {at}"));
    }
    Ok(value)
}

fn parse_value(text: &str, bytes: &[u8], at: &mut usize) -> Result<Json, String> {
    match bytes.get(*at) {
        None => Err("unexpected end of input".to_owned()),
        Some(b'{') => parse_object(text, bytes, at),
        Some(b'[') => parse_array(text, bytes, at),
        Some(b'"') => Ok(Json::String(parse_string(text, bytes, at)?)),
        Some(b't') => literal(bytes, at, "true", Json::Bool(true)),
        Some(b'f') => literal(bytes, at, "false", Json::Bool(false)),
        Some(b'n') => literal(bytes, at, "null", Json::Null),
        Some(_) => parse_number(text, bytes, at),
    }
}

fn parse_object(text: &str, bytes: &[u8], at: &mut usize) -> Result<Json, String> {
    *at += 1;
    let mut entries = Vec::new();
    skip_whitespace(bytes, at);
    if bytes.get(*at) == Some(&b'}') {
        *at += 1;
        return Ok(Json::Object(entries));
    }

    loop {
        skip_whitespace(bytes, at);
        let key = parse_string(text, bytes, at)?;
        skip_whitespace(bytes, at);
        if bytes.get(*at) != Some(&b':') {
            return Err(format!("expected `:` at byte {at}"));
        }
        *at += 1;
        skip_whitespace(bytes, at);
        entries.push((key, parse_value(text, bytes, at)?));
        skip_whitespace(bytes, at);
        match bytes.get(*at) {
            Some(b',') => *at += 1,
            Some(b'}') => {
                *at += 1;
                return Ok(Json::Object(entries));
            }
            _ => return Err(format!("expected `,` or `}}` at byte {at}")),
        }
    }
}

fn parse_array(text: &str, bytes: &[u8], at: &mut usize) -> Result<Json, String> {
    *at += 1;
    let mut items = Vec::new();
    skip_whitespace(bytes, at);
    if bytes.get(*at) == Some(&b']') {
        *at += 1;
        return Ok(Json::Array(items));
    }

    loop {
        skip_whitespace(bytes, at);
        items.push(parse_value(text, bytes, at)?);
        skip_whitespace(bytes, at);
        match bytes.get(*at) {
            Some(b',') => *at += 1,
            Some(b']') => {
                *at += 1;
                return Ok(Json::Array(items));
            }
            _ => return Err(format!("expected `,` or `]` at byte {at}")),
        }
    }
}

fn parse_string(text: &str, bytes: &[u8], at: &mut usize) -> Result<String, String> {
    if bytes.get(*at) != Some(&b'"') {
        return Err(format!("expected a string at byte {at}"));
    }
    *at += 1;

    let mut out = String::new();
    loop {
        match bytes.get(*at) {
            None => return Err("unterminated string".to_owned()),
            Some(b'"') => {
                *at += 1;
                return Ok(out);
            }
            Some(b'\\') => {
                *at += 1;
                let escape = *bytes.get(*at).ok_or("unterminated escape")?;
                *at += 1;
                match escape {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let code = unicode_escape(text, at)?;
                        // A surrogate pair is two escapes; the low half only
                        // means anything joined to the high one.
                        let character = if (0xD800..0xDC00).contains(&code) {
                            if bytes.get(*at) != Some(&b'\\') || bytes.get(*at + 1) != Some(&b'u') {
                                return Err("lone high surrogate".to_owned());
                            }
                            *at += 2;
                            let low = unicode_escape(text, at)?;
                            0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00)
                        } else {
                            code
                        };
                        out.push(char::from_u32(character).ok_or("invalid \\u escape")?);
                    }
                    other => return Err(format!("unknown escape `\\{}`", other as char)),
                }
            }
            Some(_) => {
                // Copy one whole UTF-8 character, not one byte.
                let rest = &text[*at..];
                let character = rest.chars().next().ok_or("invalid utf-8")?;
                out.push(character);
                *at += character.len_utf8();
            }
        }
    }
}

fn unicode_escape(text: &str, at: &mut usize) -> Result<u32, String> {
    let digits = text
        .get(*at..*at + 4)
        .ok_or_else(|| "truncated \\u escape".to_owned())?;
    *at += 4;
    u32::from_str_radix(digits, 16).map_err(|_| format!("invalid \\u escape `{digits}`"))
}

fn parse_number(text: &str, bytes: &[u8], at: &mut usize) -> Result<Json, String> {
    let start = *at;
    if bytes.get(*at) == Some(&b'-') {
        *at += 1;
    }
    while bytes.get(*at).is_some_and(|byte| {
        byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-')
    }) {
        *at += 1;
    }
    if start == *at {
        return Err(format!("expected a value at byte {start}"));
    }
    Ok(Json::Number(text[start..*at].to_owned()))
}

fn literal(bytes: &[u8], at: &mut usize, word: &str, value: Json) -> Result<Json, String> {
    if bytes[*at..].starts_with(word.as_bytes()) {
        *at += word.len();
        return Ok(value);
    }
    Err(format!("expected `{word}` at byte {at}"))
}

fn skip_whitespace(bytes: &[u8], at: &mut usize) {
    while bytes
        .get(*at)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
    {
        *at += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, Json};

    #[test]
    fn round_trips_a_document() {
        let value = Json::object(vec![
            ("schema_version", Json::number(1u32)),
            ("fingerprint", Json::string("sha256:abc")),
            (
                "models",
                Json::object(vec![(
                    "Player",
                    Json::object(vec![(
                        "fields",
                        Json::Array(vec![Json::object(vec![
                            ("name", Json::string("id")),
                            ("type", Json::string("u32")),
                        ])]),
                    )]),
                )]),
            ),
        ]);

        let text = value.to_pretty();
        assert_eq!(parse(&text).expect("parse"), value);
    }

    /// A `u64` fingerprint survives the round trip bit for bit - the reason
    /// numbers are kept as text.
    #[test]
    fn keeps_a_u64_exactly() {
        let text = Json::number(u64::MAX).to_pretty();
        assert_eq!(parse(&text).expect("parse").as_u64(), Some(u64::MAX));
    }

    #[test]
    fn reads_escapes_and_unicode() {
        let value = parse(r#"{"a":"line\nbreak é 😀"}"#).expect("parse");
        assert_eq!(
            value.get("a").and_then(Json::as_str),
            Some("line\nbreak é 😀")
        );
    }

    #[test]
    fn rejects_trailing_text() {
        assert!(parse("{} {}").is_err());
    }
}
