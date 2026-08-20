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
    marker: Option<Option<String>>,
    codecs: Vec<String>,
    line: usize,
}

impl Pending {
    fn clear(&mut self) {
        self.marker = None;
        self.codecs.clear();
        self.line = 0;
    }
}

impl<'a> Scanner<'a> {
    fn file(&mut self) -> Result<Vec<Model>, Error> {
        let mut models = Vec::new();
        let mut pending = Pending::default();

        while let Some(token) = self.peek() {
            match token.kind {
                Kind::Directive(text) => {
                    let line = token.line;
                    self.bump();
                    self.apply_directive(line, text, &mut pending)?;
                }

                Kind::Ident("export") | Kind::Ident("default") | Kind::Ident("abstract")
                    if pending.marker.is_some() =>
                {
                    self.bump();
                }

                Kind::Ident("class") => {
                    self.bump();
                    if let Some(model) = self.klass(&mut pending)? {
                        models.push(model);
                    }
                    pending.clear();
                }

                _ => {
                    if pending.marker.is_some() {
                        return Err(self.error(
                            token.line,
                            "CYCLONE_MODEL must be immediately followed by a `class` \
                             declaration"
                                .to_owned(),
                        ));
                    }
                    self.bump();
                }
            }
        }

        if pending.marker.is_some() {
            return Err(self.error(
                pending.line,
                "CYCLONE_MODEL must be immediately followed by a `class` declaration".to_owned(),
            ));
        }

        Ok(models)
    }

    fn klass(&mut self, pending: &mut Pending) -> Result<Option<Model>, Error> {
        let is_model = pending.marker.is_some();
        let line = if pending.line == 0 {
            self.peek().map_or(1, |token| token.line)
        } else {
            pending.line
        };

        let name = self.peek().and_then(Token::ident).map(str::to_owned);
        if name.is_some() {
            self.bump();
        }

        let body = self.class_header_tail();

        let Some(name) = name else {
            if is_model {
                return Err(self.error(line, "CYCLONE_MODEL requires a named class".to_owned()));
            }
            if let Some(open) = body {
                self.at = self.matching(open, '{', '}') + 1;
            }
            return Ok(None);
        };

        if !is_model {
            if let Some(open) = body {
                self.at = self.matching(open, '{', '}') + 1;
            }
            return Ok(None);
        }

        let Some(open) = body else {
            return Err(self.error(
                line,
                format!("CYCLONE_MODEL marks `{name}`, which has no class body"),
            ));
        };

        Ok(Some(Model {
            name,
            source: self.path.to_path_buf(),
            line,
            codecs: dedupe(std::mem::take(&mut pending.codecs)),
            fields: self.fields(open)?,
        }))
    }

    fn fields(&mut self, open: usize) -> Result<Vec<Field>, Error> {
        let close = self.matching(open, '{', '}');
        self.at = open + 1;

        let mut fields = Vec::new();
        let mut pending = Pending::default();

        while self.at < close {
            let token = self.tokens[self.at];

            match token.kind {
                Kind::Directive(text) => {
                    self.bump();
                    self.apply_directive(token.line, text, &mut pending)?;
                    continue;
                }
                Kind::Ident(word) if is_member_modifier(word) => {
                    self.bump();
                    continue;
                }
                Kind::Punct(';') | Kind::Punct(',') => {
                    self.bump();
                    continue;
                }
                Kind::Punct('@') => {
                    self.skip_decorator();
                    continue;
                }
                _ => {}
            }

            let Some(name) = token.ident() else {
                self.dangling(&pending)?;
                self.bump();
                continue;
            };
            self.bump();

            if (name == "get" || name == "set") && self.peek().and_then(Token::ident).is_some() {
                self.dangling(&pending)?;
                self.bump();
                self.skip_member_tail(close);
                pending.clear();
                continue;
            }

            if self.at_method_start() {
                self.dangling(&pending)?;
                self.skip_member_tail(close);
                pending.clear();
                continue;
            }

            let line = pending.line;
            self.skip_field_tail(close);

            match (pending.marker.take(), pending.codecs.is_empty()) {
                (Some(None), _) => {
                    return Err(self.error(
                        line,
                        "CYCLONE_FIELD requires a wire type in parentheses".to_owned(),
                    ));
                }
                (Some(Some(network_type)), _) => fields.push(Field {
                    name: name.to_owned(),
                    network_type,
                    codecs: dedupe(std::mem::take(&mut pending.codecs)),
                    line,
                }),
                (None, false) => {
                    return Err(self.error(
                        line,
                        format!("field '{name}' has CYCLONE_CODEC but no CYCLONE_FIELD wire type"),
                    ));
                }
                (None, true) => {}
            }
            pending.clear();
        }

        self.at = close + 1;
        Ok(fields)
    }

    fn dangling(&self, pending: &Pending) -> Result<(), Error> {
        if pending.marker.is_some() || !pending.codecs.is_empty() {
            return Err(self.error(
                pending.line,
                "CYCLONE_FIELD must be immediately followed by a field declaration".to_owned(),
            ));
        }
        Ok(())
    }

    fn apply_directive(&self, line: usize, text: &str, pending: &mut Pending) -> Result<(), Error> {
        let (name, args) = split_directive(text);

        match name {
            "CYCLONE_MODEL" => {
                if pending.marker.is_some() {
                    return Err(self.error(line, "duplicate CYCLONE_MODEL annotation".to_owned()));
                }
                if pending.line == 0 {
                    pending.line = line;
                }
                pending.marker = Some(None);
            }
            "CYCLONE_FIELD" => {
                if pending.marker.is_some() {
                    return Err(self.error(line, "duplicate CYCLONE_FIELD annotation".to_owned()));
                }
                if pending.line == 0 {
                    pending.line = line;
                }
                pending.marker = Some(match args {
                    Args::Malformed => {
                        return Err(self.error(
                            line,
                            "malformed CYCLONE_FIELD: missing closing parenthesis".to_owned(),
                        ))
                    }
                    Args::Some(body) if !body.trim().is_empty() => Some(body.trim().to_owned()),
                    _ => None,
                });
            }
            "CYCLONE_CODEC" => {
                if pending.line == 0 {
                    pending.line = line;
                }
                match args {
                    Args::Some(body) => {
                        let codecs =
                            parse_codec_args(body).map_err(|message| self.error(line, message))?;
                        pending.codecs.extend(codecs);
                    }
                    Args::Malformed => {
                        return Err(self.error(
                            line,
                            "malformed CYCLONE_CODEC: missing closing parenthesis".to_owned(),
                        ))
                    }
                    Args::None => {
                        return Err(self.error(
                            line,
                            "malformed CYCLONE_CODEC: expected CYCLONE_CODEC(\"name\", ...)"
                                .to_owned(),
                        ))
                    }
                }
            }
            _ => {}
        }

        Ok(())
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

    fn class_header_tail(&mut self) -> Option<usize> {
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

    fn at_method_start(&self) -> bool {
        matches!(
            self.peek().map(|token| token.kind),
            Some(Kind::Punct('(')) | Some(Kind::Punct('<'))
        )
    }

    fn skip_decorator(&mut self) {
        self.bump();
        loop {
            match self.peek().map(|token| token.kind) {
                Some(Kind::Ident(_)) | Some(Kind::Punct('.')) => self.bump(),
                Some(Kind::Punct('(')) => {
                    self.skip_balanced('(', ')');
                    break;
                }
                _ => break,
            }
        }
    }

    fn skip_member_tail(&mut self, close: usize) {
        if self
            .peek()
            .is_some_and(|token| token.kind == Kind::Punct('<'))
        {
            self.skip_balanced('<', '>');
        }
        if self
            .peek()
            .is_some_and(|token| token.kind == Kind::Punct('('))
        {
            self.skip_balanced('(', ')');
        }
        if self
            .peek()
            .is_some_and(|token| token.kind == Kind::Punct(':'))
        {
            self.bump();
            self.skip_type_until(close, &['{', ';']);
        }
        match self.peek().map(|token| token.kind) {
            Some(Kind::Punct('{')) => {
                let open = self.at;
                self.at = self.matching(open, '{', '}') + 1;
            }
            Some(Kind::Punct(';')) => self.bump(),
            _ => {}
        }
    }

    fn skip_field_tail(&mut self, close: usize) {
        if matches!(
            self.peek().map(|token| token.kind),
            Some(Kind::Punct('?')) | Some(Kind::Punct('!'))
        ) {
            self.bump();
        }
        if self
            .peek()
            .is_some_and(|token| token.kind == Kind::Punct(':'))
        {
            self.bump();
            self.skip_type_until(close, &[';', ',', '=']);
        }
        if self
            .peek()
            .is_some_and(|token| token.kind == Kind::Punct('='))
        {
            self.bump();
            self.skip_value_until(close, &[';', ',']);
        }
        if matches!(
            self.peek().map(|token| token.kind),
            Some(Kind::Punct(';')) | Some(Kind::Punct(','))
        ) {
            self.bump();
        }
    }

    fn skip_type_until(&mut self, close: usize, stop: &[char]) {
        let mut depth = 0i32;

        while self.at < close {
            match self.tokens[self.at].kind {
                Kind::Punct(character) if depth == 0 && stop.contains(&character) => return,
                Kind::Punct('(') | Kind::Punct('[') | Kind::Punct('{') | Kind::Punct('<') => {
                    depth += 1;
                }
                Kind::Punct(')') | Kind::Punct(']') | Kind::Punct('}') | Kind::Punct('>') => {
                    depth -= 1;
                }
                _ => {}
            }
            self.bump();
        }
    }

    fn skip_value_until(&mut self, close: usize, stop: &[char]) {
        let mut depth = 0i32;

        while self.at < close {
            match self.tokens[self.at].kind {
                Kind::Punct(character) if depth == 0 && stop.contains(&character) => return,
                Kind::Punct('(') | Kind::Punct('[') | Kind::Punct('{') => depth += 1,
                Kind::Punct(')') | Kind::Punct(']') | Kind::Punct('}') => depth -= 1,
                _ => {}
            }
            self.bump();
        }
    }

    fn skip_balanced(&mut self, open: char, close: char) {
        let start = self.at;
        self.at = self.matching(start, open, close) + 1;
    }
}

fn split_directive(text: &str) -> (&str, Args<'_>) {
    let name_end = text
        .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .unwrap_or(text.len());
    let name = &text[..name_end];
    let rest = text[name_end..].trim_start();

    let Some(inner) = rest.strip_prefix('(') else {
        return (name, Args::None);
    };
    match inner.trim_end().strip_suffix(')') {
        Some(body) => (name, Args::Some(body)),
        None => (name, Args::Malformed),
    }
}

enum Args<'a> {
    None,
    Malformed,
    Some(&'a str),
}

fn parse_codec_args(body: &str) -> Result<Vec<String>, String> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();

    for raw in body.split(',') {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        let quoted = item
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .or_else(|| {
                item.strip_prefix('\'')
                    .and_then(|rest| rest.strip_suffix('\''))
            });
        let Some(name) = quoted else {
            return Err(format!(
                "malformed CYCLONE_CODEC: `{item}` is not a quoted codec name"
            ));
        };
        if name.is_empty() {
            return Err("malformed CYCLONE_CODEC: a codec name cannot be empty".to_owned());
        }
        if seen.iter().any(|kept| kept == name) {
            continue;
        }
        seen.push(name.to_owned());
        out.push(name.to_owned());
    }

    Ok(out)
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

fn is_member_modifier(word: &str) -> bool {
    matches!(
        word,
        "public"
            | "private"
            | "protected"
            | "readonly"
            | "static"
            | "abstract"
            | "override"
            | "declare"
            | "async"
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
            let content = text[content_start..at].trim();
            if content.starts_with("CYCLONE_") {
                tokens.push(Token {
                    kind: Kind::Directive(content),
                    line,
                });
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

        if byte == b'"' || byte == b'\'' {
            let quote = byte;
            at += 1;
            while at < bytes.len() && bytes[at] != quote {
                if bytes[at] == b'\n' {
                    line += 1;
                }
                at += if bytes[at] == b'\\' { 2 } else { 1 };
            }
            at = (at + 1).min(bytes.len());
            tokens.push(Token {
                kind: Kind::Other,
                line,
            });
            continue;
        }

        if byte == b'`' {
            at += 1;
            while at < bytes.len() && bytes[at] != b'`' {
                if bytes[at] == b'\n' {
                    line += 1;
                }
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
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' || byte >= 0x80
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::parse;

    fn models(source: &str) -> Vec<crate::model::Model> {
        parse(Path::new("test.ts"), source).expect("parse")
    }

    #[test]
    fn reads_the_brief_s_own_example() {
        let models = models(
            r#"
            // CYCLONE_MODEL
            // CYCLONE_CODEC("edge", "unity")
            class DeviceState {
                // CYCLONE_FIELD(u32)
                // CYCLONE_CODEC("edge", "unity")
                Id: number;

                // CYCLONE_FIELD(f32)
                // CYCLONE_CODEC("edge")
                Temperature: number;

                // CYCLONE_FIELD(string)
                // CYCLONE_CODEC("unity")
                DisplayName: string;
            }
            "#,
        );

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "DeviceState");
        assert_eq!(models[0].codecs, ["edge", "unity"]);
        assert_eq!(models[0].fields.len(), 3);
        assert_eq!(models[0].fields[0].name, "Id");
        assert_eq!(models[0].fields[0].network_type, "u32");
        assert_eq!(models[0].fields[0].codecs, ["edge", "unity"]);
        assert_eq!(models[0].fields[1].network_type, "f32");
        assert_eq!(models[0].fields[1].codecs, ["edge"]);
        assert_eq!(models[0].fields[2].network_type, "string");
        assert_eq!(models[0].fields[2].codecs, ["unity"]);
    }

    #[test]
    fn plain_javascript_fields_carry_no_type_annotation_at_all() {
        let models = models(
            "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass Player {\n    \
             // CYCLONE_FIELD(u32)\n    // CYCLONE_CODEC(\"edge\")\n    Id;\n\n    \
             // CYCLONE_FIELD(f32)\n    // CYCLONE_CODEC(\"edge\")\n    X = 0;\n}\n",
        );
        assert_eq!(models[0].fields.len(), 2);
        assert_eq!(models[0].fields[0].name, "Id");
        assert_eq!(models[0].fields[1].name, "X");
        assert_eq!(models[0].fields[1].network_type, "f32");
    }

    #[test]
    fn an_unmarked_class_is_not_a_model() {
        assert!(models("class Plain { id: number; }").is_empty());
    }

    #[test]
    fn a_field_with_codec_but_no_wire_type_is_an_error() {
        let error = parse(
            Path::new("test.ts"),
            "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass S {\n    \
             // CYCLONE_CODEC(\"edge\")\n    id: number;\n}\n",
        )
        .expect_err("missing wire type");
        assert!(error.message.contains("CYCLONE_FIELD"), "{}", error.message);
    }

    #[test]
    fn a_field_with_no_annotation_at_all_is_skipped_not_an_error() {
        let models = models(
            "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass S {\n    \
             cache: string;\n\n    // CYCLONE_FIELD(u32)\n    // CYCLONE_CODEC(\"edge\")\n    \
             id: number;\n}\n",
        );
        assert_eq!(models[0].fields.len(), 1);
        assert_eq!(models[0].fields[0].name, "id");
    }

    #[test]
    fn a_model_in_a_comment_or_a_string_is_not_a_model() {
        assert!(models("// // CYCLONE_MODEL class Ghost {}").is_empty());
        assert!(models("/* // CYCLONE_MODEL\nclass Ghost {} */").is_empty());
        assert!(models(r#"const s = "// CYCLONE_MODEL class Ghost {}";"#).is_empty());
    }

    #[test]
    fn a_composite_network_type_keeps_its_spelling() {
        let models = models(
            "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass S {\n    \
             // CYCLONE_FIELD(Array<u32>)\n    // CYCLONE_CODEC(\"edge\")\n    xs: number[];\n}\n",
        );
        assert_eq!(models[0].fields[0].network_type, "Array<u32>");
    }

    #[test]
    fn annotations_do_not_leak_past_another_class() {
        let models = models(
            "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass Before {}\n\
             class After { id: number; }\n",
        );
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "Before");
    }

    #[test]
    fn a_model_carries_its_source_and_line() {
        let models = models("\n\n// CYCLONE_MODEL\nclass Player {}\n");
        assert_eq!(models[0].source, Path::new("test.ts"));
        assert_eq!(models[0].line, 3);
    }

    #[test]
    fn cyclone_model_without_a_class_is_an_error() {
        let error = parse(
            Path::new("test.ts"),
            "// CYCLONE_MODEL\nfunction notAClass() {}\n",
        )
        .expect_err("error");
        assert!(error.message.contains("class"), "{}", error.message);
    }

    #[test]
    fn cyclone_model_dangling_at_end_of_file_is_an_error() {
        let error = parse(Path::new("test.ts"), "// CYCLONE_MODEL\n").expect_err("error");
        assert!(error.message.contains("class"), "{}", error.message);
    }

    #[test]
    fn cyclone_field_not_followed_by_a_field_is_an_error() {
        let error = parse(
            Path::new("test.ts"),
            "// CYCLONE_MODEL\nclass S {\n    // CYCLONE_FIELD(u32)\n    doStuff(): void {}\n}\n",
        )
        .expect_err("error");
        assert!(error.message.contains("CYCLONE_FIELD"), "{}", error.message);
    }

    #[test]
    fn a_malformed_codec_is_an_error() {
        let error = parse(
            Path::new("test.ts"),
            "// CYCLONE_MODEL\n// CYCLONE_CODEC(edge)\nclass S {}\n",
        )
        .expect_err("unquoted codec name");
        assert!(error.message.contains("quoted"), "{}", error.message);

        let error = parse(
            Path::new("test.ts"),
            "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\"\nclass S {}\n",
        )
        .expect_err("unclosed paren");
        assert!(
            error.message.contains("closing parenthesis"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_duplicate_cyclone_field_is_an_error() {
        let error = parse(
            Path::new("test.ts"),
            "// CYCLONE_MODEL\nclass S {\n    // CYCLONE_FIELD(u32)\n    \
             // CYCLONE_FIELD(f32)\n    id: number;\n}\n",
        )
        .expect_err("duplicate");
        assert!(error.message.contains("duplicate"), "{}", error.message);
    }

    #[test]
    fn export_default_and_abstract_modifiers_are_allowed_before_class() {
        let default_export =
            models("// CYCLONE_MODEL\nexport default class Player { id: number; }");
        assert_eq!(default_export[0].name, "Player");

        let abstract_class =
            models("// CYCLONE_MODEL\nexport abstract class Player { id: number; }");
        assert_eq!(abstract_class[0].name, "Player");
    }

    #[test]
    fn methods_and_accessors_are_not_mistaken_for_fields() {
        let models = models(
            "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass S {\n    \
             constructor() {}\n\n    \
             get computed(): number { return 1; }\n\n    \
             set computed(value: number) {}\n\n    \
             async doStuff<T>(x: T): Promise<void> {}\n\n    \
             // CYCLONE_FIELD(u32)\n    // CYCLONE_CODEC(\"edge\")\n    id: number;\n}\n",
        );
        assert_eq!(models[0].fields.len(), 1);
        assert_eq!(models[0].fields[0].name, "id");
    }

    #[test]
    fn a_field_with_a_default_value_is_parsed() {
        let models = models(
            "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass S {\n    \
             // CYCLONE_FIELD(u32)\n    // CYCLONE_CODEC(\"edge\")\n    id: number = 0;\n}\n",
        );
        assert_eq!(models[0].fields[0].name, "id");
        assert_eq!(models[0].fields[0].network_type, "u32");
    }

    #[test]
    fn extends_and_implements_are_stepped_over() {
        let models = models(
            "interface Nameable {}\n\n// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\n\
             class Player extends Base<number> implements Nameable {\n    \
             // CYCLONE_FIELD(u32)\n    // CYCLONE_CODEC(\"edge\")\n    id: number;\n}\n",
        );
        assert_eq!(models[0].name, "Player");
        assert_eq!(models[0].fields.len(), 1);
    }

    #[test]
    fn a_javascript_file_parses_the_same_way() {
        let models = parse(
            Path::new("device_state.js"),
            "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\", \"unity\")\nclass DeviceState {\n    \
             // CYCLONE_FIELD(u32)\n    // CYCLONE_CODEC(\"edge\", \"unity\")\n    Id;\n\n    \
             // CYCLONE_FIELD(string)\n    // CYCLONE_CODEC(\"unity\")\n    DisplayName;\n}\n",
        )
        .expect("parse");
        assert_eq!(models[0].name, "DeviceState");
        assert_eq!(models[0].fields.len(), 2);
    }
}
