use crate::ir::{Field, Model, WireType};
use crate::sha256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    pub const ZERO: Fingerprint = Fingerprint([0; 32]);

    pub fn of(canonical: &str) -> Fingerprint {
        Fingerprint(sha256::hash(canonical.as_bytes()))
    }

    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn hex(&self) -> String {
        sha256::hex(&self.0)
    }

    pub fn tagged(&self) -> String {
        format!("sha256:{}", self.hex())
    }

    pub fn u64(&self) -> u64 {
        u64::from_be_bytes([
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5], self.0[6], self.0[7],
        ])
    }

    pub fn parse(text: &str) -> Result<Fingerprint, String> {
        let hex = text
            .strip_prefix("sha256:")
            .ok_or_else(|| format!("`{text}` is not a `sha256:` fingerprint"))?;
        if hex.len() != 64 {
            return Err(format!("`{text}` is {} hex digits, not 64", hex.len()));
        }

        let mut bytes = [0u8; 32];
        for (index, slot) in bytes.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
                .map_err(|_| format!("`{text}` is not hexadecimal"))?;
        }
        Ok(Fingerprint(bytes))
    }
}

const HEADER: &str = "cyclone-fingerprint/2\n";

pub fn canonical_field_name(name: &str) -> String {
    let canonical: String = name
        .chars()
        .filter(|character| !matches!(character, '_' | '-' | ' '))
        .map(|character| character.to_ascii_lowercase())
        .collect();

    if canonical.is_empty() {
        return name.to_owned();
    }
    canonical
}

pub fn message(model: &Model, codec: &str, schema: &[Model]) -> Fingerprint {
    Fingerprint::of(&canonical_message(model, codec, schema))
}

pub fn model(model: &Model, schema: &[Model]) -> Fingerprint {
    Fingerprint::of(&canonical_model(model, schema))
}

pub fn project(models: &[Model]) -> Fingerprint {
    let mut lines: Vec<String> = models
        .iter()
        .flat_map(|model| model.messages.iter())
        .map(|message| format!("message {} {}\n", message.name, message.fingerprint.hex()))
        .collect();
    lines.sort();

    let mut text = String::with_capacity(64 + lines.len() * 96);
    text.push_str(HEADER);
    text.push_str("schema\n");
    for line in lines {
        text.push_str(&line);
    }
    text.push_str("end\n");

    Fingerprint::of(&text)
}

pub fn message_id(name: &str) -> u32 {
    let digest = sha256::hash(format!("cyclone-message-id/1\n{name}\n").as_bytes());
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}

pub fn canonical_message(model: &Model, codec: &str, schema: &[Model]) -> String {
    let mut text = String::with_capacity(256);
    text.push_str(HEADER);
    write_message(&mut text, model, codec, schema, &mut Vec::new());
    text
}

pub fn canonical_model(model: &Model, schema: &[Model]) -> String {
    let mut text = String::with_capacity(256);
    text.push_str(HEADER);
    write_model(&mut text, model, schema, &mut Vec::new());
    text
}

fn write_message(
    text: &mut String,
    model: &Model,
    codec: &str,
    schema: &[Model],
    stack: &mut Vec<String>,
) {
    text.push_str("message ");
    text.push_str(&model.name);
    text.push('.');
    text.push_str(codec);
    text.push('\n');
    for (index, field) in model.fields_in(codec).enumerate() {
        write_field(text, index, field, Some(codec), schema, stack);
    }
    text.push_str("end\n");
}

fn write_model(text: &mut String, model: &Model, schema: &[Model], stack: &mut Vec<String>) {
    text.push_str("model ");
    text.push_str(&model.name);
    text.push('\n');
    for (index, field) in model.fields.iter().enumerate() {
        write_field(text, index, field, None, schema, stack);
    }
    text.push_str("end\n");
}

fn write_field(
    text: &mut String,
    index: usize,
    field: &Field,
    codec: Option<&str>,
    schema: &[Model],
    stack: &mut Vec<String>,
) {
    text.push_str("field ");
    text.push_str(&index.to_string());
    text.push(' ');
    text.push_str(&canonical_field_name(&field.name));
    text.push(' ');
    write_type(text, &field.ty, codec, schema, stack);
    text.push('\n');
}

fn write_type(
    text: &mut String,
    ty: &WireType,
    codec: Option<&str>,
    schema: &[Model],
    stack: &mut Vec<String>,
) {
    match ty {
        WireType::Array(element) => {
            text.push_str("Array<");
            write_type(text, element, codec, schema, stack);
            text.push('>');
        }
        WireType::Model(name) => {
            if stack.iter().any(|seen| seen == name) {
                text.push_str("recursive<");
                text.push_str(name);
                text.push('>');
                return;
            }
            let Some(nested) = schema.iter().find(|model| &model.name == name) else {
                text.push_str("extern<");
                text.push_str(name);
                text.push('>');
                return;
            };

            stack.push(name.clone());
            text.push_str("model<");
            text.push_str(name);
            text.push(':');
            match codec {
                Some(codec) => write_message(text, nested, codec, schema, stack),
                None => write_model(text, nested, schema, stack),
            }
            text.push('>');
            stack.pop();
        }
        primitive => text.push_str(&primitive.spelling()),
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_field_name, canonical_message, message_id, Fingerprint};
    use crate::ir::Schema;
    use crate::model::{Field, Model};
    use std::path::PathBuf;

    fn field(name: &str, ty: &str) -> Field {
        Field {
            name: name.to_owned(),
            network_type: ty.to_owned(),
            codecs: vec!["edge".to_owned()],
            line: 1,
        }
    }

    fn model(name: &str, fields: Vec<Field>) -> Model {
        Model {
            name: name.to_owned(),
            source: PathBuf::from("src/models.rs"),
            line: 1,
            codecs: vec!["edge".to_owned()],
            fields,
        }
    }

    fn schema(models: &[Model]) -> Schema {
        Schema::build(models).expect("build")
    }

    fn player(fields: Vec<Field>) -> Schema {
        schema(&[model("Player", fields)])
    }

    fn player_fingerprint(fields: Vec<Field>) -> Fingerprint {
        player(fields)
            .message("Player.edge")
            .expect("message")
            .fingerprint
    }

    #[test]
    fn the_canonical_text_is_exactly_this() {
        let schema = player(vec![field("id", "u32"), field("x", "f32")]);
        let model = schema.model("Player").expect("model");

        assert_eq!(
            canonical_message(model, "edge", &schema.models),
            "cyclone-fingerprint/2\nmessage Player.edge\nfield 0 id u32\nfield 1 x f32\nend\n"
        );
    }

    #[test]
    fn a_naming_convention_is_not_a_schema_difference() {
        let rust = player_fingerprint(vec![field("id", "u32"), field("x", "f32")]);
        let go = player_fingerprint(vec![field("ID", "u32"), field("X", "f32")]);
        let csharp = player_fingerprint(vec![field("Id", "u32"), field("X", "f32")]);
        let screaming = player_fingerprint(vec![field("ID_", "u32"), field("X", "f32")]);

        assert_eq!(rust, go);
        assert_eq!(rust, csharp);
        assert_eq!(rust, screaming);
    }

    #[test]
    fn a_real_rename_still_changes_the_fingerprint() {
        assert_ne!(
            player_fingerprint(vec![field("x", "f32")]),
            player_fingerprint(vec![field("position_x", "f32")]),
        );
    }

    #[test]
    fn a_canonical_name_is_exactly_this() {
        for (spelling, canonical) in [
            ("id", "id"),
            ("ID", "id"),
            ("Id", "id"),
            ("player_id", "playerid"),
            ("playerId", "playerid"),
            ("PlayerId", "playerid"),
            ("PlayerID", "playerid"),
            ("PLAYER_ID", "playerid"),
            ("player-id", "playerid"),
            ("__player_id__", "playerid"),
            ("HTTPServer", "httpserver"),
            ("http_server", "httpserver"),
            ("HttpServer", "httpserver"),
            ("UserIDs", "userids"),
            ("user_ids", "userids"),
            ("UserIds", "userids"),
            ("vec3", "vec3"),
            ("Vec3", "vec3"),
            ("vec_3", "vec3"),
            ("position3D", "position3d"),
            ("position_3d", "position3d"),
            ("café_id", "caféid"),
            ("_", "_"),
            ("__", "__"),
        ] {
            assert_eq!(canonical_field_name(spelling), canonical, "{spelling}");
        }
    }

    #[test]
    fn a_canonical_name_still_separates_what_matters() {
        assert_ne!(canonical_field_name("x"), canonical_field_name("y"));
        assert_ne!(
            canonical_field_name("x"),
            canonical_field_name("position_x")
        );
    }

    #[test]
    fn a_fingerprint_is_stable_across_runs_and_releases() {
        let fingerprint = player_fingerprint(vec![field("id", "u32"), field("x", "f32")]);
        assert_eq!(
            fingerprint.tagged(),
            "sha256:f1ed8779e2a4a35d30067fa88ba1cec15f7417ea9907df5752f06b97a0d93ddc"
        );
    }

    #[test]
    fn every_wire_change_changes_the_fingerprint() {
        let base = player_fingerprint(vec![field("id", "u32"), field("x", "f32")]);

        assert_ne!(
            base,
            player_fingerprint(vec![field("x", "f32"), field("id", "u32")])
        );
        assert_ne!(
            base,
            player_fingerprint(vec![field("id", "u64"), field("x", "f32")])
        );
        assert_ne!(base, player_fingerprint(vec![field("id", "u32")]));
        assert_ne!(
            base,
            player_fingerprint(vec![
                field("id", "u32"),
                field("level", "u32"),
                field("x", "f32"),
            ])
        );
        assert_ne!(
            base,
            player_fingerprint(vec![
                field("id", "u32"),
                field("x", "f32"),
                field("level", "u32"),
            ])
        );
    }

    #[test]
    fn swapping_two_same_typed_fields_changes_the_fingerprint() {
        assert_ne!(
            player_fingerprint(vec![field("x", "f32"), field("y", "f32")]),
            player_fingerprint(vec![field("y", "f32"), field("x", "f32")]),
        );
    }

    #[test]
    fn a_nested_change_reaches_the_outer_fingerprint() {
        let before = schema(&[
            model("Player", vec![field("info", "PlayerInfo")]),
            model("PlayerInfo", vec![field("level", "u32")]),
        ]);
        let after = schema(&[
            model("Player", vec![field("info", "PlayerInfo")]),
            model("PlayerInfo", vec![field("level", "u64")]),
        ]);

        assert_ne!(
            before.message("Player.edge").expect("before").fingerprint,
            after.message("Player.edge").expect("after").fingerprint,
        );
    }

    #[test]
    fn a_recursive_model_terminates() {
        let schema = schema(&[model(
            "Node",
            vec![field("id", "u32"), field("children", "Array<Node>")],
        )]);
        let text = canonical_message(schema.model("Node").expect("model"), "edge", &schema.models);
        assert!(text.contains("recursive<Node>"), "{text}");
    }

    #[test]
    fn a_message_id_follows_the_name_only() {
        let one = player(vec![field("id", "u32")]);
        let two = player(vec![field("id", "u32"), field("x", "f32")]);

        let one = one.message("Player.edge").expect("one");
        let two = two.message("Player.edge").expect("two");
        assert_eq!(one.id, two.id);
        assert_ne!(one.fingerprint, two.fingerprint);
        assert_eq!(one.id, message_id("Player.edge"));
    }

    #[test]
    fn a_fingerprint_round_trips_through_its_text_form() {
        let fingerprint = player_fingerprint(vec![field("id", "u32")]);
        assert_eq!(
            Fingerprint::parse(&fingerprint.tagged()).expect("parse"),
            fingerprint
        );
        assert!(Fingerprint::parse("abc").is_err());
        assert!(Fingerprint::parse("sha256:xyz").is_err());
    }

    #[test]
    fn the_u64_form_is_the_leading_eight_bytes() {
        let fingerprint =
            Fingerprint::parse(&format!("sha256:{}", "0123456789abcdef".repeat(4))).expect("parse");
        assert_eq!(fingerprint.u64(), 0x0123_4567_89ab_cdef);
    }
}
