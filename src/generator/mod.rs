//! [`Model`](crate::model::Model)s → source, one backend per target language.
//!
//! All three backends are single-pass, table-driven renderers with the same
//! shape:
//!
//! ```text
//! Model  →  Codec  →  Field  →  statement
//! ```
//!
//! [`rust`], [`csharp`] and [`go`] are independent - none imports another -
//! but they read the identical [`crate::model::Model`], so a schema means the
//! same thing to all three: same codec names, same field routing, same wire
//! bytes. What differs is only which method name and which syntax gets the
//! bytes there. [`rust::render`]'s header documents the shared rules once;
//! [`csharp`]'s and [`go`]'s headers document where each language forces a
//! different shape and why.
//!
//! No backend writes byte layout, endianness or string encoding of its own:
//! [`rust_runtime`], [`csharp_runtime`] and [`go_runtime`] are the three
//! implementations of that, each a fixed constant written once against
//! RFC-0002 and carried verbatim into every file its matching backend renders.

pub mod csharp;
pub mod csharp_runtime;
pub mod gdscript;
pub mod gdscript_runtime;
pub mod go;
pub mod go_runtime;
pub mod rust;
pub mod rust_runtime;
