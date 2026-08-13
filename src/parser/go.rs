//! Go source → [`Model`]s.
//!
//! Go has no attributes, so Cyclone metadata lives in the two places Go itself
//! offers for out-of-band information: a `//cyclone:model` comment directive
//! names a model and its codecs, and `` `cyclone:"..." codec:"..."` `` struct
//! tags name a field's wire type and codec membership. See [`crate::parser`]
//! for what this scanner is and is not.
//!
//! ```text
//! //cyclone:model codec=edge,unity     this struct is a model, generate these
//! type DeviceState struct {
//!     ID uint32 `cyclone:"u32" codec:"edge,unity"`   this field's wire type and codecs
//! }
//! ```
//!
//! # Two errors, not zero
//!
//! Every other scanner in this project reports exactly one error - a network
//! field with no wire type. Go's directive comment adds a second failure mode
//! that has no Rust counterpart: `//cyclone:model` is text on a line by
//! itself, with nothing in the language forcing it to sit next to the type it
//! names. A directive not immediately followed by a `type ... struct` - a
//! typo'd struct, a `func`, end of file - would otherwise mark nothing and
//! vanish silently, which is exactly the kind of malformed-directive silence
//! the brief forbids passing over.

use std::path::Path;

use crate::model::{Field, Model};
use crate::parser::Error;

/// Extracts every `//cyclone:model` struct from `text`.
///
/// # Errors
///
/// A `//cyclone:model` directive not immediately followed by a `type ...
/// struct` declaration; a malformed directive argument; a field tagged
/// `codec:"..."` with no `cyclone:"..."` wire type. Source that does not
/// compile for any other reason is the Go compiler's to report.
pub fn parse(path: &Path, text: &str) -> Result<Vec<Model>, Error> {
    let tokens = lex(text);
    Scanner {
        path,
        tokens: &tokens,
        at: 0,
    }
    .file()
}

/// The `package` clause a Go source file declares, if any.
///
/// Go compiles by directory, not by file: every `.go` file in the directory
/// generated output lands in must declare the same package (including the
/// model types the generated codecs name), or nothing in it will resolve.
/// `cyclonec` never resolves that itself; this is where it reads the one name
/// it needs, to spell a qualified reference like `models.Player`.
pub fn package_name(text: &str) -> Option<String> {
    let tokens = lex(text);
    tokens
        .iter()
        .position(|token| token.kind == Kind::Ident("package"))
        .and_then(|index| tokens.get(index + 1))
        .and_then(Token::ident)
        .map(str::to_owned)
}

// ============================================================== the scanner

struct Scanner<'a> {
    path: &'a Path,
    tokens: &'a [Token<'a>],
    at: usize,
}

impl<'a> Scanner<'a> {
    /// Walks a file, collecting models declared at the top level.
    fn file(&mut self) -> Result<Vec<Model>, Error> {
        let mut models = Vec::new();

        while let Some(token) = self.peek() {
            match token.kind {
                Kind::Directive(arguments) => {
                    let line = token.line;
                    self.bump();
                    let codecs = parse_directive_arguments(arguments)
                        .map_err(|message| self.error(line, message))?;
                    models.push(self.model_after_directive(line, codecs)?);
                }
                _ => {
                    self.bump();
                }
            }
        }

        Ok(models)
    }

    /// Reads the `type Name struct { ... }` a directive must be followed by.
    fn model_after_directive(&mut self, line: usize, codecs: Vec<String>) -> Result<Model, Error> {
        if !self
            .peek()
            .is_some_and(|token| token.kind == Kind::Ident("type"))
        {
            return Err(self.error(
                line,
                "//cyclone:model must be immediately followed by a `type Name struct { ... }` \
                 declaration"
                    .to_owned(),
            ));
        }
        self.bump();

        let Some(name) = self.peek().and_then(Token::ident) else {
            return Err(self.error(
                line,
                "//cyclone:model: expected a type name after `type`".to_owned(),
            ));
        };
        self.bump();

        if !self
            .peek()
            .is_some_and(|token| token.kind == Kind::Ident("struct"))
        {
            return Err(self.error(
                line,
                format!(
                    "//cyclone:model marks `{name}`, which is not a struct: only a struct \
                     can be a Cyclone model"
                ),
            ));
        }
        self.bump();

        if !self
            .peek()
            .is_some_and(|token| token.kind == Kind::Punct('{'))
        {
            return Err(self.error(line, "//cyclone:model: expected `struct {`".to_owned()));
        }
        let open = self.at;

        Ok(Model {
            name: name.to_owned(),
            source: self.path.to_path_buf(),
            line,
            codecs,
            fields: self.fields(open)?,
        })
    }

    /// Reads the tagged fields out of a struct body.
    ///
    /// `open` is the index of the body's `{`; the cursor is left past its `}`.
    fn fields(&mut self, open: usize) -> Result<Vec<Field>, Error> {
        let close = self.matching(open, '{', '}');
        self.at = open + 1;

        let mut fields = Vec::new();

        while self.at < close {
            let token = self.tokens[self.at];

            let Some(name) = token.ident() else {
                self.bump();
                continue;
            };

            // A field is `Name Type` (or `Name Type \`tag\``), always the first
            // thing on its line inside the body. A comma-separated name list
            // (`A, B int`) is deliberately not supported, so only the leading
            // identifier is read as a name, and everything else up to the end
            // of the line is the type and, optionally, the tag.
            let line = token.line;
            self.bump();

            let mut tag = None;
            while self.at < close && self.tokens[self.at].line == line {
                if let Kind::Str(text) = self.tokens[self.at].kind {
                    tag = Some(text);
                }
                self.bump();
            }

            let Some(tag) = tag else {
                // No struct tag at all: not a network field, and not an error -
                // the same treatment an unannotated field gets in Rust.
                continue;
            };

            let parsed = parse_tag(tag);
            let codecs = parsed.codec.map(split_codec_list).unwrap_or_default();

            match parsed.cyclone {
                Some(network_type) if !network_type.is_empty() => {
                    fields.push(Field {
                        name: name.to_owned(),
                        network_type,
                        codecs,
                        line,
                    });
                }
                _ if !codecs.is_empty() => {
                    return Err(
                        self.error(line, format!("field '{name}' is missing cyclone wire type"))
                    );
                }
                // Neither `cyclone` nor `codec` present: an ordinary Go struct
                // tag (`json:"..."`, say) on a field Cyclone was never told
                // about.
                _ => {}
            }
        }

        self.at = close + 1;
        Ok(fields)
    }

    // ------------------------------------------------------------- movement

    fn peek(&self) -> Option<&'a Token<'a>> {
        self.tokens.get(self.at)
    }

    fn bump(&mut self) {
        self.at += 1;
    }

    fn error(&self, line: usize, message: String) -> Error {
        Error {
            path: self.path.to_path_buf(),
            line,
            message,
        }
    }

    /// The index of the bracket closing the one at `start`.
    fn matching(&self, start: usize, open: char, close: char) -> usize {
        let mut depth = 0i32;

        for index in start..self.tokens.len() {
            let kind = self.tokens[index].kind;
            if kind == Kind::Punct(open) {
                depth += 1;
            } else if kind == Kind::Punct(close) {
                depth -= 1;
                if depth == 0 {
                    return index;
                }
            }
        }

        self.tokens.len().saturating_sub(1)
    }
}

/// Parses the text after `//cyclone:model`: either nothing (a model with no
/// codecs - valid, and generates nothing, the same as bare `#[network]` in
/// Rust), or `codec=name,name,...`.
fn parse_directive_arguments(text: &str) -> Result<Vec<String>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }

    let Some(list) = text.strip_prefix("codec=") else {
        return Err(format!(
            "invalid //cyclone:model directive: expected nothing or `codec=name,name,...`, \
             found `{text}`"
        ));
    };

    Ok(split_codec_list(list.to_owned()))
}

/// Splits and trims a comma-separated codec list, dropping empties (so
/// `codec=` and `codec=,` both mean "no codecs" rather than one blank name)
/// and repeats, keeping the order written.
fn split_codec_list(list: String) -> Vec<String> {
    let mut seen = Vec::new();
    let mut out = Vec::new();

    for name in list.split(',') {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if seen.iter().any(|kept: &String| kept == name) {
            continue;
        }
        seen.push(name.to_owned());
        out.push(name.to_owned());
    }

    out
}

/// The `cyclone` and `codec` values out of a struct tag, if present.
struct Tag {
    cyclone: Option<String>,
    codec: Option<String>,
}

/// Parses a Go struct tag: `` key:"value" key:"value" `` (the format
/// `reflect.StructTag` reads), picking out `cyclone` and `codec`.
///
/// Only enough of the grammar to read those two keys correctly, including
/// through backslash escapes in the values, so a tag with a `"` or `\` in some
/// other key's value does not throw off where `cyclone`'s or `codec`'s value
/// ends. Any other key (`json`, `yaml`, ...) is skipped.
fn parse_tag(text: &str) -> Tag {
    let mut tag = Tag {
        cyclone: None,
        codec: None,
    };
    let bytes = text.as_bytes();
    let mut at = 0;

    while at < bytes.len() {
        while at < bytes.len() && bytes[at] == b' ' {
            at += 1;
        }
        if at >= bytes.len() {
            break;
        }

        let key_start = at;
        while at < bytes.len() && bytes[at] != b':' && bytes[at] != b' ' {
            at += 1;
        }
        let key = &text[key_start..at];

        if at >= bytes.len() || bytes[at] != b':' || bytes.get(at + 1) != Some(&b'"') {
            // Not a well-formed `key:"value"` from here on; stop rather than
            // guess. Whatever named `cyclone`/`codec` earlier is still used.
            break;
        }
        at += 2;

        let value_start = at;
        while at < bytes.len() && bytes[at] != b'"' {
            at += if bytes[at] == b'\\' { 2 } else { 1 };
        }
        let raw_value = &text[value_start..at.min(bytes.len())];
        at = (at + 1).min(bytes.len());

        let value = unescape(raw_value);
        match key {
            "cyclone" if tag.cyclone.is_none() => tag.cyclone = Some(value),
            "codec" if tag.codec.is_none() => tag.codec = Some(value),
            _ => {}
        }
    }

    tag
}

/// Resolves `\"` and `\\` inside a struct tag value. Cyclone wire types and
/// codec names never legitimately contain either, but a tag copied from
/// elsewhere might, and the text must still be read back correctly.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();

    while let Some(character) = chars.next() {
        if character == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
                continue;
            }
        }
        out.push(character);
    }

    out
}

// =================================================================== the lexer

/// One token, borrowed from the source.
#[derive(Debug, Clone, Copy)]
struct Token<'a> {
    kind: Kind<'a>,
    line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind<'a> {
    Ident(&'a str),
    /// A quoted or backtick-quoted string's contents.
    Str(&'a str),
    /// A `//cyclone:model` directive, holding the text after that prefix.
    Directive(&'a str),
    Punct(char),
    /// A numeric or rune literal. Never meaningful here, only skipped.
    Other,
}

impl<'a> Token<'a> {
    fn ident(&self) -> Option<&'a str> {
        match self.kind {
            Kind::Ident(name) => Some(name),
            _ => None,
        }
    }
}

/// The exact prefix a directive comment starts with.
const DIRECTIVE_PREFIX: &str = "cyclone:model";

/// Splits Go source into tokens.
///
/// Every comment is dropped except one shape: a line comment `//` immediately
/// (no space) followed by `cyclone:model` becomes a [`Kind::Directive`] token
/// instead of being discarded, because unlike every other comment in every
/// language this project reads, that one *is* source.
fn lex(text: &str) -> Vec<Token<'_>> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;
    let mut line = 1;

    while at < bytes.len() {
        let byte = bytes[at];

        if byte == b'\n' {
            line += 1;
            at += 1;
            continue;
        }
        if byte.is_ascii_whitespace() {
            at += 1;
            continue;
        }

        if byte == b'/' && bytes.get(at + 1) == Some(&b'/') {
            let content_start = at + 2;
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            let rest = &text[content_start..at];
            if let Some(arguments) = rest.strip_prefix(DIRECTIVE_PREFIX) {
                // A word boundary after the prefix: `//cyclone:model` and
                // `//cyclone:model codec=edge` are the directive;
                // `//cyclone:modeling` is an ordinary comment that happens to
                // start the same way.
                if arguments.is_empty() || arguments.starts_with(char::is_whitespace) {
                    tokens.push(Token {
                        kind: Kind::Directive(arguments),
                        line,
                    });
                }
            }
            continue;
        }

        if byte == b'/' && bytes.get(at + 1) == Some(&b'*') {
            at += 2;
            while at < bytes.len() && !(bytes[at] == b'*' && bytes.get(at + 1) == Some(&b'/')) {
                if bytes[at] == b'\n' {
                    line += 1;
                }
                at += 1;
            }
            at = (at + 2).min(bytes.len());
            continue;
        }

        // Backtick raw strings: struct tags, and any other raw string literal.
        // No escapes at all in Go - the only way to end one is another
        // backtick.
        if byte == b'`' {
            let start = at + 1;
            at += 1;
            while at < bytes.len() && bytes[at] != b'`' {
                if bytes[at] == b'\n' {
                    line += 1;
                }
                at += 1;
            }
            tokens.push(Token {
                kind: Kind::Str(&text[start..at]),
                line,
            });
            at = (at + 1).min(bytes.len());
            continue;
        }

        // Interpreted strings: `"..."`, with backslash escapes.
        if byte == b'"' {
            let start = at + 1;
            at += 1;
            while at < bytes.len() && bytes[at] != b'"' && bytes[at] != b'\n' {
                at += if bytes[at] == b'\\' { 2 } else { 1 };
            }
            tokens.push(Token {
                kind: Kind::Str(&text[start..at.min(bytes.len())]),
                line,
            });
            at = (at + 1).min(bytes.len());
            continue;
        }

        // Rune literals: `'x'`, `'\n'`, `'\''`.
        if byte == b'\'' {
            at += 1;
            while at < bytes.len() && bytes[at] != b'\'' {
                at += if bytes[at] == b'\\' { 2 } else { 1 };
            }
            at = (at + 1).min(bytes.len());
            tokens.push(Token {
                kind: Kind::Other,
                line,
            });
            continue;
        }

        if is_ident_start(byte) {
            let start = at;
            while at < bytes.len() && is_ident_continue(bytes[at]) {
                at += 1;
            }
            tokens.push(Token {
                kind: Kind::Ident(&text[start..at]),
                line,
            });
            continue;
        }

        if byte.is_ascii_digit() {
            while at < bytes.len() && (is_ident_continue(bytes[at]) || bytes[at] == b'.') {
                at += 1;
            }
            tokens.push(Token {
                kind: Kind::Other,
                line,
            });
            continue;
        }

        tokens.push(Token {
            kind: Kind::Punct(byte as char),
            line,
        });
        at += 1;
    }

    tokens
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{package_name, parse};

    #[test]
    fn a_directive_followed_by_a_struct_is_a_model() {
        let text = "package models\n\n//cyclone:model codec=edge,unity\ntype DeviceState struct {\n\tID uint32 `cyclone:\"u32\" codec:\"edge,unity\"`\n}\n";
        let models = parse(Path::new("device_state.go"), text).expect("parse");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "DeviceState");
        assert_eq!(models[0].codecs, ["edge", "unity"]);
        assert_eq!(models[0].fields.len(), 1);
        assert_eq!(models[0].fields[0].network_type, "u32");
    }

    #[test]
    fn a_directive_not_followed_by_a_struct_is_an_error() {
        let text = "//cyclone:model\nfunc notAStruct() {}\n";
        let error = parse(Path::new("bad.go"), text).expect_err("error");
        assert!(
            error.message.contains("type Name struct"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_field_with_codec_but_no_wire_type_is_an_error() {
        let text =
            "//cyclone:model codec=edge\ntype Player struct {\n\tID uint32 `codec:\"edge\"`\n}\n";
        let error = parse(Path::new("player.go"), text).expect_err("error");
        assert!(
            error.message.contains("missing cyclone wire type"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_field_with_no_tag_at_all_is_skipped_not_an_error() {
        let text = "//cyclone:model codec=edge\ntype Player struct {\n\tCache string\n\tID uint32 `cyclone:\"u32\" codec:\"edge\"`\n}\n";
        let models = parse(Path::new("player.go"), text).expect("parse");
        assert_eq!(models[0].fields.len(), 1);
        assert_eq!(models[0].fields[0].name, "ID");
    }

    #[test]
    fn the_package_clause_is_read_for_import_qualification() {
        assert_eq!(
            package_name("package models\n\ntype X struct{}\n"),
            Some("models".to_owned())
        );
        assert_eq!(package_name("type X struct{}\n"), None);
    }

    #[test]
    fn a_directive_not_immediately_followed_by_type_is_not_silently_dropped() {
        let text = "//cyclone:model\nvar notAType = 1\n";
        let error = parse(Path::new("player.go"), text).expect_err("error");
        assert!(
            error.message.contains("immediately followed"),
            "{}",
            error.message
        );
    }
}
