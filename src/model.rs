use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
    pub source: PathBuf,
    pub line: usize,
    pub codecs: Vec<String>,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub network_type: String,
    pub codecs: Vec<String>,
    pub line: usize,
}

impl Model {
    pub fn fields_in<'a>(&'a self, codec: &'a str) -> impl Iterator<Item = &'a Field> {
        self.fields
            .iter()
            .filter(move |field| field.codecs.iter().any(|name| name == codec))
    }
}

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

pub fn snake_case(identifier: &str) -> String {
    screaming_snake_case(identifier).to_lowercase()
}

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
