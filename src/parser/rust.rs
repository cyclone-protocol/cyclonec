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
}

impl<'a> Scanner<'a> {
    fn file(&mut self) -> Result<Vec<Model>, Error> {
        let mut models = Vec::new();
        let mut pending = Pending::default();

        while let Some(token) = self.peek() {
            match token.kind {
                Kind::Punct('#') => {
                    self.attribute(&mut pending)?;
                }

                Kind::Ident("struct") => {
                    self.bump();
                    let model = self.strukt(&mut pending)?;
                    if let Some(model) = model {
                        models.push(model);
                    }
                    pending.clear();
                }

                Kind::Ident(word) if is_item_keyword(word) => {
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

    fn attribute(&mut self, pending: &mut Pending) -> Result<(), Error> {
        let line = self.tokens[self.at].line;
        self.bump();

        if self
            .peek()
            .is_some_and(|token| token.kind == Kind::Punct('!'))
        {
            self.bump();
        }
        if !self
            .peek()
            .is_some_and(|token| token.kind == Kind::Punct('['))
        {
            return Ok(());
        }

        let open = self.at;
        let close = self.matching(open, '[', ']');
        self.at = close + 1;

        let body = &self.tokens[open + 1..close];
        let Some(name) = body.first().and_then(Token::ident) else {
            return Ok(());
        };

        let arguments = match body.get(1) {
            Some(token) if token.kind == Kind::Punct('(') => {
                Some(&body[2..body.len().saturating_sub(1)])
            }
            _ => None,
        };

        match name {
            "network" => {
                if pending.network.is_none() {
                    pending.line = line;
                }
                pending.network = Some(arguments.map(render));
            }
            "codec" => {
                if pending.network.is_none() && pending.codecs.is_empty() {
                    pending.line = line;
                }
                pending.codecs.extend(
                    arguments
                        .into_iter()
                        .flatten()
                        .filter_map(Token::ident)
                        .map(str::to_owned),
                );
            }
            _ => {}
        }

        Ok(())
    }

    fn strukt(&mut self, pending: &mut Pending) -> Result<Option<Model>, Error> {
        let Some(name) = self.peek().and_then(Token::ident) else {
            return Ok(None);
        };
        let line = if pending.line == 0 {
            self.peek().map_or(1, |token| token.line)
        } else {
            pending.line
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
            line,
            codecs: dedupe(std::mem::take(&mut pending.codecs)),
            fields: match body {
                Some(open) => self.fields(open)?,
                None => Vec::new(),
            },
        };

        Ok(Some(model))
    }

    fn fields(&mut self, open: usize) -> Result<Vec<Field>, Error> {
        let close = self.matching(open, '{', '}');
        self.at = open + 1;

        let mut fields = Vec::new();
        let mut pending = Pending::default();

        while self.at < close {
            let token = &self.tokens[self.at];

            if token.kind == Kind::Punct('#') {
                self.attribute(&mut pending)?;
                continue;
            }

            let name = token.ident();
            let is_field = name.is_some()
                && self
                    .tokens
                    .get(self.at + 1)
                    .is_some_and(|next| next.kind == Kind::Punct(':'))
                && !self
                    .tokens
                    .get(self.at + 2)
                    .is_some_and(|after| after.kind == Kind::Punct(':'));

            if !is_field {
                self.bump();
                if token.kind == Kind::Punct(',') {
                    pending.clear();
                }
                continue;
            }

            let name = name.expect("checked above");
            let line = pending.line;
            self.at += 2;
            self.skip_field_type(close);

            match pending.network.take() {
                Some(None) => {
                    return Err(self.error(line, "#[network] field requires a network type"));
                }
                Some(Some(network_type)) => fields.push(Field {
                    name: name.to_owned(),
                    network_type,
                    codecs: dedupe(std::mem::take(&mut pending.codecs)),
                    line,
                }),
                None => {}
            }

            pending.clear();
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

    fn skip_field_type(&mut self, close: usize) {
        let mut depth = 0i32;

        while self.at < close {
            let kind = self.tokens[self.at].kind;
            match kind {
                Kind::Punct(',') if depth == 0 => {
                    self.bump();
                    return;
                }
                Kind::Punct('<') | Kind::Punct('(') | Kind::Punct('[') | Kind::Punct('{') => {
                    depth += 1;
                }
                Kind::Punct('>') | Kind::Punct(')') | Kind::Punct(']') | Kind::Punct('}') => {
                    let is_arrow = kind == Kind::Punct('>')
                        && self.at > 0
                        && self.tokens[self.at - 1].kind == Kind::Punct('-');
                    if !is_arrow {
                        depth -= 1;
                    }
                }
                _ => {}
            }
            self.bump();
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

fn render(tokens: &[Token<'_>]) -> String {
    let mut out = String::new();
    for token in tokens {
        match token.kind {
            Kind::Ident(name) => out.push_str(name),
            Kind::Punct(character) => out.push(character),
            Kind::Other => {}
        }
    }
    out
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

fn is_item_keyword(word: &str) -> bool {
    matches!(
        word,
        "fn" | "enum"
            | "union"
            | "trait"
            | "impl"
            | "mod"
            | "use"
            | "const"
            | "static"
            | "type"
            | "extern"
            | "macro_rules"
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
            let mut depth = 0;
            while at < bytes.len() {
                if bytes[at] == b'\n' {
                    line += 1;
                }
                if bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'*') {
                    depth += 1;
                    at += 2;
                    continue;
                }
                if bytes[at] == b'*' && bytes.get(at + 1) == Some(&b'/') {
                    depth -= 1;
                    at += 2;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                at += 1;
            }
            continue;
        }

        if let Some(next) = raw_string(bytes, at) {
            line += count_newlines(&bytes[at..next]);
            at = next;
            tokens.push(Token {
                kind: Kind::Other,
                line,
            });
            continue;
        }

        let quote = at + usize::from(byte == b'b' && bytes.get(at + 1) == Some(&b'"'));
        if bytes.get(quote) == Some(&b'"') {
            let start = at;
            at = quote + 1;
            while at < bytes.len() && bytes[at] != b'"' {
                at += if bytes[at] == b'\\' { 2 } else { 1 };
            }
            at = (at + 1).min(bytes.len());
            line += count_newlines(&bytes[start..at]);
            tokens.push(Token {
                kind: Kind::Other,
                line,
            });
            continue;
        }

        if byte == b'\'' {
            if let Some(next) = char_literal(bytes, at) {
                at = next;
                tokens.push(Token {
                    kind: Kind::Other,
                    line,
                });
                continue;
            }
            tokens.push(Token {
                kind: Kind::Punct('\''),
                line,
            });
            at += 1;
            continue;
        }

        if is_ident_start(byte) {
            let start = at;
            while at < bytes.len() && is_ident_continue(bytes[at]) {
                at += 1;
            }
            let name = &text[start..at];
            let name = name.strip_prefix("r#").unwrap_or(name);
            tokens.push(Token {
                kind: Kind::Ident(name),
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

fn raw_string(bytes: &[u8], at: usize) -> Option<usize> {
    let mut cursor = at;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;

    let hashes = {
        let start = cursor;
        while bytes.get(cursor) == Some(&b'#') {
            cursor += 1;
        }
        cursor - start
    };

    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }
    cursor += 1;

    while cursor < bytes.len() {
        if bytes[cursor] == b'"' && bytes[cursor + 1..].iter().take(hashes).all(|b| *b == b'#') {
            return Some((cursor + 1 + hashes).min(bytes.len()));
        }
        cursor += 1;
    }

    Some(bytes.len())
}

fn char_literal(bytes: &[u8], at: usize) -> Option<usize> {
    let after_escape = match bytes.get(at + 1) {
        Some(b'\\') => {
            let mut cursor = at + 2;
            while cursor < bytes.len() && bytes[cursor] != b'\'' {
                cursor += 1;
            }
            return (cursor < bytes.len()).then_some(cursor + 1);
        }
        Some(_) => at + 2,
        None => return None,
    };

    (bytes.get(after_escape) == Some(&b'\'')).then_some(after_escape + 1)
}

fn count_newlines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|byte| **byte == b'\n').count()
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80 || byte == b'#'
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::parse;

    fn models(source: &str) -> Vec<crate::model::Model> {
        parse(Path::new("test.rs"), source).expect("parse")
    }

    #[test]
    fn reads_a_model_its_codecs_and_its_fields() {
        let models = models(
            r#"
            #[network]
            #[codec(edge, unity)]
            pub struct Player {
                #[network(u32)]
                #[codec(edge, unity)]
                pub id: u32,

                #[network(f32)]
                #[codec(edge)]
                pub x: f32,

                /// Not on the wire at all.
                pub cache: String,
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
    fn an_unmarked_struct_is_not_a_model() {
        assert!(models("pub struct Plain { pub id: u32 }").is_empty());
    }

    #[test]
    fn a_network_field_without_a_type_is_an_error() {
        let error = parse(
            Path::new("test.rs"),
            "#[network]\n#[codec(edge)]\nstruct S {\n    #[network]\n    id: u32,\n}",
        )
        .expect_err("no network type");

        assert_eq!(error.line, 4);
        assert!(error.message.contains("requires a network type"));
    }

    #[test]
    fn a_model_in_a_comment_or_a_string_is_not_a_model() {
        assert!(models("// #[network] struct Ghost { }").is_empty());
        assert!(models("/* #[network]\nstruct Ghost { } */").is_empty());
        assert!(models(r##"const S: &str = "#[network] struct Ghost { }";"##).is_empty());
    }

    #[test]
    fn a_composite_network_type_keeps_its_spelling() {
        let models = models(
            "#[network]\n#[codec(edge)]\nstruct S {\n    #[network(Array<u32>)]\n    #[codec(edge)]\n    xs: Vec<u32>,\n}",
        );
        assert_eq!(models[0].fields[0].network_type, "Array<u32>");
    }

    #[test]
    fn attributes_do_not_leak_past_another_item() {
        let models =
            models("#[network]\n#[codec(edge)]\nenum Kind { A }\nstruct After { id: u32 }");
        assert!(models.is_empty());
    }

    #[test]
    fn a_model_carries_its_source_and_line() {
        let models = models("\n\n#[network]\n#[codec(edge)]\nstruct Player {}");
        assert_eq!(models[0].source, Path::new("test.rs"));
        assert_eq!(models[0].line, 3);
    }
}
