//! Rust source → [`Model`]s.
//!
//! Reads `#[network]` / `#[network(TYPE)]` / `#[codec(...)]`. See
//! [`crate::parser`] for what this scanner is and is not.

use std::path::Path;

use crate::model::{Field, Language, Model};
use crate::parser::Error;

/// Extracts every `#[network]` model from `text`.
///
/// `path` is carried for error messages only; nothing reads it.
///
/// # Errors
///
/// Only what stops generation: an annotation the generator needs and cannot
/// read. Source that does not compile for any other reason is `rustc`'s to
/// report, and passes through here without comment.
pub fn parse(path: &Path, text: &str) -> Result<Vec<Model>, Error> {
    let tokens = lex(text);
    Scanner { path, tokens: &tokens, at: 0 }.file()
}

// ============================================================== the scanner

struct Scanner<'a> {
    path: &'a Path,
    tokens: &'a [Token<'a>],
    at: usize,
}

/// The Cyclone attributes collected so far, waiting for the item they precede.
#[derive(Default)]
struct Pending {
    /// `#[network]` or `#[network(TYPE)]`, and the type if it had one.
    network: Option<Option<String>>,
    /// Every identifier from every `#[codec(...)]`, in order.
    codecs: Vec<String>,
    /// The line the first Cyclone attribute was on.
    line: usize,
}

impl Pending {
    fn clear(&mut self) {
        self.network = None;
        self.codecs.clear();
    }
}

impl<'a> Scanner<'a> {
    /// Walks a file, collecting models.
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

                // Any other item keyword ends the run of attributes, so a
                // `#[network]` on an enum is not inherited by the next struct.
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

    /// Reads one attribute, adding it to `pending` if it is Cyclone's.
    fn attribute(&mut self, pending: &mut Pending) -> Result<(), Error> {
        let line = self.tokens[self.at].line;
        self.bump();

        // `#!` is an inner attribute and never one of ours; `#` before anything
        // but `[` is not an attribute at all.
        if self.peek().is_some_and(|token| token.kind == Kind::Punct('!')) {
            self.bump();
        }
        if !self.peek().is_some_and(|token| token.kind == Kind::Punct('[')) {
            return Ok(());
        }

        let open = self.at;
        let close = self.matching(open, '[', ']');
        self.at = close + 1;

        let body = &self.tokens[open + 1..close];
        let Some(name) = body.first().and_then(Token::ident) else {
            return Ok(());
        };

        // The arguments of `name(...)`, if it has any.
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

    /// Reads a struct declaration, returning it if it is a model.
    fn strukt(&mut self, pending: &mut Pending) -> Result<Option<Model>, Error> {
        let line = self.peek().map_or(pending.line, |token| token.line);
        let Some(name) = self.peek().and_then(Token::ident) else {
            return Ok(None);
        };
        self.bump();

        // A struct nothing marks is somebody else's type. It must not become an
        // error just because it shares a file with a model.
        let is_model = pending.network.is_some();
        let _ = line;

        // Generics, a where-clause, a tuple body: stepped over without being
        // read. A `;` before any `{` means the struct has no named fields.
        let body = self.skip_to_body();

        // An unmarked struct is stepped over whole. Its fields are not read, so
        // an annotation inside one cannot leak onto the next struct, and cannot
        // raise an error for a type the generator was never asked about.
        if !is_model {
            if let Some(open) = body {
                self.at = self.matching(open, '{', '}') + 1;
            }
            return Ok(None);
        }

        let model = Model {
            language: Language::Rust,
            name: name.to_owned(),
            codecs: dedupe(std::mem::take(&mut pending.codecs)),
            fields: match body {
                Some(open) => self.fields(open)?,
                None => Vec::new(),
            },
        };

        Ok(Some(model))
    }

    /// Reads the annotated fields out of a struct body.
    ///
    /// `open` is the index of the body's `{`; the cursor is left past its `}`.
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

            // A field is `name : type`, after any modifiers. Anything else in a
            // struct body is skipped to the next comma.
            // A field is `name :`, and `name ::` is a path in somebody's default
            // expression.
            let name = token.ident();
            let is_field = name.is_some()
                && self.tokens.get(self.at + 1).is_some_and(|next| next.kind == Kind::Punct(':'))
                && !self.tokens.get(self.at + 2).is_some_and(|after| after.kind == Kind::Punct(':'));

            if !is_field {
                self.bump();
                if token.kind == Kind::Punct(',') {
                    pending.clear();
                }
                continue;
            }

            let name = name.expect("checked above");
            // The attribute's line, not the field's: an error about `#[network]`
            // should point at the `#[network]` the user wrote.
            let line = pending.line;
            self.at += 2;
            self.skip_field_type(close);

            match pending.network.take() {
                // §18 — the one syntax error worth reporting: the generator was
                // told this field is on the wire but not what to write for it.
                Some(None) => {
                    return Err(self.error(line, "#[network] field requires a network type"));
                }
                Some(Some(network_type)) => fields.push(Field {
                    name: name.to_owned(),
                    network_type,
                    codecs: dedupe(std::mem::take(&mut pending.codecs)),
                }),
                // No `#[network(...)]`: not a network field, and not an error.
                None => {}
            }

            pending.clear();
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

    fn error(&self, line: usize, message: &str) -> Error {
        Error { path: self.path.to_path_buf(), line, message: message.to_owned() }
    }

    /// Steps over a struct's generics and where-clause.
    ///
    /// Returns the index of its body's `{`, or `None` for a struct that ends in
    /// `;` and so has no named fields.
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

    /// Steps over a field's Rust type without reading it.
    ///
    /// The type is never inspected — `#[network(TYPE)]` already said what goes
    /// on the wire — so this only has to find where the field ends.
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
                    // The `>` of `->` closes nothing.
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

/// Joins an attribute's argument tokens back into the text the user wrote.
///
/// `Array < u32 >` becomes `Array<u32>`. The result is handed to the generator
/// as a name, not as something to analyse.
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

/// Drops repeats, keeping the order the source wrote.
///
/// A codec named twice would otherwise produce the same generated type twice,
/// which is a guaranteed compile error rather than a schema question — so it is
/// removed here instead of reported.
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

/// Keywords that start an item, and so end a run of attributes.
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
    Punct(char),
    /// A literal. Never meaningful here, only stepped over.
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

/// Splits source into tokens, borrowing every identifier rather than copying it.
///
/// Comments and literals are dropped, which is the whole point: a `#[` inside a
/// string and a `struct` inside a comment are not source, and a scanner that
/// could not tell would be a search-and-replace with extra steps.
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

        // `//` to end of line.
        if byte == b'/' && bytes.get(at + 1) == Some(&b'/') {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }

        // `/* */`, which nests in Rust.
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

        // `r"…"`, `r#"…"#`, `br#"…"#` — the hash count closes it.
        if let Some(next) = raw_string(bytes, at) {
            line += count_newlines(&bytes[at..next]);
            at = next;
            tokens.push(Token { kind: Kind::Other, line });
            continue;
        }

        // `"…"`, and `b"…"`.
        let quote = at + usize::from(byte == b'b' && bytes.get(at + 1) == Some(&b'"'));
        if bytes.get(quote) == Some(&b'"') {
            let start = at;
            at = quote + 1;
            while at < bytes.len() && bytes[at] != b'"' {
                at += if bytes[at] == b'\\' { 2 } else { 1 };
            }
            at = (at + 1).min(bytes.len());
            line += count_newlines(&bytes[start..at]);
            tokens.push(Token { kind: Kind::Other, line });
            continue;
        }

        // `'c'` is a literal; `'a` is a lifetime, and its `'` is just punctuation.
        if byte == b'\'' {
            if let Some(next) = char_literal(bytes, at) {
                at = next;
                tokens.push(Token { kind: Kind::Other, line });
                continue;
            }
            tokens.push(Token { kind: Kind::Punct('\''), line });
            at += 1;
            continue;
        }

        if is_ident_start(byte) {
            let start = at;
            while at < bytes.len() && is_ident_continue(bytes[at]) {
                at += 1;
            }
            // `r#type` is an identifier whose `r#` is not part of the name.
            let name = &text[start..at];
            let name = name.strip_prefix("r#").unwrap_or(name);
            tokens.push(Token { kind: Kind::Ident(name), line });
            continue;
        }

        if byte.is_ascii_digit() {
            while at < bytes.len() && (is_ident_continue(bytes[at]) || bytes[at] == b'.') {
                at += 1;
            }
            tokens.push(Token { kind: Kind::Other, line });
            continue;
        }

        tokens.push(Token { kind: Kind::Punct(byte as char), line });
        at += 1;
    }

    tokens
}

/// Matches a raw string at `at`, returning the index just past it.
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

    // The literal ends at the first `"` followed by as many `#` as opened it.
    while cursor < bytes.len() {
        if bytes[cursor] == b'"' && bytes[cursor + 1..].iter().take(hashes).all(|b| *b == b'#') {
            return Some((cursor + 1 + hashes).min(bytes.len()));
        }
        cursor += 1;
    }

    Some(bytes.len())
}

/// Matches a character literal at `at`, returning the index just past it.
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

    // A lifetime looks the same up to here; only the closing quote separates them.
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
