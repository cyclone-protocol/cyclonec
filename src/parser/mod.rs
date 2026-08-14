//! Source → [`Model`]s.
//!
//! **This is not a parser for Rust.** It knows no types, traits, generics or
//! modules - `rustc` already does, and running a second copy of it to find four
//! markers would be the slowest possible way to answer the smallest possible
//! question. It looks for exactly this much:
//!
//! ```text
//! this is a model            #[network]
//! generate these codecs      #[codec(a, b)]
//! this field's wire type     #[network(TYPE)]
//! this field's codecs        #[codec(a, b)]
//! ```
//!
//! Everything else in a file is tokens to step over. A field's host-language
//! type is skipped without being read, because the annotation already said what
//! goes on the wire.
//!
//! The one thing the scanner must get right is *where a token is*: a `#[` inside
//! a string, or `struct` inside a comment, must not be mistaken for source. That
//! is what [`rust`]'s lexer is for, and it is the only reason this is a scanner
//! and not a substring search.
//!
//! Rust, Go, C#, GDScript, C++, C and TypeScript/JavaScript are read today,
//! by seven independent scanners ([`rust`], [`go`], [`csharp`], [`gdscript`],
//! [`cpp`], [`c`], [`typescript`]) into the identical [`Model`] shape - and
//! nothing downstream of [`crate::ir`] cares which one produced it, because
//! the IR is where a schema stops being source and starts being a schema. A
//! further language is a further module here, dispatched on its extension in
//! [`parse`] below. [`typescript`] alone covers two extensions (`.ts` and
//! `.js`): TypeScript and JavaScript share one Cyclone annotation concept, so
//! one scanner reads both.

pub mod c;
pub mod cpp;
pub mod csharp;
pub mod gdscript;
pub mod go;
pub mod rust;
pub mod typescript;

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

/// Extracts every network model from `text`.
///
/// `path` picks the scanner - `.go` reads Go, `.cs` reads C#, `.gd` reads
/// GDScript, `.hpp`/`.cpp`/`.cc`/`.cxx` reads C++, `.c`/`.h` reads C,
/// `.ts`/`.js` reads TypeScript/JavaScript, everything else reads Rust - and
/// is carried into the models (for `schema.json` and the build graph) and
/// into error messages; nothing else about it decides content.
///
/// `.h` reads as C, not C++: the two share no other extension, and a C
/// project's models live in headers as often as not, so a C++ project's
/// headers are expected to use `.hpp` instead.
///
/// # Errors
///
/// Only what stops generation: an annotation the generator needs and cannot
/// read. Source that does not compile for any other reason is the host
/// compiler's to report, and passes through here without comment.
pub fn parse(path: &Path, text: &str) -> Result<Vec<Model>, Error> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("go") => go::parse(path, text),
        Some("cs") => csharp::parse(path, text),
        Some("gd") => gdscript::parse(path, text),
        Some("hpp") | Some("cpp") | Some("cc") | Some("cxx") => cpp::parse(path, text),
        Some("c") | Some("h") => c::parse(path, text),
        Some("ts") | Some("js") => typescript::parse(path, text),
        _ => rust::parse(path, text),
    }
}
