//! What the parser collected, and nothing more.
//!
//! This is not an IR. There is no schema here, no type graph, no codec graph, no
//! dependency graph — only the five things the generator needs to write a call:
//! which source language the model came from, its name, which codecs it
//! declares, and for each field its name, its network type and which codecs it
//! belongs to.
//!
//! Everything else about the source is dropped on the way in. The host
//! language's type of a field is never recorded, because the generator never
//! asks: `#[network(TYPE)]` (or `[Network("TYPE")]`) is the answer, and whether
//! the two agree is the host compiler's question.
//!
//! # Rust and C# produce the same shape
//!
//! `#[network(u32)]` and `[Network("u32")]` are two spellings of one fact: this
//! field is a Cyclone `u32`. Both parsers resolve to the identical
//! [`Field::network_type`] string — the Cyclone Specification's own identifier,
//! never a host-language type name — so the generator that reads this module
//! cannot tell, and does not need to, which syntax a model was written in. The
//! only thing [`Model::language`] is for is choosing *which* generator backend
//! renders the model, not what it renders.

/// Which source syntax a [`Model`] was read from.
///
/// This selects a generator backend and an output file extension. It carries no
/// schema information: a model parsed from C# and an equivalent one parsed from
/// Rust hold the same [`Field::network_type`] strings and produce the same
/// bytes on the wire, per the Cyclone Specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// Read from `#[network]` / `#[codec(...)]` in a `.rs` file.
    Rust,
    /// Read from `[Network]` / `[Codec(...)]` in a `.cs` file.
    CSharp,
}

/// A struct (or, in C#, a struct or class) marked as a Cyclone network model.
pub struct Model {
    /// Which source syntax this model was read from.
    pub language: Language,
    /// The type name, as the source spells it.
    pub name: String,
    /// The codecs the model declares, in the order written.
    ///
    /// These, and only these, are generated. A field naming a codec the model
    /// did not declare cannot conjure one into existence.
    pub codecs: Vec<String>,
    /// The annotated fields, in declaration order.
    pub fields: Vec<Field>,
}

/// A field marked `#[network(TYPE)]` (Rust) or `[Network("TYPE")]` (C#).
pub struct Field {
    /// The field name, used to reach the value.
    pub name: String,
    /// The Cyclone network type, exactly as the annotation spelled it.
    ///
    /// Either a primitive the generator knows a method for, or a name it treats
    /// as another model. Nothing here decides which — [`crate::generator`] does,
    /// with a table lookup and no analysis.
    pub network_type: String,
    /// The codecs this field belongs to, in the order written.
    ///
    /// A field with none belongs to none, and is written by no codec.
    pub codecs: Vec<String>,
}

impl Model {
    /// The fields belonging to `codec`, in declaration order.
    ///
    /// Declaration order is the whole of the ordering rule: a codec writes its
    /// fields in the order the struct declares them, skipping the ones that did
    /// not name it.
    pub fn fields_in<'a>(&'a self, codec: &'a str) -> impl Iterator<Item = &'a Field> {
        self.fields
            .iter()
            .filter(move |field| field.codecs.iter().any(|name| name == codec))
    }
}

/// Turns a codec identifier into the PascalCase fragment of a generated name.
///
/// `edge` becomes `Edge`, `orange_pi` becomes `OrangePi`, `custom_a` becomes
/// `CustomA`. That is the entire meaning a codec name has here: the generator
/// never resolves it, imports it, or looks it up — it spells a type name with it
/// and moves on.
pub fn pascal_case(identifier: &str) -> String {
    let mut out = String::with_capacity(identifier.len());
    let mut capitalise = true;

    for character in identifier.chars() {
        if character == '_' {
            capitalise = true;
            continue;
        }
        if capitalise {
            out.extend(character.to_uppercase());
            capitalise = false;
        } else {
            out.push(character);
        }
    }

    out
}
