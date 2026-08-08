//! GDScript source → [`Model`]s.
//!
//! GDScript has no attribute syntax a user can extend: an unrecognized `@name`
//! annotation is a **parse error** in Godot's own compiler (only a fixed,
//! engine-defined set - `@export`, `@onready`, `@rpc`, `@tool`, ... - is
//! accepted, and none of them carries arbitrary user metadata). A `.gd` file
//! using `@network(...)` or `@codec(...)` would not compile in Godot at all,
//! which is disqualifying: h.md's own requirement is that Cyclone-annotated
//! GDScript compiles with a stock Godot editor, no Cyclone plugin needed.
//!
//! So - like [`super::go`], for the identical reason - Cyclone metadata lives
//! in a comment:
//!
//! ```text
//! # cyclone:model codec=edge,godot     this class is a model, generate these
//! class_name DeviceState
//!
//! # cyclone:u32 codec=edge,godot       this field's wire type and codecs
//! var id: int
//! ```
//!
//! `# cyclone:model` is Go's `//cyclone:model` (names the model and its
//! codecs); `# cyclone:u32` is Go's `` `cyclone:"u32" codec:"edge,godot"` ``
//! struct tag, spelled as a comment instead because GDScript has no tag
//! syntax to attach it to the field directly. Both pieces of a field's
//! metadata - wire type and codec list - live on the one comment line, the
//! same "one line per field" shape Go's struct tag already has.
//!
//! GDScript's own type (`int`, `float`, `String`, ...) is never read: exactly
//! as for Rust, C# and Go, the annotation alone says what goes on the wire.
//!
//! # A comment is safe by construction
//!
//! Nothing here tries to guess whether a comment "means" Cyclone: ownership is
//! the exact text `# cyclone:`, nothing softer. A comment that does not start
//! with it is left alone, the same way [`super::go`] drops every comment but
//! its one recognized shape - this project does not interpret a comment it
//! does not own.
//!
//! Unlike Go's fixed literal prefix (`//cyclone:model`, matched whole, so
//! `//cyclone:modeling` is just an ordinary comment), everything after
//! `# cyclone:` here *is* Cyclone's - a directive's head is compared as a
//! whole token, not a prefix, so there is no second "did I actually mean
//! this" gate the way Go's word-boundary check gives it. That is deliberate:
//! there is no way to tell "a typo for `# cyclone:model`" apart from "an
//! unrelated comment that happens to start the same way" without guessing,
//! and h.md is explicit that a malformed directive must never pass over in
//! silence. `# cyclone:modeling this is not a directive` is therefore a
//! reported error (an attempted field directive with wire type `modeling`
//! and a malformed `codec=` argument), not a silently ignored comment.
//!
//! # Scope
//!
//! Only column-0 (unindented) `class_name` and `var` declarations are
//! recognized - the top level of the script's implicit class, which is the
//! only class a `.gd` file's own `class_name` can name. A nested `class
//! Name:` block is out of scope, the same non-handling the C# scanner gives a
//! nested type and the Rust scanner gives a model inside `mod`.
//!
//! # Two errors, not zero
//!
//! As in Go: a directive is source, and a directive that names nothing -
//! `# cyclone:model` not immediately followed by `class_name Name`, or
//! `# cyclone:u32` not immediately followed by `var name` - is a malformed
//! Cyclone directive, reported rather than silently dropped. "Immediately
//! followed by" means the next *meaningful* line: blank lines and ordinary
//! (non-Cyclone) comments in between do not break the association, the same
//! leniency Go's token-based scanner has for free (a plain comment is not a
//! token, so it cannot sit "between" a directive and what it marks).

use std::path::Path;

use crate::model::{Field, Language, Model};
use crate::parser::Error;

/// Extracts every `# cyclone:model` class from `text`.
///
/// # Errors
///
/// A `# cyclone:model` directive not immediately followed by a `class_name
/// Name` declaration; a `# cyclone:TYPE` directive not immediately followed
/// by a `var name` declaration; a malformed directive argument; a field
/// directive with no model yet open to attach it to. Source that does not
/// compile for any other reason is Godot's to report.
pub fn parse(path: &Path, text: &str) -> Result<Vec<Model>, Error> {
    let mut models: Vec<Model> = Vec::new();
    let mut current: Option<usize> = None; // index into `models` of the open model
    let mut pending: Option<(Directive, usize)> = None; // (directive, its line)

    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = raw_line.trim();

        if trimmed.is_empty() {
            continue; // blank lines never break an association
        }

        let top_level = raw_line.starts_with(|c: char| !c.is_whitespace());

        if top_level {
            if let Some(rest) = directive_text(trimmed) {
                if let Some((directive, line)) = pending.take() {
                    // A directive where the previous one's declaration was due.
                    return Err(err(path, line, &pending_message(&directive)));
                }
                let directive = parse_directive(rest)
                    .map_err(|message| err(path, line_number, &message))?;
                pending = Some((directive, line_number));
                continue;
            }

            if trimmed.starts_with('#') {
                continue; // an ordinary comment: not ours, left alone
            }

            if let Some(name) = keyword_identifier(trimmed, "class_name") {
                match pending.take() {
                    Some((Directive::Model { codecs }, _)) => {
                        models.push(Model {
                            language: Language::GDScript,
                            name: name.to_owned(),
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
                    // An unmarked `class_name`: somebody else's script, not a
                    // Cyclone model - not an error, same as an unmarked
                    // `struct` in Rust or `class` in C#.
                    None => {}
                }
                continue;
            }

            if let Some(name) = keyword_identifier(trimmed, "var") {
                match pending.take() {
                    Some((Directive::Field { network_type, codecs }, _)) => {
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
                    // An unmarked `var`: not a network field, and not an
                    // error - the same treatment every other scanner gives an
                    // unannotated field.
                    None => {}
                }
                continue;
            }
        }

        // Any other line - indented, or top-level but neither `class_name`
        // nor `var` - cannot be what a pending directive requires.
        if let Some((directive, line)) = pending.take() {
            return Err(err(path, line, &pending_message(&directive)));
        }
    }

    if let Some((directive, line)) = pending {
        return Err(err(path, line, &pending_message(&directive)));
    }

    Ok(models)
}

/// What a directive comment, once parsed, turned out to require next.
enum Directive {
    /// `# cyclone:model`, holding the codecs it declared.
    Model { codecs: Vec<String> },
    /// `# cyclone:TYPE`, holding the wire type and the codecs it declared.
    Field { network_type: String, codecs: Vec<String> },
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

/// If `trimmed` is a Cyclone directive comment, the text after `cyclone:`.
///
/// The exact grammar: `#`, then zero or more spaces, then the literal
/// `cyclone:`. No space at all (`#cyclone:model`) and one space
/// (`# cyclone:model`, h.md's own spelling) both match; anything else
/// starting with `#` is an ordinary comment, left alone.
fn directive_text(trimmed: &str) -> Option<&str> {
    let after_hash = trimmed.strip_prefix('#')?;
    let after_spaces = after_hash.trim_start_matches(' ');
    after_spaces.strip_prefix("cyclone:")
}

/// Parses the text after `cyclone:`: `model`, optionally followed by
/// `codec=name,name,...`, or a wire type spelled the same way.
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
        Ok(Directive::Field { network_type: head.to_owned(), codecs })
    }
}

/// Parses the text after a directive's head (`model` or a wire type): nothing,
/// or `codec=name,name,...`.
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

/// Splits and trims a comma-separated codec list, dropping empties and
/// repeats, keeping the order written. See the identical helper in
/// [`super::go`].
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

/// If `trimmed` starts with `keyword` as a whole word, the identifier that
/// follows it - the rest of the line (a native type annotation, a default
/// value, ...) is never read, the same "the annotation already said what goes
/// on the wire" rule every other scanner in this project follows.
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
    Error { path: path.to_path_buf(), line, message: message.to_owned() }
}
