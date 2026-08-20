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

pub fn namespace_name(text: &str) -> Option<String> {
    let tokens = lex(text);
    let start = tokens
        .iter()
        .position(|token| token.kind == Kind::Ident("namespace"))?;

    let mut name = String::new();
    for token in &tokens[start + 1..] {
        match token.kind {
            Kind::Ident(part) => name.push_str(part),
            Kind::Punct('.') => name.push('.'),
            _ => break,
        }
    }

    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

struct Scanner<'a> {
    path: &'a Path,
    tokens: &'a [Token<'a>],
    at: usize,
}

#[derive(Default)]
struct Pending {
    network: Option<Option<String>>,
    codecs: Vec<String>,
    line: usize,
}

impl Pending {
    fn clear(&mut self) {
        self.network = None;
        self.codecs.clear();
    }

    fn is_empty(&self) -> bool {
        self.network.is_none() && self.codecs.is_empty()
    }
}

impl<'a> Scanner<'a> {
    fn file(&mut self) -> Result<Vec<Model>, Error> {
        let mut models = Vec::new();
        let mut pending = Pending::default();

        while let Some(token) = self.peek() {
            match token.kind {
                Kind::Punct('[') => {
                    self.attribute_section(&mut pending)?;
                }

                Kind::Ident("class") | Kind::Ident("struct") => {
                    self.bump();
                    let model = self.type_declaration(&mut pending)?;
                    if let Some(model) = model {
                        models.push(model);
                    }
                    pending.clear();
                }

                Kind::Ident(word) if is_boundary_keyword(word) => {
                    self.bump();
                    pending.clear();
                }

                Kind::Punct(';') => {
                    self.bump();
                    pending.clear();
                }

                _ => {
                    self.bump();
                }
            }
        }

        Ok(models)
    }

    fn attribute_section(&mut self, pending: &mut Pending) -> Result<(), Error> {
        let line = self.tokens[self.at].line;
        let open = self.at;
        let close = self.matching(open, '[', ']');
        self.at = close + 1;

        for attribute in split_top_level(&self.tokens[open + 1..close]) {
            self.attribute(attribute, pending, line)?;
        }

        Ok(())
    }

    fn attribute(
        &self,
        tokens: &[Token<'a>],
        pending: &mut Pending,
        line: usize,
    ) -> Result<(), Error> {
        let mut cursor = 0;
        let mut name = None;
        while let Some(token) = tokens.get(cursor) {
            match token.kind {
                Kind::Ident(part) => name = Some(part),
                Kind::Punct('.') => {}
                _ => break,
            }
            cursor += 1;
        }

        let is_network = matches!(name, Some("Network") | Some("NetworkAttribute"));
        let is_codec = matches!(name, Some("Codec") | Some("CodecAttribute"));
        if !is_network && !is_codec {
            return Ok(());
        }

        let arguments = match tokens.get(cursor) {
            Some(token) if token.kind == Kind::Punct('(') => {
                Some(&tokens[cursor + 1..tokens.len().saturating_sub(1)])
            }
            _ => None,
        };

        if is_network {
            if pending.network.is_none() && pending.codecs.is_empty() {
                pending.line = line;
            }
            let wire_type = arguments
                .into_iter()
                .flat_map(split_top_level)
                .find_map(string_literal);
            pending.network = Some(wire_type);
            return Ok(());
        }

        if pending.network.is_none() && pending.codecs.is_empty() {
            pending.line = line;
        }
        pending.codecs.extend(
            arguments
                .into_iter()
                .flat_map(split_top_level)
                .filter_map(string_literal),
        );

        Ok(())
    }

    fn type_declaration(&mut self, pending: &mut Pending) -> Result<Option<Model>, Error> {
        let Some(name) = self.peek().and_then(Token::ident) else {
            return Ok(None);
        };
        self.bump();

        let is_model = pending.network.is_some();

        let body = self.skip_to_body();

        if !is_model {
            if let Some(open) = body {
                self.at = self.matching(open, '{', '}') + 1;
            }
            return Ok(None);
        }

        let model = Model {
            name: name.to_owned(),
            source: self.path.to_path_buf(),
            line: pending.line,
            codecs: dedupe(std::mem::take(&mut pending.codecs)),
            fields: match body {
                Some(open) => self.members(open)?,
                None => Vec::new(),
            },
        };

        Ok(Some(model))
    }

    fn members(&mut self, open: usize) -> Result<Vec<Field>, Error> {
        let close = self.matching(open, '{', '}');
        self.at = open + 1;

        let mut fields = Vec::new();
        let mut pending = Pending::default();

        while self.at < close {
            let token = &self.tokens[self.at];

            if token.kind == Kind::Punct('[') {
                self.attribute_section(&mut pending)?;
                continue;
            }

            if token.kind == Kind::Punct(';') {
                self.bump();
                pending.clear();
                continue;
            }

            match self.declaration_end(close) {
                DeclarationEnd::Method => {
                    if !pending.is_empty() {
                        return Err(self.error(
                            pending.line,
                            "a [Network] or [Codec] annotation is not on a field or property",
                        ));
                    }
                    self.skip_balanced('(', ')');
                    self.skip_member_tail(close);
                }
                DeclarationEnd::Member { name_index } => {
                    let name = self.tokens[name_index].ident().expect("checked by caller");
                    let line = pending.line;
                    self.skip_member_tail(close);

                    if !pending.is_empty() {
                        match pending.network.take() {
                            Some(None) => {
                                return Err(self.error(
                                    line,
                                    "[Network] field requires a wire type: [Network(\"...\")]",
                                ));
                            }
                            Some(Some(network_type)) => fields.push(Field {
                                name: name.to_owned(),
                                network_type,
                                codecs: dedupe(std::mem::take(&mut pending.codecs)),
                                line,
                            }),
                            None if !pending.codecs.is_empty() => {
                                return Err(self.error(
                                    line,
                                    "[Codec(...)] on a field with no [Network(\"...\")] \
                                     has nothing to route: give the field a wire type",
                                ));
                            }
                            None => {}
                        }
                    }

                    pending.clear();
                }
                DeclarationEnd::EndOfBody => {
                    if self.at < close {
                        self.bump();
                    }
                }
            }
        }

        self.at = close + 1;
        Ok(fields)
    }

    fn declaration_end(&mut self, close: usize) -> DeclarationEnd {
        let mut depth = 0i32;
        let mut last_ident = None;

        while self.at < close {
            let token = self.tokens[self.at];
            match token.kind {
                Kind::Ident(_) if depth == 0 => {
                    last_ident = Some(self.at);
                    self.bump();
                }
                Kind::Punct('(') if depth == 0 => {
                    return DeclarationEnd::Method;
                }
                Kind::Punct('{') if depth == 0 => {
                    return match last_ident {
                        Some(name_index) => DeclarationEnd::Member { name_index },
                        None => DeclarationEnd::EndOfBody,
                    };
                }
                Kind::Punct(';') | Kind::Punct('=') if depth == 0 => {
                    return match last_ident {
                        Some(name_index) => DeclarationEnd::Member { name_index },
                        None => DeclarationEnd::EndOfBody,
                    };
                }
                Kind::Punct('<') | Kind::Punct('[') => {
                    depth += 1;
                    self.bump();
                }
                Kind::Punct('>') | Kind::Punct(']') => {
                    depth -= 1;
                    self.bump();
                }
                _ => {
                    self.bump();
                }
            }
        }

        DeclarationEnd::EndOfBody
    }

    fn skip_member_tail(&mut self, close: usize) {
        while self.at < close {
            match self.tokens[self.at].kind {
                Kind::Punct(';') => {
                    self.bump();
                    return;
                }
                Kind::Punct('{') => {
                    self.skip_balanced('{', '}');
                    if !self
                        .tokens
                        .get(self.at)
                        .is_some_and(|token| token.kind == Kind::Punct('='))
                    {
                        return;
                    }
                }
                Kind::Punct('}') => return,
                _ => self.bump(),
            }
        }
    }

    fn peek(&self) -> Option<&'a Token<'a>> {
        self.tokens.get(self.at)
    }

    fn bump(&mut self) {
        self.at += 1;
    }

    fn error(&self, line: usize, message: &str) -> Error {
        Error {
            path: self.path.to_path_buf(),
            line,
            message: message.to_owned(),
        }
    }

    fn skip_to_body(&mut self) -> Option<usize> {
        let mut depth = 0i32;

        while let Some(token) = self.peek() {
            match token.kind {
                Kind::Punct('{') if depth == 0 => return Some(self.at),
                Kind::Punct(';') if depth == 0 => {
                    self.bump();
                    return None;
                }
                Kind::Punct('<') | Kind::Punct('(') | Kind::Punct('[') => depth += 1,
                Kind::Punct('>') | Kind::Punct(')') | Kind::Punct(']') => depth -= 1,
                _ => {}
            }
            self.bump();
        }

        None
    }

    fn skip_balanced(&mut self, open: char, close: char) {
        self.at = self.matching(self.at, open, close) + 1;
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

enum DeclarationEnd {
    Method,
    Member { name_index: usize },
    EndOfBody,
}

fn string_literal(tokens: &[Token<'_>]) -> Option<String> {
    match tokens {
        [Token {
            kind: Kind::Str(text),
            ..
        }] => Some((*text).to_owned()),
        _ => None,
    }
}

fn split_top_level<'a, 'b>(tokens: &'b [Token<'a>]) -> Vec<&'b [Token<'a>]> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;

    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            Kind::Punct('(') | Kind::Punct('[') | Kind::Punct('<') => depth += 1,
            Kind::Punct(')') | Kind::Punct(']') | Kind::Punct('>') => depth -= 1,
            Kind::Punct(',') if depth == 0 => {
                parts.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }

    if start < tokens.len() || !tokens.is_empty() {
        parts.push(&tokens[start..]);
    }
    parts.retain(|part| !part.is_empty());
    parts
}

fn dedupe(mut names: Vec<String>) -> Vec<String> {
    let mut seen = Vec::with_capacity(names.len());
    names.retain(|name| {
        if seen.iter().any(|kept: &String| kept == name) {
            return false;
        }
        seen.push(name.clone());
        true
    });
    names
}

fn is_boundary_keyword(word: &str) -> bool {
    matches!(
        word,
        "namespace" | "using" | "enum" | "interface" | "delegate"
    )
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
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
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

        if byte == b'#' {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }

        if let Some((text, next, lines)) = string_token(text, at) {
            tokens.push(Token {
                kind: Kind::Str(text),
                line,
            });
            line += lines;
            at = next;
            continue;
        }

        if byte == b'\'' {
            let (next, lines) = char_literal(bytes, at);
            tokens.push(Token {
                kind: Kind::Other,
                line,
            });
            line += lines;
            at = next;
            continue;
        }

        if byte == b'@' && bytes.get(at + 1).is_some_and(|b| is_ident_start(*b)) {
            let start = at;
            at += 1;
            while at < bytes.len() && is_ident_continue(bytes[at]) {
                at += 1;
            }
            tokens.push(Token {
                kind: Kind::Ident(&text[start..at]),
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
            while at < bytes.len()
                && (is_ident_continue(bytes[at])
                    || (bytes[at] == b'.' && bytes.get(at + 1) != Some(&b'.')))
            {
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

fn string_token(text: &str, at: usize) -> Option<(&str, usize, usize)> {
    let bytes = text.as_bytes();
    let mut cursor = at;
    let mut verbatim = false;

    while matches!(bytes.get(cursor), Some(b'@') | Some(b'$')) {
        if bytes[cursor] == b'@' {
            verbatim = true;
        }
        cursor += 1;
    }

    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }

    if bytes.get(cursor + 1) == Some(&b'"') && bytes.get(cursor + 2) == Some(&b'"') {
        let mut fence = 0;
        while bytes.get(cursor + fence) == Some(&b'"') {
            fence += 1;
        }
        let content_start = cursor + fence;
        let mut scan = content_start;
        while scan < bytes.len() {
            if bytes[scan] == b'"' {
                let mut run = 0;
                while bytes.get(scan + run) == Some(&b'"') {
                    run += 1;
                }
                if run >= fence {
                    let lines = text[at..scan].bytes().filter(|b| *b == b'\n').count();
                    return Some((&text[content_start..scan], scan + run, lines));
                }
                scan += run;
                continue;
            }
            scan += 1;
        }
        return Some(("", bytes.len(), 0));
    }

    cursor += 1;
    let content_start = cursor;

    if verbatim {
        while cursor < bytes.len() {
            if bytes[cursor] == b'"' {
                if bytes.get(cursor + 1) == Some(&b'"') {
                    cursor += 2;
                    continue;
                }
                let lines = text[at..cursor].bytes().filter(|b| *b == b'\n').count();
                return Some((&text[content_start..cursor], cursor + 1, lines));
            }
            cursor += 1;
        }
        return Some(("", bytes.len(), 0));
    }

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            b'"' => {
                let lines = text[at..cursor].bytes().filter(|b| *b == b'\n').count();
                return Some((&text[content_start..cursor], cursor + 1, lines));
            }
            b'\n' => break,
            _ => cursor += 1,
        }
    }

    Some(("", (cursor).min(bytes.len()), 0))
}

fn char_literal(bytes: &[u8], at: usize) -> (usize, usize) {
    let mut cursor = at + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            b'\'' => return (cursor + 1, 0),
            b'\n' => return (cursor, 0),
            _ => cursor += 1,
        }
    }
    (bytes.len(), 0)
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

    use super::{namespace_name, parse};

    fn models(source: &str) -> Vec<crate::model::Model> {
        parse(Path::new("Test.cs"), source).expect("parse")
    }

    #[test]
    fn reads_a_model_its_codecs_and_its_fields() {
        let models = models(
            r#"
            namespace Game.Models;

            [Network]
            [Codec("edge", "unity")]
            public class Player
            {
                [Network("u32")]
                [Codec("edge", "unity")]
                public uint Id { get; set; }

                [Network("f32")]
                [Codec("edge")]
                public float X { get; set; }

                // Not on the wire at all.
                public string Cache { get; set; }
            }
            "#,
        );

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "Player");
        assert_eq!(models[0].codecs, ["edge", "unity"]);
        assert_eq!(models[0].fields.len(), 2);
        assert_eq!(models[0].fields[1].network_type, "f32");
        assert_eq!(models[0].fields[1].codecs, ["edge"]);
    }

    #[test]
    fn a_field_may_be_a_plain_field_not_only_a_property() {
        let models = models(
            "[Network][Codec(\"edge\")]\nstruct S {\n    [Network(\"u32\")]\n    [Codec(\"edge\")]\n    public uint id;\n}",
        );
        assert_eq!(models[0].fields[0].name, "id");
    }

    #[test]
    fn an_unmarked_type_is_not_a_model() {
        assert!(models("public class Plain { public uint Id; }").is_empty());
    }

    #[test]
    fn a_network_field_without_a_type_is_an_error() {
        let error = parse(
            Path::new("test.cs"),
            "[Network]\n[Codec(\"edge\")]\nclass S {\n    [Network]\n    public uint Id { get; set; }\n}",
        )
        .expect_err("no network type");

        assert_eq!(error.line, 4);
        assert!(error.message.contains("requires a wire type"));
    }

    #[test]
    fn a_model_in_a_comment_or_a_string_is_not_a_model() {
        assert!(models("// [Network] class Ghost { }").is_empty());
        assert!(models("/* [Network]\nclass Ghost { } */").is_empty());
        assert!(models("const string S = \"[Network] class Ghost { }\";").is_empty());
    }

    #[test]
    fn a_composite_network_type_keeps_its_spelling() {
        let models = models(
            "[Network][Codec(\"edge\")]\nclass S {\n    [Network(\"Array<u32>\")]\n    [Codec(\"edge\")]\n    public System.Collections.Generic.List<uint> Xs { get; set; }\n}",
        );
        assert_eq!(models[0].fields[0].network_type, "Array<u32>");
    }

    #[test]
    fn attributes_do_not_leak_past_another_declaration() {
        let models = models(
            "[Network]\n[Codec(\"edge\")]\nenum Kind { A }\nclass After { public uint Id; }",
        );
        assert!(models.is_empty());
    }

    #[test]
    fn a_model_carries_its_source_and_line() {
        let models = models("\n\n[Network]\n[Codec(\"edge\")]\nclass Player {}");
        assert_eq!(models[0].source, Path::new("Test.cs"));
        assert_eq!(models[0].line, 3);
    }

    #[test]
    fn a_method_may_not_carry_network_or_codec() {
        let error = parse(
            Path::new("test.cs"),
            "[Network]\n[Codec(\"edge\")]\nclass S {\n    [Network(\"u32\")]\n    public uint GetId() => 0;\n}",
        )
        .expect_err("not a field");
        assert!(error.message.contains("not on a field or property"));
    }

    #[test]
    fn a_property_with_an_initializer_is_still_read_correctly() {
        let models = models(
            "[Network][Codec(\"edge\")]\nclass S {\n    [Network(\"u32\")]\n    [Codec(\"edge\")]\n    public uint Id { get; set; } = 7;\n\n    [Network(\"string\")]\n    [Codec(\"edge\")]\n    public string Name { get; set; }\n}",
        );
        assert_eq!(models[0].fields.len(), 2);
        assert_eq!(models[0].fields[1].name, "Name");
    }

    #[test]
    fn the_namespace_is_read_for_import_qualification() {
        assert_eq!(
            namespace_name("namespace Game.Models;\n\nclass X {}\n"),
            Some("Game.Models".to_owned())
        );
        assert_eq!(
            namespace_name("namespace Game.Models\n{\n    class X {}\n}\n"),
            Some("Game.Models".to_owned())
        );
        assert_eq!(namespace_name("class X {}\n"), None);
    }

    #[test]
    fn a_qualified_attribute_name_is_still_recognised() {
        let models = models(
            "[Cyclone.Network(\"u32\")]\n[Cyclone.Codec(\"edge\")]\nclass S {\n    [Cyclone.Network(\"u32\")]\n    [Cyclone.Codec(\"edge\")]\n    public uint Id { get; set; }\n}",
        );
        assert_eq!(models[0].fields[0].network_type, "u32");
    }
}
