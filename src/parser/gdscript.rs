use std::path::Path;

use crate::model::{Field, Model};
use crate::parser::Error;

pub fn parse(path: &Path, text: &str) -> Result<Vec<Model>, Error> {
    let mut models: Vec<Model> = Vec::new();
    let mut current: Option<usize> = None;
    let mut pending: Option<(Directive, usize)> = None;

    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = raw_line.trim();

        if trimmed.is_empty() {
            continue;
        }

        let top_level = raw_line.starts_with(|c: char| !c.is_whitespace());

        if top_level {
            if let Some(rest) = directive_text(trimmed) {
                if let Some((directive, line)) = pending.take() {
                    return Err(err(path, line, &pending_message(&directive)));
                }
                let directive =
                    parse_directive(rest).map_err(|message| err(path, line_number, &message))?;
                pending = Some((directive, line_number));
                continue;
            }

            if trimmed.starts_with('#') {
                continue;
            }

            if let Some(name) = keyword_identifier(trimmed, "class_name") {
                match pending.take() {
                    Some((Directive::Model { codecs }, line)) => {
                        models.push(Model {
                            name: name.to_owned(),
                            source: path.to_path_buf(),
                            line,
                            codecs,
                            fields: Vec::new(),
                        });
                        current = Some(models.len() - 1);
                    }
                    Some((Directive::Field { .. }, line)) => {
                        return Err(err(
                            path,
                            line,
                            "# cyclone:TYPE directive must be immediately followed by a \
                             `var name` declaration, not `class_name`",
                        ));
                    }
                    None => {}
                }
                continue;
            }

            if let Some(name) = keyword_identifier(trimmed, "var") {
                match pending.take() {
                    Some((
                        Directive::Field {
                            network_type,
                            codecs,
                        },
                        line,
                    )) => {
                        let Some(model_index) = current else {
                            return Err(err(
                                path,
                                line_number,
                                &format!(
                                    "# cyclone:{network_type} marks field '{name}', but no \
                                     `# cyclone:model` / `class_name` has opened a model yet"
                                ),
                            ));
                        };
                        models[model_index].fields.push(Field {
                            name: name.to_owned(),
                            network_type,
                            codecs,
                            line,
                        });
                    }
                    Some((Directive::Model { .. }, line)) => {
                        return Err(err(
                            path,
                            line,
                            "# cyclone:model directive must be immediately followed by a \
                             `class_name Name` declaration, not `var`",
                        ));
                    }
                    None => {}
                }
                continue;
            }
        }

        if let Some((directive, line)) = pending.take() {
            return Err(err(path, line, &pending_message(&directive)));
        }
    }

    if let Some((directive, line)) = pending {
        return Err(err(path, line, &pending_message(&directive)));
    }

    Ok(models)
}

enum Directive {
    Model {
        codecs: Vec<String>,
    },
    Field {
        network_type: String,
        codecs: Vec<String>,
    },
}

fn pending_message(directive: &Directive) -> String {
    match directive {
        Directive::Model { .. } => {
            "# cyclone:model directive must be immediately followed by a `class_name Name` \
             declaration"
                .to_owned()
        }
        Directive::Field { network_type, .. } => format!(
            "# cyclone:{network_type} directive must be immediately followed by a `var name` \
             declaration"
        ),
    }
}

fn directive_text(trimmed: &str) -> Option<&str> {
    let after_hash = trimmed.strip_prefix('#')?;
    let after_spaces = after_hash.trim_start_matches(' ');
    after_spaces.strip_prefix("cyclone:")
}

fn parse_directive(text: &str) -> Result<Directive, String> {
    let text = text.trim_start();
    if text.is_empty() {
        return Err(
            "invalid # cyclone: directive: expected `model` or a wire type, optionally \
             followed by `codec=name,name,...`"
                .to_owned(),
        );
    }

    let head_end = text.find(char::is_whitespace).unwrap_or(text.len());
    let head = &text[..head_end];
    let rest = text[head_end..].trim();

    let codecs = parse_codec_argument(head, rest)?;

    if head == "model" {
        Ok(Directive::Model { codecs })
    } else {
        Ok(Directive::Field {
            network_type: head.to_owned(),
            codecs,
        })
    }
}

fn parse_codec_argument(head: &str, rest: &str) -> Result<Vec<String>, String> {
    if rest.is_empty() {
        return Ok(Vec::new());
    }
    let Some(list) = rest.strip_prefix("codec=") else {
        return Err(format!(
            "invalid # cyclone:{head} directive: expected nothing or `codec=name,name,...`, \
             found `{rest}`"
        ));
    };
    Ok(split_codec_list(list))
}

fn split_codec_list(list: &str) -> Vec<String> {
    let mut seen = Vec::new();
    let mut out = Vec::new();

    for name in list.split(',') {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if seen.iter().any(|kept: &String| kept == name) {
            continue;
        }
        seen.push(name.to_owned());
        out.push(name.to_owned());
    }

    out
}

fn keyword_identifier<'a>(trimmed: &'a str, keyword: &str) -> Option<&'a str> {
    let after_keyword = trimmed.strip_prefix(keyword)?;
    let after_spaces = after_keyword.strip_prefix(char::is_whitespace)?;
    let after_spaces = after_spaces.trim_start();

    let end = after_spaces
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(after_spaces.len());
    if end == 0 {
        return None;
    }
    Some(&after_spaces[..end])
}

fn err(path: &Path, line: usize, message: &str) -> Error {
    Error {
        path: path.to_path_buf(),
        line,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::parse;

    #[test]
    fn a_directive_followed_by_class_name_is_a_model() {
        let text = "# cyclone:model codec=edge,godot\nclass_name DeviceState\n\n# cyclone:u32 codec=edge,godot\nvar id: int\n";
        let models = parse(Path::new("device_state.gd"), text).expect("parse");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "DeviceState");
        assert_eq!(models[0].codecs, ["edge", "godot"]);
        assert_eq!(models[0].fields.len(), 1);
        assert_eq!(models[0].fields[0].name, "id");
        assert_eq!(models[0].fields[0].network_type, "u32");
        assert_eq!(models[0].fields[0].codecs, ["edge", "godot"]);
    }

    #[test]
    fn a_model_directive_not_followed_by_class_name_is_an_error() {
        let text = "# cyclone:model\nfunc not_a_class() -> void:\n\tpass\n";
        let error = parse(Path::new("bad.gd"), text).expect_err("error");
        assert!(
            error.message.contains("class_name Name"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_field_directive_not_followed_by_var_is_an_error() {
        let text = "# cyclone:model\nclass_name Player\n\n# cyclone:u32\nfunc get_id() -> int:\n\treturn 0\n";
        let error = parse(Path::new("player.gd"), text).expect_err("error");
        assert!(error.message.contains("var name"), "{}", error.message);
    }

    #[test]
    fn a_field_with_no_directive_at_all_is_skipped_not_an_error() {
        let text = "# cyclone:model codec=edge\nclass_name Player\n\nvar cache: String\n\n# cyclone:u32 codec=edge\nvar id: int\n";
        let models = parse(Path::new("player.gd"), text).expect("parse");
        assert_eq!(models[0].fields.len(), 1);
        assert_eq!(models[0].fields[0].name, "id");
    }

    #[test]
    fn a_field_directive_with_no_model_open_yet_is_an_error() {
        let text = "# cyclone:u32\nvar id: int\n";
        let error = parse(Path::new("player.gd"), text).expect_err("error");
        assert!(
            error
                .message
                .contains("no `# cyclone:model` / `class_name`"),
            "{}",
            error.message
        );
    }

    #[test]
    fn an_unmarked_class_name_is_not_a_model_and_not_an_error() {
        let text = "class_name NotAModel\nvar id: int\n";
        let models = parse(Path::new("plain.gd"), text).expect("parse");
        assert!(models.is_empty());
    }

    #[test]
    fn a_malformed_directive_argument_is_reported() {
        let text = "# cyclone:model weird=stuff\nclass_name Player\n";
        let error = parse(Path::new("player.gd"), text).expect_err("error");
        assert!(
            error.message.contains("codec=name,name,..."),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_typo_of_the_directive_prefix_is_still_a_reported_error() {
        let text = "# cyclone:modeling this is not a directive\nclass_name Player\n";
        let error = parse(Path::new("player.gd"), text).expect_err("error");
        assert!(error.message.contains("modeling"), "{}", error.message);
    }

    #[test]
    fn blank_lines_and_plain_comments_do_not_break_an_association() {
        let text = "# cyclone:model codec=edge\n\n# just a comment\n\nclass_name Player\n\n# cyclone:u32 codec=edge\n\n# another comment\n\nvar id: int\n";
        let models = parse(Path::new("player.gd"), text).expect("parse");
        assert_eq!(models[0].name, "Player");
        assert_eq!(models[0].fields[0].name, "id");
    }

    #[test]
    fn a_nested_indented_directive_is_out_of_scope() {
        let text = "# cyclone:model codec=edge\nclass_name Player\n\nclass Nested:\n\t# cyclone:u32 codec=edge\n\tvar id: int\n";
        let models = parse(Path::new("player.gd"), text).expect("parse");
        assert_eq!(models.len(), 1);
        assert!(models[0].fields.is_empty(), "{:?}", models[0].fields);
    }

    #[test]
    fn source_and_line_are_tracked_for_error_messages() {
        let text = "# cyclone:model codec=edge\nclass_name Player\n\n# cyclone:u32 codec=edge\nvar id: int\n";
        let models = parse(Path::new("models/player.gd"), text).expect("parse");
        assert_eq!(models[0].source, Path::new("models/player.gd"));
        assert_eq!(models[0].line, 1);
        assert_eq!(models[0].fields[0].line, 4);
    }
}
