use std::path::Path;

use crate::model::{Field, Model};
use crate::parser::Error;

pub fn parse(path: &Path, text: &str) -> Result<Vec<Model>, Error> {
    let tokens = lex(text);
    Scanner {
        path,
        tokens: &tokens,
        at: 0,
    }
    .file()
}

pub fn package_name(text: &str) -> Option<String> {
    let tokens = lex(text);
    tokens
        .iter()
        .position(|token| token.kind == Kind::Ident("package"))
        .and_then(|index| tokens.get(index + 1))
        .and_then(Token::ident)
        .map(str::to_owned)
}

struct Scanner<'a> {
    path: &'a Path,
    tokens: &'a [Token<'a>],
    at: usize,
}

impl<'a> Scanner<'a> {
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
                _ => {}
            }
        }

        self.at = close + 1;
        Ok(fields)
    }

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

struct Tag {
    cyclone: Option<String>,
    codec: Option<String>,
}

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

#[derive(Debug, Clone, Copy)]
struct Token<'a> {
    kind: Kind<'a>,
    line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind<'a> {
    Ident(&'a str),
    Str(&'a str),
    Directive(&'a str),
    Punct(char),
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

const DIRECTIVE_PREFIX: &str = "cyclone:model";

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
