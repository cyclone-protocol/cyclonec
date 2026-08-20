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
    let mut at = start + 1;
    loop {
        match tokens.get(at).map(|token| token.kind) {
            Some(Kind::Ident(part)) => {
                name.push_str(part);
                at += 1;
            }
            Some(Kind::Punct(':'))
                if tokens.get(at + 1).map(|token| token.kind) == Some(Kind::Punct(':')) =>
            {
                name.push_str("::");
                at += 2;
            }
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
    model: bool,
    field_type: Option<Option<String>>,
    codecs: Vec<String>,
    line: usize,
}

impl Pending {
    fn clear(&mut self) {
        self.model = false;
        self.field_type = None;
        self.codecs.clear();
        self.line = 0;
    }

    fn is_empty(&self) -> bool {
        !self.model && self.field_type.is_none() && self.codecs.is_empty()
    }
}

impl<'a> Scanner<'a> {
    fn file(&mut self) -> Result<Vec<Model>, Error> {
        let mut models = Vec::new();
        let mut pending = Pending::default();

        while let Some(token) = self.peek() {
            match token.kind {
                Kind::Ident("CYCLONE_MODEL") => {
                    let line = token.line;
                    if pending.line == 0 {
                        pending.line = line;
                    }
                    pending.model = true;
                    self.bump();
                }

                Kind::Ident("CYCLONE_CODEC") => {
                    self.codec_directive(&mut pending)?;
                }

                Kind::Ident("struct") | Kind::Ident("class") => {
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

    fn codec_directive(&mut self, pending: &mut Pending) -> Result<(), Error> {
        let line = self.tokens[self.at].line;
        if pending.line == 0 {
            pending.line = line;
        }
        self.bump();

        if !self
            .peek()
            .is_some_and(|token| token.kind == Kind::Punct('('))
        {
            return Ok(());
        }

        let open = self.at;
        let close = self.matching(open, '(', ')');
        self.at = close + 1;

        pending.codecs.extend(
            split_top_level(&self.tokens[open + 1..close])
                .into_iter()
                .filter_map(string_literal),
        );

        Ok(())
    }

    fn field_directive(&mut self, pending: &mut Pending) -> Result<(), Error> {
        let line = self.tokens[self.at].line;
        if pending.line == 0 {
            pending.line = line;
        }
        self.bump();

        if !self
            .peek()
            .is_some_and(|token| token.kind == Kind::Punct('('))
        {
            return Ok(());
        }

        let open = self.at;
        let close = self.matching(open, '(', ')');
        self.at = close + 1;

        let text = render(&self.tokens[open + 1..close]);
        let text = text.trim();
        pending.field_type = Some(if text.is_empty() {
            None
        } else {
            Some(text.to_owned())
        });

        Ok(())
    }

    fn type_declaration(&mut self, pending: &mut Pending) -> Result<Option<Model>, Error> {
        let Some(name) = self.peek().and_then(Token::ident) else {
            return Ok(None);
        };
        self.bump();

        let is_model = pending.model;

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
            let token = self.tokens[self.at];

            if token.kind == Kind::Ident("CYCLONE_CODEC") {
                self.codec_directive(&mut pending)?;
                continue;
            }
            if token.kind == Kind::Ident("CYCLONE_FIELD") {
                self.field_directive(&mut pending)?;
                continue;
            }
            if token.kind == Kind::Punct(';') {
                self.bump();
                pending.clear();
                continue;
            }
            if is_access_specifier(token, self.tokens.get(self.at + 1)) {
                self.at += 2;
                continue;
            }

            match self.declaration_end(close) {
                DeclarationEnd::Method => {
                    if !pending.is_empty() {
                        return Err(self.error(
                            pending.line,
                            "a CYCLONE_FIELD or CYCLONE_CODEC marker is not on a field",
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
                        match pending.field_type.take() {
                            Some(None) => {
                                return Err(self.error(
                                    line,
                                    "CYCLONE_FIELD() requires a wire type: CYCLONE_FIELD(...)",
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
                                    "CYCLONE_CODEC(...) on a field with no CYCLONE_FIELD(...) \
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

fn is_access_specifier(token: Token<'_>, next: Option<&Token<'_>>) -> bool {
    matches!(
        token.kind,
        Kind::Ident("public") | Kind::Ident("private") | Kind::Ident("protected")
    ) && next.is_some_and(|token| token.kind == Kind::Punct(':'))
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

fn render(tokens: &[Token<'_>]) -> String {
    let mut out = String::new();
    for token in tokens {
        match token.kind {
            Kind::Ident(name) => out.push_str(name),
            Kind::Punct(character) => out.push(character),
            Kind::Str(_) | Kind::Other => {}
        }
    }
    out
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
        "namespace" | "enum" | "union" | "typedef" | "using" | "template" | "extern"
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

        if let Some(next) = raw_string_end(bytes, at) {
            line += count_newlines(&bytes[at..next]);
            at = next;
            tokens.push(Token {
                kind: Kind::Other,
                line,
            });
            continue;
        }

        if byte == b'"' {
            let start = at;
            at += 1;
            while at < bytes.len() && bytes[at] != b'"' {
                at += if bytes[at] == b'\\' { 2 } else { 1 };
            }
            let content = &text[start + 1..at.min(text.len())];
            at = (at + 1).min(bytes.len());
            line += count_newlines(&bytes[start..at]);
            tokens.push(Token {
                kind: Kind::Str(content),
                line,
            });
            continue;
        }

        if byte == b'\'' {
            let next = char_literal(bytes, at);
            tokens.push(Token {
                kind: Kind::Other,
                line,
            });
            at = next;
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

fn raw_string_end(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'R') || bytes.get(at + 1) != Some(&b'"') {
        return None;
    }

    let delimiter_start = at + 2;
    let mut cursor = delimiter_start;
    while cursor < bytes.len() && bytes[cursor] != b'(' {
        cursor += 1;
    }
    if cursor >= bytes.len() {
        return Some(bytes.len());
    }
    let delimiter = &bytes[delimiter_start..cursor];
    cursor += 1;

    while cursor < bytes.len() {
        if bytes[cursor] == b')' {
            let after = cursor + 1;
            if bytes[after..].starts_with(delimiter)
                && bytes.get(after + delimiter.len()) == Some(&b'"')
            {
                return Some(after + delimiter.len() + 1);
            }
        }
        cursor += 1;
    }

    Some(bytes.len())
}

fn char_literal(bytes: &[u8], at: usize) -> usize {
    let mut cursor = at + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            b'\'' => return cursor + 1,
            b'\n' => return cursor,
            _ => cursor += 1,
        }
    }
    bytes.len()
}

fn count_newlines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|byte| **byte == b'\n').count()
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
        parse(Path::new("test.hpp"), source).expect("parse")
    }

    #[test]
    fn reads_a_model_its_codecs_and_its_fields() {
        let models = models(
            r#"
            CYCLONE_MODEL
            CYCLONE_CODEC("edge", "unity")
            struct Player
            {
                CYCLONE_FIELD(u32)
                CYCLONE_CODEC("edge", "unity")
                uint32_t Id;

                CYCLONE_FIELD(f32)
                CYCLONE_CODEC("edge")
                float X;

                // Not on the wire at all.
                std::string cache;
            };
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
    fn an_unmarked_type_is_not_a_model() {
        assert!(models("struct Plain { uint32_t id; };").is_empty());
    }

    #[test]
    fn a_field_without_a_wire_type_is_an_error() {
        let error = parse(
            Path::new("test.hpp"),
            "CYCLONE_MODEL\nCYCLONE_CODEC(\"edge\")\nstruct S {\n    CYCLONE_FIELD()\n    uint32_t id;\n};",
        )
        .expect_err("no wire type");

        assert_eq!(error.line, 4);
        assert!(error.message.contains("requires a wire type"));
    }

    #[test]
    fn a_model_in_a_comment_or_a_string_is_not_a_model() {
        assert!(models("// CYCLONE_MODEL struct Ghost { };").is_empty());
        assert!(models("/* CYCLONE_MODEL\nstruct Ghost { }; */").is_empty());
        assert!(models("const char* s = \"CYCLONE_MODEL struct Ghost { };\";").is_empty());
    }

    #[test]
    fn a_composite_wire_type_keeps_its_spelling() {
        let models = models(
            "CYCLONE_MODEL\nCYCLONE_CODEC(\"edge\")\nstruct S {\n    CYCLONE_FIELD(Array<u32>)\n    CYCLONE_CODEC(\"edge\")\n    std::vector<uint32_t> xs;\n};",
        );
        assert_eq!(models[0].fields[0].network_type, "Array<u32>");
    }

    #[test]
    fn markers_do_not_leak_past_another_declaration() {
        let models = models(
            "CYCLONE_MODEL\nCYCLONE_CODEC(\"edge\")\nenum Kind { A };\nstruct After { uint32_t id; };",
        );
        assert!(models.is_empty());
    }

    #[test]
    fn a_model_carries_its_source_and_line() {
        let models = models("\n\nCYCLONE_MODEL\nCYCLONE_CODEC(\"edge\")\nstruct Player {};");
        assert_eq!(models[0].source, Path::new("test.hpp"));
        assert_eq!(models[0].line, 3);
    }

    #[test]
    fn a_method_may_not_carry_a_cyclone_marker() {
        let error = parse(
            Path::new("test.hpp"),
            "CYCLONE_MODEL\nCYCLONE_CODEC(\"edge\")\nstruct S {\n    CYCLONE_FIELD(u32)\n    uint32_t GetId();\n};",
        )
        .expect_err("not a field");
        assert!(error.message.contains("not on a field"));
    }

    #[test]
    fn access_specifiers_do_not_swallow_the_field_after_them() {
        let models = models(
            "CYCLONE_MODEL\nCYCLONE_CODEC(\"edge\")\nstruct S {\npublic:\n    CYCLONE_FIELD(u32)\n    CYCLONE_CODEC(\"edge\")\n    uint32_t id;\nprivate:\n    uint32_t cache;\n};",
        );
        assert_eq!(models[0].fields.len(), 1);
        assert_eq!(models[0].fields[0].name, "id");
    }

    #[test]
    fn a_field_may_carry_a_default_member_initializer() {
        let models = models(
            "CYCLONE_MODEL\nCYCLONE_CODEC(\"edge\")\nstruct S {\n    CYCLONE_FIELD(u32)\n    CYCLONE_CODEC(\"edge\")\n    uint32_t id = 0;\n\n    CYCLONE_FIELD(f32)\n    CYCLONE_CODEC(\"edge\")\n    float x{0.0f};\n};",
        );
        assert_eq!(models[0].fields.len(), 2);
        assert_eq!(models[0].fields[1].name, "x");
    }

    #[test]
    fn the_class_keyword_works_the_same_as_struct() {
        let models = models(
            "CYCLONE_MODEL\nCYCLONE_CODEC(\"edge\")\nclass S {\npublic:\n    CYCLONE_FIELD(u32)\n    CYCLONE_CODEC(\"edge\")\n    uint32_t id;\n};",
        );
        assert_eq!(models[0].fields[0].name, "id");
    }

    #[test]
    fn the_namespace_is_read_for_import_qualification() {
        assert_eq!(
            namespace_name("namespace Game::Models {\n    struct X {};\n}\n"),
            Some("Game::Models".to_owned())
        );
        assert_eq!(
            namespace_name("namespace Game {\n    struct X {};\n}\n"),
            Some("Game".to_owned())
        );
        assert_eq!(namespace_name("struct X {};\n"), None);
    }

    #[test]
    fn a_field_with_no_codec_belongs_to_none_but_is_not_an_error() {
        let models = models(
            "CYCLONE_MODEL\nCYCLONE_CODEC(\"edge\")\nstruct S {\n    CYCLONE_FIELD(u32)\n    uint32_t unrouted;\n};",
        );
        assert_eq!(models[0].fields.len(), 1);
        assert!(models[0].fields[0].codecs.is_empty());
    }

    #[test]
    fn a_codec_with_no_field_marker_is_an_error() {
        let error = parse(
            Path::new("test.hpp"),
            "CYCLONE_MODEL\nCYCLONE_CODEC(\"edge\")\nstruct S {\n    CYCLONE_CODEC(\"edge\")\n    uint32_t id;\n};",
        )
        .expect_err("nothing to route");
        assert!(error.message.contains("has nothing to route"));
    }
}
