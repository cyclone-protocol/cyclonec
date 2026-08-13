//! C++ source → [`Model`]s.
//!
//! Reads `CYCLONE_MODEL` / `CYCLONE_CODEC(...)` / `CYCLONE_FIELD(TYPE)` - three
//! macros a project defines for itself in a small header with no dependency of
//! its own (the same role `cyclone-attributes` plays for Rust and the
//! `Cyclone.Network` / `Cyclone.Codec` pair plays for C#), each expanding to
//! nothing so an annotated struct compiles unchanged on any C++ toolchain
//! whether or not `cyclonec` ever runs over it:
//!
//! ```text
//! CYCLONE_MODEL
//! CYCLONE_CODEC("edge", "unity")
//! struct DeviceState
//! {
//!     CYCLONE_FIELD(u32)
//!     CYCLONE_CODEC("edge", "unity")
//!     uint32_t Id;
//! };
//! ```
//!
//! Unlike C#'s `[Network]` / `[Codec(...)]`, these are not wrapped in
//! `[...]` - they are ordinary tokens, exactly the shape a real macro
//! invocation has to be, and the scanner below reads them as such rather than
//! as a bracketed attribute section. See [`crate::parser`] for what this
//! scanner is and is not - it never looks at a field's C++ type, only at the
//! type name `CYCLONE_FIELD(...)` itself spelled.
//!
//! # Scope
//!
//! A model is a top-level `struct` or `class`; nested types are not specially
//! recognised (the same non-handling the Rust and C# scanners have for a model
//! nested inside `mod` / another type). A `namespace` is stepped over like any
//! other braces - unlike Go's `package` or C#'s `namespace`, a C++
//! `namespace` may legally nest and may legally repeat across a file, so
//! [`namespace_name`] reads only the *first* one, the same simplification
//! [`super::csharp::namespace_name`] already makes for C#'s "first `namespace`
//! in the file" and documents there for the same reason: the common case is
//! one namespace per model file, and a project whose layout is not that shape
//! has `--model-path` to say so explicitly.
//!
//! # What C++ needs that C# does not
//!
//! A C++ member may sit behind an `public:` / `private:` / `protected:`
//! access specifier, which is not itself a declaration and must not be read as
//! one - the scanner steps over `identifier :` at the top of a struct body
//! before ever asking what kind of member follows. And a field's host type may
//! be a template (`std::vector<uint32_t>`) or a C array (`uint32_t[4]`) - both
//! are stepped over by the same bracket-depth walk [`super::csharp`] already
//! uses for C#'s generics, since `<...>` and `[...]` need the identical
//! "count the nesting, do not stop at the first closer" treatment.

use std::path::Path;

use crate::model::{Field, Model};
use crate::parser::Error;

/// Extracts every `CYCLONE_MODEL` model from `text`.
///
/// # Errors
///
/// Only what stops generation: a `CYCLONE_FIELD()` with no wire type inside
/// its parentheses, or a `CYCLONE_FIELD` / `CYCLONE_CODEC` marker sitting on a
/// method instead of a field. Source that does not compile for any other
/// reason is the C++ compiler's to report.
pub fn parse(path: &Path, text: &str) -> Result<Vec<Model>, Error> {
    let tokens = lex(text);
    Scanner {
        path,
        tokens: &tokens,
        at: 0,
    }
    .file()
}

/// The `namespace` a C++ source file opens, if any - the first one, however
/// deeply the file goes on to nest others. See the module docs for why only
/// the first is read.
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

// ============================================================== the scanner

struct Scanner<'a> {
    path: &'a Path,
    tokens: &'a [Token<'a>],
    at: usize,
}

/// The Cyclone markers collected so far, waiting for the declaration they
/// precede.
#[derive(Default)]
struct Pending {
    /// Whether a `CYCLONE_MODEL` has been seen (top-level scanning only).
    model: bool,
    /// `CYCLONE_FIELD(TYPE)`, and the type if its parentheses were not empty
    /// (member scanning only).
    field_type: Option<Option<String>>,
    /// Every string from every `CYCLONE_CODEC(...)`, in order.
    codecs: Vec<String>,
    /// The line the first Cyclone marker was on. `0` means "none yet" - every
    /// real line number is at least `1`.
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
    /// Walks a file, collecting models declared at the top level.
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

                // A keyword that starts a different kind of declaration ends
                // the run of markers, so a `CYCLONE_MODEL` on an enum is not
                // inherited by the next struct.
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

    /// Reads a `CYCLONE_CODEC(...)` invocation, adding its codec names to
    /// `pending`. Not followed by `(` at all - a definition this scanner does
    /// not need to understand - is left alone rather than treated as ours.
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

    /// Reads a `CYCLONE_FIELD(TYPE)` invocation into `pending.field_type`.
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

    /// Reads a `struct` or `class` declaration, returning it if it is a
    /// model.
    fn type_declaration(&mut self, pending: &mut Pending) -> Result<Option<Model>, Error> {
        let Some(name) = self.peek().and_then(Token::ident) else {
            return Ok(None);
        };
        self.bump();

        // A type nothing marks is somebody else's. It must not become an
        // error just because it shares a file with a model.
        let is_model = pending.model;

        // A base-clause (`: public Base`) and any template parameters are
        // stepped over without being read.
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

    /// Reads the annotated members out of a `struct` or `class` body.
    ///
    /// `open` is the index of the body's `{`; the cursor is left past its
    /// `}`.
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
            // `public:` / `private:` / `protected:` is not a declaration -
            // stepping over it here keeps it from being fed to
            // `declaration_end`, which would otherwise read the field right
            // after it as if the specifier were the field's own host type.
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
                            // Told the field is on the wire, not told what to
                            // write for it.
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
                            // `CYCLONE_CODEC(...)` with no `CYCLONE_FIELD(...)`
                            // names a codec for a field the generator does not
                            // know is on the wire at all.
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
                // Defensive: every other arm above is guaranteed to advance
                // `self.at`. This one is reached on debris `declaration_end`
                // could not name a member from - guarantee progress anyway.
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

    /// Scans a declaration from the current position, stopping at the token
    /// that reveals what kind of member it is.
    ///
    /// `(` at depth `0` is always read as a method or constructor starting -
    /// a plain field's C++ host type cannot itself contain one, the same
    /// simplification [`super::csharp::Scanner::declaration_end`] makes for
    /// C#, where it is exact rather than a simplification at all.
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

    /// Skips from a member's name to just past the end of its declaration:
    /// past `;`, or past `= expression ;`, or past a brace initializer
    /// (`{0}`) and any trailing `;`.
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

    // ------------------------------------------------------------- movement

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

    /// Steps over a type's base-clause and any template parameters.
    ///
    /// Returns the index of its body's `{`, or `None` for a `;`-terminated
    /// forward declaration, which has no members to read.
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

/// What a member declaration turned out to be, once the scanner reached the
/// token that reveals it.
enum DeclarationEnd {
    /// A method or constructor - `(` at depth 0.
    Method,
    /// A field, named by the identifier at `name_index`.
    Member { name_index: usize },
    /// The body ended before a member could be identified.
    EndOfBody,
}

/// Whether `token` is `public` / `private` / `protected` immediately followed
/// by `:` - an access specifier, not a declaration.
fn is_access_specifier(token: Token<'_>, next: Option<&Token<'_>>) -> bool {
    matches!(
        token.kind,
        Kind::Ident("public") | Kind::Ident("private") | Kind::Ident("protected")
    ) && next.is_some_and(|token| token.kind == Kind::Punct(':'))
}

/// The string content of a token sequence that is exactly one string literal,
/// ignoring surrounding whitespace-equivalent tokens there are none of.
fn string_literal(tokens: &[Token<'_>]) -> Option<String> {
    match tokens {
        [Token {
            kind: Kind::Str(text),
            ..
        }] => Some((*text).to_owned()),
        _ => None,
    }
}

/// Joins a `CYCLONE_FIELD(...)` argument's tokens back into the text the user
/// wrote: `Array < u32 >` becomes `Array<u32>`. The result is handed to the IR
/// as a name to resolve, not as something this module analyses.
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

/// Splits a token slice on commas that are not inside brackets.
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

/// Drops repeats, keeping the order the source wrote. See the identical
/// helper in [`crate::parser::rust`].
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

/// Keywords that start a declaration `CYCLONE_MODEL`/`CYCLONE_CODEC` cannot
/// attach to, and so end a run of pending markers.
fn is_boundary_keyword(word: &str) -> bool {
    matches!(
        word,
        "namespace" | "enum" | "union" | "typedef" | "using" | "template" | "extern"
    )
}

// ================================================================= the lexer

/// One token, borrowed from the source.
#[derive(Debug, Clone, Copy)]
struct Token<'a> {
    kind: Kind<'a>,
    line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind<'a> {
    Ident(&'a str),
    /// A string literal's contents - decoded enough to read a codec name,
    /// which is the only thing a string ever holds here.
    Str(&'a str),
    Punct(char),
    /// A numeric or character literal. Never meaningful here, only skipped.
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

/// Splits C++ source into tokens.
///
/// Comments, whitespace and preprocessor directives are dropped: none of them
/// can carry a Cyclone marker, and all of them can carry a `{` or `(` that
/// would otherwise be counted. `#if` blocks are not evaluated - a model behind
/// one is read either way, which is safer than guessing which branch a build
/// takes.
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

        // A preprocessor directive runs to end of line - `#include`,
        // `#pragma once`, `#define`, `#ifdef`, and so on. `CYCLONE_MODEL`
        // itself never starts with `#`: it is an ordinary macro
        // *invocation*, indistinguishable in the token stream from any other
        // identifier until this scanner recognises its spelling.
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

/// Matches a C++11 raw string literal `R"delim(...)delim"` at `at`, returning
/// the index just past it. `None` if `at` is not the start of one - in
/// particular, an ordinary identifier that happens to start with `R` and is
/// not immediately followed by `"` is left alone.
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
