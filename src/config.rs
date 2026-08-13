//! `cyclone.toml` - the two paths, and one option.
//!
//! ```toml
//! src = "src"
//! out = "generated"
//! model_path = "crate::models"        # optional
//! validate_message_fingerprint = false
//! ```
//!
//! A command-line flag always wins over the file: the file says what a project
//! normally does, and a flag says what this invocation is doing instead.
//!
//! This reads the subset of TOML those three lines need and no more. `src` may
//! also be a list, and keys may sit under a `[cyclone]` table for projects that
//! would rather keep them out of the top level. Anything else in the file is
//! skipped rather than rejected, so a `cyclone.toml` that grows a section for
//! some future tool does not stop this one from starting.

use std::path::{Path, PathBuf};

/// The file name, looked for in the project root.
pub const FILE_NAME: &str = "cyclone.toml";

/// What `cyclone.toml` said.
#[derive(Debug, Default, Clone)]
pub struct Config {
    /// Directories (or files) to scan. Every file found must be one
    /// language (`.rs`, `.go`, `.cs`, `.gd`, `.hpp`/`.cpp`/`.cc`/`.cxx`
    /// (C++), or `.c`/`.h` (C)), never a mix.
    pub src: Vec<PathBuf>,
    /// Where generated source goes.
    pub out: Option<PathBuf>,
    /// Where a generated codec reaches the models: a Rust module path, a Go
    /// import path, or a C#/C++ namespace. No effect on GDScript or C -
    /// GDScript because every model's own `class_name` is already reachable
    /// project-wide, C because it has no namespace to begin with.
    pub model_path: Option<String>,
    /// Whether generated frames carry `[MessageId][MessageFingerprint]`.
    pub validate_message_fingerprint: Option<bool>,
}

impl Config {
    /// Reads `cyclone.toml` from `directory`, or returns the empty config if
    /// there is none.
    ///
    /// # Errors
    ///
    /// The file exists but cannot be read, or holds a value of the wrong shape.
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

        // Only the top level and a `[cyclone]` table are ours.
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

/// A `#` outside a string starts a comment.
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
