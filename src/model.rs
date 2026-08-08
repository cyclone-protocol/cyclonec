//! What the parser collected, and nothing more.
//!
//! This is not an IR. There is no schema here, no type graph, no codec graph, no
//! dependency graph - only the five things the generator needs to write a call:
//! which source language the model came from, its name, which codecs it
//! declares, and for each field its name, its network type and which codecs it
//! belongs to.
//!
//! Everything else about the source is dropped on the way in. The host
//! language's type of a field is never recorded, because the generator never
//! asks: `#[network(TYPE)]`, `[Network("TYPE")]`, or `` `cyclone:"TYPE"` `` is
//! the answer, and whether the two agree is the host compiler's question.
//!
//! # Rust, C# and Go produce the same shape
//!
//! `#[network(u32)]`, `[Network("u32")]`, and `` `cyclone:"u32"` `` are three
//! spellings of one fact: this field is a Cyclone `u32`. All three scanners
//! resolve to the identical [`Field::network_type`] string - the Cyclone
//! Specification's own identifier, never a host-language type name - so the
//! generator that reads this module cannot tell, and does not need to, which
//! syntax a model was written in. The only thing [`Model::language`] is for is
//! choosing *which* generator backend renders the model, not what it renders.

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
    /// Read from a `//cyclone:model` comment directive and `cyclone:"..."` /
    /// `codec:"..."` struct tags in a `.go` file.
    Go,
    /// Read from `# cyclone:model` / `# cyclone:TYPE` comment directives in a
    /// `.gd` file. GDScript has no attribute syntax the compiler recognizes for
    /// arbitrary user metadata (an unrecognized `@` annotation is a parse
    /// error), so - like Go - Cyclone metadata lives in a comment.
    GDScript,
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
    /// as another model. Nothing here decides which - [`crate::generator`] does,
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

/// Checks that every nested-model field routes only into codecs the
/// *referenced* model actually declares.
///
/// A field whose Cyclone type names another model - `#[network(PlayerInfo)]`,
/// `[Network("PlayerInfo")]`, `` `cyclone:"PlayerInfo"` `` - carries its own
/// codec list over to the nested call: routing `Player.info` into `edge` means
/// the generated `PlayerEdgeCodec` calls `PlayerInfoEdgeCodec`. If `PlayerInfo`
/// itself never declared an `edge` codec, that call names a type nothing will
/// ever generate - a dangling reference the three backends have no way to
/// notice on their own, because each renders one model at a time and never
/// looks at another model's own codec list.
///
/// This is the one thing generation itself must catch before writing anything:
/// unlike a Cyclone type this run never parsed at all (a hand-written codec, a
/// type from another package), which is rightly left for the host compiler to
/// resolve or reject, a model *this run did parse* has already told us exactly
/// which codecs it has. Silently emitting the call anyway would defer a
/// knowable error to a confusing one, in whichever host compiler hits it
/// first - and design a validation that requires nothing this crate doesn't
/// already have: the check is pure comparison of the metadata already
/// collected, run once per language before that language's models are handed
/// to its generator.
///
/// # Errors
///
/// The first dangling reference found, naming the model, the field, the codec,
/// and what the referenced model declares instead.
pub fn check_nested_codecs(models: &[Model]) -> Result<(), String> {
    for model in models {
        for codec in &model.codecs {
            for field in model.fields_in(codec) {
                let Some(referenced) =
                    models.iter().find(|candidate| candidate.name == field.network_type)
                else {
                    // Not a model this run parsed - a primitive, or a type
                    // defined elsewhere. Left to the host compiler, same as
                    // ever: see h.md's own "let native compiler resolve
                    // missing symbols" rule.
                    continue;
                };

                if referenced.codecs.iter().any(|declared| declared == codec) {
                    continue;
                }

                let declares = if referenced.codecs.is_empty() {
                    "no codecs".to_owned()
                } else {
                    format!("only: {}", referenced.codecs.join(", "))
                };
                return Err(format!(
                    "model '{}' field '{}' routes into codec '{codec}', but the model it \
                     references, '{}', declares {declares} - '{}{}Codec' would never be \
                     generated",
                    model.name,
                    field.name,
                    referenced.name,
                    referenced.name,
                    pascal_case(codec),
                ));
            }
        }
    }

    Ok(())
}

/// Turns a codec identifier into the PascalCase fragment of a generated name.
///
/// `edge` becomes `Edge`, `orange_pi` becomes `OrangePi`, `custom_a` becomes
/// `CustomA`. That is the entire meaning a codec name has here: the generator
/// never resolves it, imports it, or looks it up - it spells a type name with it
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
