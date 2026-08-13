//! Schema compatibility: what changed between two schemas, and whether it
//! breaks the wire.
//!
//! A fingerprint answers *same or different* and stops there. When it says
//! different, this module says **how**, by comparing two schemas field by
//! field. Nothing here infers a kind of change from a hash - a hash has no
//! structure to infer from, and guessing is how a checker starts calling
//! append-only changes breaking, or worse, the other way round.
//!
//! | Change | Verdict |
//! |---|---|
//! | nothing changed | `CURRENT` |
//! | a field appended at the end | `COMPATIBLE` |
//! | a field removed from the end | `BREAKING` |
//! | a field removed from the middle | `BREAKING` |
//! | a field inserted in the middle | `BREAKING` |
//! | fields reordered | `BREAKING` |
//! | a field's wire type changed | `BREAKING` |
//! | a whole message added | `COMPATIBLE` |
//! | a whole message removed | `BREAKING` |
//!
//! The table is RFC-0002 §9.1 restated: version skew is valid **only** for
//! fields appended to the end. Everything else changes what an existing offset
//! means, and an old peer reading a new stream cannot tell.
//!
//! Comparison is per **message** (`Model.codec`), because a message is what a
//! peer actually encodes. Two codecs of one model can evolve independently, and
//! frequently do.

use crate::ir::Schema;

/// How a change rates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    /// Byte-for-byte the same contract.
    Current,
    /// Different, but an old peer and a new one still understand each other:
    /// version skew, in the one shape RFC-0002 §9.1 permits.
    Compatible,
    /// An old peer would misread a new stream, or the other way round.
    Breaking,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Current => "CURRENT",
            Verdict::Compatible => "COMPATIBLE",
            Verdict::Breaking => "BREAKING",
        }
    }
}

/// One field-level difference between two versions of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A field exists at `index` in the new message and nowhere in the old one
    /// - it is past the old message's last field.
    Added {
        index: usize,
        name: String,
        ty: String,
    },
    /// A field the old message had at `index` that the new one does not reach.
    Removed {
        index: usize,
        name: String,
        ty: String,
    },
    /// Both messages have a field at `index`, and it is not the same field.
    Changed {
        index: usize,
        old: (String, String),
        new: (String, String),
    },
    /// The message itself is new.
    MessageAdded,
    /// The message itself is gone.
    MessageRemoved,
}

/// One message's worth of comparison.
#[derive(Debug, Clone)]
pub struct MessageDiff {
    /// `Model.codec`.
    pub name: String,
    pub verdict: Verdict,
    /// Why it rates that way, in the words the report prints.
    pub reason: String,
    pub changes: Vec<Change>,
}

/// Every message of two schemas, compared.
#[derive(Debug, Clone)]
pub struct Report {
    pub messages: Vec<MessageDiff>,
    /// Whether the two schema-level fingerprints matched.
    pub fingerprint_matched: bool,
}

impl Report {
    /// The worst verdict in the report - what a CI run exits on.
    pub fn verdict(&self) -> Verdict {
        self.messages
            .iter()
            .map(|message| message.verdict)
            .max()
            .unwrap_or(Verdict::Current)
    }

    /// Whether anything at all differs.
    pub fn is_current(&self) -> bool {
        self.verdict() == Verdict::Current
    }

    /// The report as text.
    ///
    /// `unchanged` includes the `✓ Player.edge unchanged` lines; a local
    /// `generate` prints them, CI does not.
    pub fn render(&self, unchanged: bool) -> String {
        let mut out = String::new();

        for message in &self.messages {
            if message.verdict == Verdict::Current {
                if unchanged {
                    out.push_str(&format!("✓ {} unchanged\n", message.name));
                }
                continue;
            }

            out.push_str(&format!("⚠ {}:\n", message.name));
            for change in &message.changes {
                match change {
                    Change::Added { index, name, ty } => {
                        out.push_str(&format!("  + {name}:{ty} at index {index}\n"));
                    }
                    Change::Removed { index, name, ty } => {
                        out.push_str(&format!("  - {name}:{ty} at index {index}\n"));
                    }
                    Change::Changed { index, old, new } => {
                        out.push_str(&format!("  field[{index}]:\n"));
                        out.push_str(&format!("    old: {}:{}\n", old.0, old.1));
                        out.push_str(&format!("    new: {}:{}\n", new.0, new.1));
                    }
                    Change::MessageAdded => out.push_str("  + new message\n"),
                    Change::MessageRemoved => out.push_str("  - message removed\n"),
                }
            }
            out.push_str(&format!(
                "  {}: {}\n",
                message.verdict.label(),
                message.reason
            ));
        }

        out
    }
}

/// Compares `head` (what source says now) against `base` (the schema being
/// evolved from).
pub fn compare(base: &Schema, head: &Schema) -> Report {
    let mut messages = Vec::new();

    for head_message in head.messages() {
        match base.message(&head_message.name) {
            Some(base_message) => messages.push(diff(
                &head_message.name,
                &base_message.fields,
                &head_message.fields,
            )),
            None => messages.push(MessageDiff {
                name: head_message.name.clone(),
                verdict: Verdict::Compatible,
                reason: "new message; no peer can be running it yet".to_owned(),
                changes: vec![Change::MessageAdded],
            }),
        }
    }

    for base_message in base.messages() {
        if head.message(&base_message.name).is_none() {
            messages.push(MessageDiff {
                name: base_message.name.clone(),
                verdict: Verdict::Breaking,
                reason: "message removed; a peer may still send it".to_owned(),
                changes: vec![Change::MessageRemoved],
            });
        }
    }

    messages.sort_by(|left, right| left.name.cmp(&right.name));

    Report {
        fingerprint_matched: base.fingerprint == head.fingerprint,
        messages,
    }
}

/// Compares one message's fields, by position - the only identifier the wire
/// has (RFC-0001 §4.1).
fn diff(name: &str, base: &[crate::ir::Field], head: &[crate::ir::Field]) -> MessageDiff {
    let mut changes = Vec::new();

    for index in 0..base.len().max(head.len()) {
        match (base.get(index), head.get(index)) {
            (Some(old), Some(new)) => {
                let old = (old.name.clone(), old.ty.spelling());
                let new = (new.name.clone(), new.ty.spelling());
                if old != new {
                    changes.push(Change::Changed { index, old, new });
                }
            }
            (None, Some(new)) => changes.push(Change::Added {
                index,
                name: new.name.clone(),
                ty: new.ty.spelling(),
            }),
            (Some(old), None) => changes.push(Change::Removed {
                index,
                name: old.name.clone(),
                ty: old.ty.spelling(),
            }),
            (None, None) => unreachable!("index is below one of the two lengths"),
        }
    }

    let (verdict, reason) = classify(&changes, base.len(), head.len());

    MessageDiff {
        name: name.to_owned(),
        verdict,
        reason,
        changes,
    }
}

/// Turns a list of differences into a verdict and the sentence explaining it.
fn classify(changes: &[Change], base_len: usize, head_len: usize) -> (Verdict, String) {
    if changes.is_empty() {
        return (Verdict::Current, "no change".to_owned());
    }

    let removed = changes
        .iter()
        .any(|change| matches!(change, Change::Removed { .. }));
    let retyped = changes
        .iter()
        .any(|change| matches!(change, Change::Changed { old, new, .. } if old.1 != new.1));
    let renamed = changes.iter().any(|change| {
        matches!(change, Change::Changed { old, new, .. } if old.1 == new.1 && old.0 != new.0)
    });
    let changed_in_place = retyped || renamed;

    // Appending to the end is the one evolution RFC-0002 §9.1 calls valid: a
    // new peer's extra bytes are ignored by an old reader, and an old peer's
    // missing bytes leave the new reader's extra fields absent.
    if !removed && !changed_in_place {
        let count = changes.len();
        let plural = if count == 1 { "field" } else { "fields" };
        return (
            Verdict::Compatible,
            format!("append-only {plural} ({count} appended at the end)"),
        );
    }

    let reason = if removed && !changed_in_place {
        if head_len < base_len && changes.len() == base_len - head_len {
            "field removed from the end".to_owned()
        } else {
            "field removed".to_owned()
        }
    } else if head_len > base_len {
        "field inserted; every field after it moved".to_owned()
    } else if head_len < base_len {
        "field removed from the middle; every field after it moved".to_owned()
    } else if retyped && renamed {
        "fields replaced".to_owned()
    } else if retyped {
        "field wire type changed".to_owned()
    } else if reordered(changes) {
        "field order changed".to_owned()
    } else {
        // Same types, same positions, different names. The bytes did not move,
        // but a fingerprint hashes field names (SPEC-FINGERPRINT.md §5), so two
        // peers either side of this change will refuse each other.
        "field renamed; the bytes are unchanged, but the fingerprint is not, \
         so peers either side of it will refuse each other"
            .to_owned()
    };

    (Verdict::Breaking, reason)
}

/// Whether the changed fields are the same set of fields in a different order,
/// rather than one of them having been renamed.
fn reordered(changes: &[Change]) -> bool {
    let mut before: Vec<&(String, String)> = Vec::new();
    let mut after: Vec<&(String, String)> = Vec::new();
    for change in changes {
        if let Change::Changed { old, new, .. } = change {
            before.push(old);
            after.push(new);
        }
    }
    before.sort();
    after.sort();
    before == after
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{compare, Verdict};
    use crate::ir::Schema;
    use crate::model::{Field, Model};

    fn schema(fields: &[(&str, &str)]) -> Schema {
        Schema::build(&[Model {
            name: "Player".to_owned(),
            source: PathBuf::from("src/models.rs"),
            line: 1,
            codecs: vec!["edge".to_owned()],
            fields: fields
                .iter()
                .map(|(name, ty)| Field {
                    name: (*name).to_owned(),
                    network_type: (*ty).to_owned(),
                    codecs: vec!["edge".to_owned()],
                    line: 1,
                })
                .collect(),
        }])
        .expect("build")
    }

    const V1: &[(&str, &str)] = &[("id", "u32"), ("x", "f32"), ("y", "f32")];

    fn verdict(base: &[(&str, &str)], head: &[(&str, &str)]) -> Verdict {
        compare(&schema(base), &schema(head)).verdict()
    }

    fn reason(base: &[(&str, &str)], head: &[(&str, &str)]) -> String {
        compare(&schema(base), &schema(head)).messages[0]
            .reason
            .clone()
    }

    #[test]
    fn no_change_is_current() {
        assert_eq!(verdict(V1, V1), Verdict::Current);
    }

    #[test]
    fn appending_at_the_end_is_compatible() {
        let head = &[("id", "u32"), ("x", "f32"), ("y", "f32"), ("level", "u32")];
        assert_eq!(verdict(V1, head), Verdict::Compatible);
        assert!(
            reason(V1, head).contains("append-only"),
            "{}",
            reason(V1, head)
        );
    }

    #[test]
    fn removing_the_last_field_is_breaking() {
        let head = &[("id", "u32"), ("x", "f32")];
        assert_eq!(verdict(V1, head), Verdict::Breaking);
        assert!(reason(V1, head).contains("removed from the end"));
    }

    #[test]
    fn removing_a_middle_field_is_breaking() {
        let head = &[("id", "u32"), ("y", "f32")];
        assert_eq!(verdict(V1, head), Verdict::Breaking);
    }

    #[test]
    fn inserting_a_middle_field_is_breaking() {
        let head = &[("id", "u32"), ("level", "u32"), ("x", "f32"), ("y", "f32")];
        assert_eq!(verdict(V1, head), Verdict::Breaking);
        assert!(reason(V1, head).contains("inserted"));
    }

    #[test]
    fn reordering_two_same_typed_fields_is_breaking() {
        let head = &[("id", "u32"), ("y", "f32"), ("x", "f32")];
        assert_eq!(verdict(V1, head), Verdict::Breaking);
        assert_eq!(reason(V1, head), "field order changed");
    }

    /// A rename moves no bytes, but it does move the fingerprint, and the
    /// report says exactly that rather than calling it a reorder.
    #[test]
    fn renaming_a_field_is_breaking_and_says_why() {
        let head = &[("id", "u32"), ("x", "f32"), ("position_y", "f32")];
        assert_eq!(verdict(V1, head), Verdict::Breaking);
        assert!(
            reason(V1, head).starts_with("field renamed"),
            "{}",
            reason(V1, head)
        );
    }

    #[test]
    fn changing_a_wire_type_is_breaking() {
        let head = &[("id", "u64"), ("x", "f32"), ("y", "f32")];
        assert_eq!(verdict(V1, head), Verdict::Breaking);
        assert_eq!(reason(V1, head), "field wire type changed");
    }

    /// The report reads the way §9 of the brief spells it out.
    #[test]
    fn the_report_explains_the_cause() {
        let report = compare(
            &schema(V1),
            &schema(&[("id", "u32"), ("y", "f32"), ("x", "f32")]),
        );
        let text = report.render(false);

        assert!(text.contains("Player.edge:"), "{text}");
        assert!(text.contains("field[1]:"), "{text}");
        assert!(text.contains("old: x:f32"), "{text}");
        assert!(text.contains("new: y:f32"), "{text}");
        assert!(text.contains("BREAKING: field order changed"), "{text}");
    }

    #[test]
    fn an_append_reads_the_way_the_brief_spells_it() {
        let report = compare(
            &schema(V1),
            &schema(&[("id", "u32"), ("x", "f32"), ("y", "f32"), ("level", "u32")]),
        );
        let text = report.render(false);

        assert!(text.contains("+ level:u32 at index 3"), "{text}");
        assert!(text.contains("COMPATIBLE: append-only"), "{text}");
    }

    #[test]
    fn an_unchanged_message_can_be_reported_or_kept_quiet() {
        let report = compare(&schema(V1), &schema(V1));
        assert_eq!(report.render(true), "✓ Player.edge unchanged\n");
        assert_eq!(report.render(false), "");
    }

    #[test]
    fn a_new_message_is_compatible_and_a_removed_one_is_not() {
        let empty = Schema::build(&[]).expect("build");
        assert_eq!(compare(&empty, &schema(V1)).verdict(), Verdict::Compatible);
        assert_eq!(compare(&schema(V1), &empty).verdict(), Verdict::Breaking);
    }
}
