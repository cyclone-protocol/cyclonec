//! [`Model`](crate::model::Model)s → source, one backend per target language.
//!
//! Both backends are single-pass, table-driven renderers with the same shape:
//!
//! ```text
//! Model  →  Codec  →  Field  →  statement
//! ```
//!
//! [`rust`] and [`csharp`] are independent — neither imports the other — but
//! they read the identical [`crate::model::Model`], so a schema means the same
//! thing to both: same codec names, same field routing, same wire bytes. What
//! differs is only which method name and which syntax gets the bytes there.
//! [`rust::render`]'s header documents the shared rules once; [`csharp`]'s
//! header documents where C# forces a different shape and why.
//!
//! Neither backend writes byte layout, endianness or string encoding of its
//! own: [`rust_runtime`] and [`csharp_runtime`] are the two implementations of
//! that, each a fixed constant written once against RFC-0002 and carried
//! verbatim into every file the matching backend renders.

pub mod csharp;
pub mod csharp_runtime;
pub mod rust;
pub mod rust_runtime;
