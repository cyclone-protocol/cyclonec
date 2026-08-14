//! The official Cyclone **source generator**.
//!
//! ```text
//! source annotation
//!       ↓
//! scanner / parser
//!       ↓
//! model discovery
//!       ↓
//! codec generation
//! ```
//!
//! `cyclonec` is not a compiler and not a runtime. It reads Cyclone attributes
//! out of your sources (Rust's `#[network]` / `#[codec(...)]`, Go's
//! `//cyclone:model` directive and `cyclone:"..."` / `codec:"..."` struct
//! tags, C#'s `[Network]` / `[Codec(...)]` attributes, GDScript's
//! `# cyclone:model` / `# cyclone:TYPE` comment directives - GDScript has no
//! attribute syntax a user can extend - C++/C's shared `CYCLONE_MODEL` /
//! `CYCLONE_CODEC(...)` / `CYCLONE_FIELD(TYPE)` macros, which expand to
//! nothing, or TypeScript/JavaScript's `// CYCLONE_MODEL` /
//! `// CYCLONE_CODEC(...)` / `// CYCLONE_FIELD(TYPE)` comment directives -
//! neither language has anything usable without a decorator or a runtime
//! dependency, which the brief forbids) and writes the `encode` / `decode`
//! calls that go with them, then exits, the way `protoc` does.
//!
//! What it writes reads and writes **your** types. There is no DTO, no wire
//! struct, no mapper, no registry and no reflection: `encode` takes a
//! `&Player` (Go: `*Player`, C#: `Player`, GDScript: `Player`, C++:
//! `const Player&`, C: `const Player *`, TypeScript/JavaScript: `Player`),
//! `decode` takes a `&mut Player` (Go: `*Player`, returning `error`; C#:
//! `ref Player`, throwing on failure; GDScript: `Player`, returning a
//! `DecodeError` or `null`; C++: `Player&`, returning a `DecodeError`; C:
//! `Player *`, returning a `CycloneDecodeError`; TypeScript/JavaScript:
//! `Player`, throwing a `DecodeError`), and the bytes in between are
//! RFC-0002's, produced by a runtime block that is copied out unchanged
//! rather than derived per model.
//!
//! Rust, Go, C#, GDScript, C++, C, TypeScript and JavaScript are read by
//! independent scanners into the identical [`model::Model`] shape, so a
//! schema written in any of them produces the same codec names, the same
//! field routing, and the same bytes on the wire - see [`parser`] and
//! [`generator`].
//!
//! # What this version adds
//!
//! `cyclonec_old` stopped at the codecs. This one also derives, from the same
//! single pass over your source:
//!
//! - [`ir`] - the Cyclone IR, the source of truth for everything below;
//! - [`schema`] - `.cyclone/schema.json`, a build artifact and a project
//!   contract, never a runtime dependency;
//! - [`fingerprint`] - a deterministic, cross-SDK digest of each wire contract;
//! - [`compat`] - what changed between two schemas, and whether it breaks;
//! - [`buildgraph`] - which source produced which file;
//! - [`generator::handshake`] - the fingerprint constants a peer compares at
//!   connect time.
//!
//! The order matters: the IR is derived from source on every run, and
//! everything else is derived from the IR. An existing `schema.json` is never
//! read to decide what to generate - only to say what changed since.

pub mod buildgraph;
pub mod cli;
pub mod compat;
pub mod config;
pub mod fingerprint;
pub mod generate;
pub mod generator;
pub mod gomod;
pub mod inspect;
pub mod ir;
pub mod json;
pub mod model;
pub mod parser;
pub mod schema;
pub mod sha256;
pub mod timestamp;
