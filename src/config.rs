use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "cyclone.toml";

#[derive(Debug, Default, Clone)]
pub struct Config {
    pub src: Vec<PathBuf>,
    pub out: Option<PathBuf>,
    pub model_path: Option<String>,
    pub validate_message_fingerprint: Option<bool>,
}

impl Config {
    pub fn load(directory: &Path) -> Result<Config, String> {
        let path = directory.join(FILE_NAME);
        if !path.exists() {
            return Ok(Config::default());
        }

        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        parse(&text).map_err(|problem| format!("{}: {problem}", path.display()))
    }
}

fn parse(text: &str) -> Result<Config, String> {
    let mut config = Config::default();
    let mut table: Option<String> = None;

    for (number, line) in text.lines().enumerate() {
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            table = Some(name.trim().to_owned());
            continue;
        }

        if table.as_deref().is_some_and(|name| name != "cyclone") {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        let at = number + 1;

        match key {
            "src" => {
                config.src = strings(value).map_err(|problem| format!("{at}: src {problem}"))?
            }
            "out" => {
                config.out = Some(PathBuf::from(
                    string(value).map_err(|problem| format!("{at}: out {problem}"))?,
                ))
            }
            "model_path" => {
                config.model_path =
                    Some(string(value).map_err(|problem| format!("{at}: model_path {problem}"))?)
            }
            "validate_message_fingerprint" => {
                config.validate_message_fingerprint = Some(match value {
                    "true" => true,
                    "false" => false,
                    other => {
                        return Err(format!(
                            "{at}: validate_message_fingerprint is `true` or `false`, not `{other}`"
                        ))
                    }
                })
            }
            _ => {}
        }
    }

    Ok(config)
}

fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    for (index, character) in line.char_indices() {
        match character {
            '"' => in_string = !in_string,
            '#' if !in_string => return &line[..index],
            _ => {}
        }
    }
    line
}

fn string(value: &str) -> Result<String, String> {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .map(str::to_owned)
        .ok_or_else(|| format!("must be a quoted string, not `{value}`"))
}

fn strings(value: &str) -> Result<Vec<PathBuf>, String> {
    if let Some(list) = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        return list
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| string(item).map(PathBuf::from))
            .collect();
    }
    Ok(vec![PathBuf::from(string(value)?)])
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::parse;

    #[test]
    fn reads_the_three_keys() {
        let config = parse(
            "# the project's paths\nsrc = \"src\"\nout = \"generated\"\n\
             validate_message_fingerprint = true\n",
        )
        .expect("parse");

        assert_eq!(config.src, [PathBuf::from("src")]);
        assert_eq!(config.out, Some(PathBuf::from("generated")));
        assert_eq!(config.validate_message_fingerprint, Some(true));
    }

    #[test]
    fn src_may_be_a_list() {
        let config = parse("src = [\"src\", \"crates/shared/src\"]").expect("parse");
        assert_eq!(
            config.src,
            [PathBuf::from("src"), PathBuf::from("crates/shared/src")]
        );
    }

    #[test]
    fn keys_may_sit_under_a_cyclone_table_and_other_tables_are_ignored() {
        let config =
            parse("[cyclone]\nout = \"generated\"\n\n[other]\nout = \"nope\"\n").expect("parse");
        assert_eq!(config.out, Some(PathBuf::from("generated")));
    }

    #[test]
    fn a_hash_inside_a_string_is_not_a_comment() {
        let config = parse("out = \"gen#erated\" # really").expect("parse");
        assert_eq!(config.out, Some(PathBuf::from("gen#erated")));
    }

    #[test]
    fn a_value_of_the_wrong_shape_is_reported() {
        assert!(parse("out = generated").is_err());
        assert!(parse("validate_message_fingerprint = yes").is_err());
    }
}
