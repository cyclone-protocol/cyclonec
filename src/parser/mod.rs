//! Source → [`Model`]s, one scanner per source language.
//!
//! **Neither scanner is a parser for its language.** Neither knows types,
//! traits, generics, namespaces, or any other semantic of Rust or C# — the host
//! compiler already does, and running a second copy of it to find four markers
//! would be the slowest possible way to answer the smallest possible question.
//!
//! Both look for exactly this much, in their own syntax:
//!
//! ```text
//!                              Rust                    C#
//! this is a model              #[network]               [Network]
//! generate these codecs        #[codec(a, b)]            [Codec("a", "b")]
//! this field's wire type       #[network(TYPE)]          [Network("TYPE")]
//! this field's codecs          #[codec(a, b)]             [Codec("a", "b")]
//! ```
//!
//! Everything else in a file is tokens to step over. A field's host-language
//! type is skipped without being read, because the annotation already said what
//! goes on the wire — see [`crate::model`] for why that is not a detail, but the
//! whole point.
//!
//! The one thing each scanner must get right is *where a token is*: a `#[` or
//! `[` inside a string, or `struct` inside a comment, must not be mistaken for
//! source. That is what each module's lexer is for, and it is the only reason
//! this is a scanner and not a substring search.

pub mod csharp;
pub mod rust;

use std::path::{Path, PathBuf};

use crate::model::Model;

/// Anything that stops the generator, with the file and line it happened in.
#[derive(Debug)]
pub struct Error {
    /// The file being read.
    pub path: PathBuf,
    /// The line it happened on.
    pub line: usize,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.path.display(), self.line, self.message)
    }
}

/// Extracts every network model from `text`, choosing a scanner by `path`'s
/// extension: `.cs` reads C#, anything else reads Rust.
///
/// `path` is carried for error messages only; nothing reads it to decide
/// content — the extension check is the one exception, and it exists only to
/// pick which of the two scanners below runs.
///
/// # Errors
///
/// Only what stops generation: an annotation the generator needs and cannot
/// read. Source that does not compile for any other reason is the host
/// compiler's to report, and passes through here without comment.
pub fn parse(path: &Path, text: &str) -> Result<Vec<Model>, Error> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("cs") => csharp::parse(path, text),
        _ => rust::parse(path, text),
    }
}
