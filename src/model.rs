//! What the scanner collected, and nothing more.
//!
//! This is deliberately *not* the IR. It is the raw reading of one source file:
//! which type is a model, which codecs it declared, and for each annotated
//! field its name, the network type the annotation spelled, and which codecs it
//! joined. Nothing is resolved, checked or canonicalised here.
//!
//! [`crate::ir`] is the next step and the source of truth: it takes these,
//! resolves each `network_type` string into a [`crate::ir::WireType`], checks
//! the schema as a whole, and computes fingerprints. Everything downstream -
//! generated code, `schema.json`, the build graph - is derived from the IR and
//! never from this.
//!
//! The host language's type of a field is never recorded, because nothing ever
//! asks: `#[network(TYPE)]` is the answer, and whether the two agree is the host
//! compiler's question.

use std::path::PathBuf;

/// A struct marked as a Cyclone network model.
#[derive(Debug, Clone)]
pub struct Model {
    /// The type name, as the source spells it.
    pub name: String,
    /// The file it was read from, as given on the command line.
    pub source: PathBuf,
    /// The line its `#[network]` was on, for error messages.
    pub line: usize,
    /// The codecs the model declares, in the order written.
    ///
    /// These, and only these, are generated. A field naming a codec the model
    /// did not declare cannot conjure one into existence.
    pub codecs: Vec<String>,
    /// The annotated fields, in declaration order.
    pub fields: Vec<Field>,
}

/// A field marked `#[network(TYPE)]`.
#[derive(Debug, Clone)]
pub struct Field {
    /// The field name, used to reach the value.
    pub name: String,
    /// The Cyclone network type, exactly as the annotation spelled it.
    pub network_type: String,
    /// The codecs this field belongs to, in the order written.
    ///
    /// A field with none belongs to none, and is written by no codec.
    pub codecs: Vec<String>,
    /// The line the field's `#[network(...)]` was on.
    pub line: usize,
}

impl Model {
    /// The fields belonging to `codec`, in declaration order.
    ///
    /// Declaration order is the whole of the ordering rule (RFC-0002 §5.1): a
    /// codec writes its fields in the order the struct declares them, skipping
    /// the ones that did not name it.
    pub fn fields_in<'a>(&'a self, codec: &'a str) -> impl Iterator<Item = &'a Field> {
        self.fields
            .iter()
            .filter(move |field| field.codecs.iter().any(|name| name == codec))
    }
}

/// Turns a codec identifier into the PascalCase fragment of a generated name.
///
/// `edge` becomes `Edge`, `orange_pi` becomes `OrangePi`. That is the entire
/// meaning a codec name has: the generator never resolves it, imports it, or
/// looks it up - it spells a type name with it and moves on.
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

/// Turns an identifier into the snake_case fragment of a generated file name:
/// `PlayerInfo` becomes `player_info`, `orange_pi` stays `orange_pi`.
pub fn snake_case(identifier: &str) -> String {
    screaming_snake_case(identifier).to_lowercase()
}

/// Turns an identifier into the SCREAMING_SNAKE_CASE fragment of a generated
/// constant name: `PlayerInfo` becomes `PLAYER_INFO`, `orange_pi` stays
/// `ORANGE_PI`.
pub fn screaming_snake_case(identifier: &str) -> String {
    let mut out = String::with_capacity(identifier.len() + 4);
    let mut previous_was_lower = false;

    for character in identifier.chars() {
        if character == '_' {
            out.push('_');
            previous_was_lower = false;
            continue;
        }
        if character.is_uppercase() && previous_was_lower {
            out.push('_');
        }
        previous_was_lower = character.is_lowercase() || character.is_numeric();
        out.extend(character.to_uppercase());
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{pascal_case, screaming_snake_case};

    #[test]
    fn pascal_case_spells_generated_type_names() {
        assert_eq!(pascal_case("edge"), "Edge");
        assert_eq!(pascal_case("orange_pi"), "OrangePi");
        assert_eq!(pascal_case("custom_a"), "CustomA");
    }

    #[test]
    fn screaming_snake_case_spells_generated_constant_names() {
        assert_eq!(screaming_snake_case("Player"), "PLAYER");
        assert_eq!(screaming_snake_case("PlayerInfo"), "PLAYER_INFO");
        assert_eq!(screaming_snake_case("orange_pi"), "ORANGE_PI");
        assert_eq!(screaming_snake_case("HTTPServer"), "HTTPSERVER");
    }

    #[test]
    fn snake_case_spells_generated_file_names() {
        assert_eq!(super::snake_case("Player"), "player");
        assert_eq!(super::snake_case("PlayerInfo"), "player_info");
        assert_eq!(super::snake_case("orange_pi"), "orange_pi");
    }
}
