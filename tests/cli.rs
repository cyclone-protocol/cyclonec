//! The generator, driven the way a user drives it.
//!
//! These run the real binaries over real files in a real directory, so what is
//! asserted is what a user gets - not what an internal function returns.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

// ===================================================================== harness

/// A clean copy of `tests/fixtures/` - the annotated schema and a
/// `cyclone.toml` - in a directory of its own.
fn project(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("src/models")).expect("create project");

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::copy(
        fixtures.join("cyclone.toml"),
        directory.join("cyclone.toml"),
    )
    .expect("copy cyclone.toml");
    for entry in std::fs::read_dir(fixtures.join("src/models")).expect("read fixtures") {
        let path = entry.expect("entry").path();
        std::fs::copy(
            &path,
            directory
                .join("src/models")
                .join(path.file_name().expect("name")),
        )
        .expect("copy schema");
    }

    directory
}

fn cyclonec(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cyclonec"))
        .current_dir(directory)
        .args(arguments)
        .output()
        .expect("run cyclonec")
}

fn inspect(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cyclone-inspect"))
        .current_dir(directory)
        .args(arguments)
        .output()
        .expect("run cyclone-inspect")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn read(directory: &Path, path: &str) -> String {
    std::fs::read_to_string(directory.join(path))
        .unwrap_or_else(|error| panic!("read {path}: {error}"))
}

/// [`read`], without the panic - for a file a background `--watch` process
/// may not have written yet.
fn read_opt(directory: &Path, path: &str) -> Option<String> {
    std::fs::read_to_string(directory.join(path)).ok()
}

/// Rewrites `Player` in the copied fixture, which is how every evolution test
/// makes its change.
fn rewrite_player(directory: &Path, fields: &str) {
    let source = read(directory, "src/models/player.rs");
    let start = source.find("pub struct Player {").expect("Player");
    let end = source[start..].find("\n}\n").expect("end of Player") + start;
    let replaced = format!("pub struct Player {{\n{fields}");
    std::fs::write(
        directory.join("src/models/player.rs"),
        format!("{}{replaced}{}", &source[..start], &source[end..]),
    )
    .expect("write player.rs");
}

const PLAYER_V1: &str = "\
    #[network(u32)]\n    #[codec(edge)]\n    pub id: u32,\n\n\
    #[network(f32)]\n    #[codec(edge)]\n    pub x: f32,\n\n\
    #[network(f32)]\n    #[codec(edge)]\n    pub y: f32,\n";

// ==================================================================== generate

#[test]
fn generate_writes_one_file_per_model_per_codec() {
    let directory = project("generate");
    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success(), "{}", stderr(&output));

    for path in [
        "src/generated/mod.rs",
        "src/generated/runtime.rs",
        "src/generated/handshake.rs",
        "src/generated/player_edge.rs",
        "src/generated/player_info_edge.rs",
        "src/generated/team_edge.rs",
        "src/generated/device_state_edge.rs",
        "src/generated/device_state_unity.rs",
        "src/generated/telemetry_orange_pi.rs",
        ".cyclone/schema.json",
        ".cyclone/build-graph.json",
    ] {
        assert!(directory.join(path).exists(), "{path} was not written");
    }

    // Not one file holding everything, the way `cyclonec_old` wrote it.
    assert!(!directory.join("src/generated/cyclone.codec.rs").exists());
}

/// Every generated file is a module a user can actually reach: named like an
/// identifier, declared by `mod.rs`, and importing everything it names -
/// starting with the model it encodes.
#[test]
fn the_generated_tree_is_a_module_tree() {
    let directory = project("module-tree");
    cyclonec(&directory, &["generate", "-q"]);

    let root = read(&directory, "src/generated/mod.rs");
    for entry in std::fs::read_dir(directory.join("src/generated")).expect("read generated") {
        let name = entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .into_owned();
        let module = name.strip_suffix(".rs").expect("a .rs file");

        // `player.edge.rs` could never be reached by `mod`: a dot is not part
        // of an identifier.
        assert!(
            module
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "{name} is not a Rust module name"
        );
        if module != "mod" {
            assert!(
                root.contains(&format!("pub mod {module};\n")),
                "{module} is not declared in mod.rs:\n{root}"
            );
        }
    }

    let codec = read(&directory, "src/generated/player_edge.rs");
    assert!(
        codec.contains("use super::runtime::{DecodeError, Reader, Writer};\n"),
        "{codec}"
    );
    assert!(
        codec.contains("use crate::models::player::Player;\n"),
        "{codec}"
    );

    // A project uses the codecs it needs and no more. A warning in a file whose
    // header says DO NOT EDIT is a warning nobody can act on.
    for path in [
        "src/generated/mod.rs",
        "src/generated/runtime.rs",
        "src/generated/handshake.rs",
    ] {
        let text = read(&directory, path);
        assert!(
            text.contains("#![allow(dead_code, unused_imports)]\n"),
            "{path} does not silence its own warnings"
        );
    }
    assert!(
        codec.contains("#![allow(dead_code, unused_imports)]\n"),
        "{codec}"
    );

    let team = read(&directory, "src/generated/team_edge.rs");
    assert!(
        team.contains("use super::player_info_edge::PlayerInfoEdgeCodec;\n"),
        "{team}"
    );
}

/// Where the models live is the one thing the generator has to be told rather
/// than assume. `cyclone.toml` says it here; `--model-path` overrides that.
#[test]
fn the_model_path_can_be_overridden() {
    let directory = project("model-path");
    cyclonec(
        &directory,
        &["generate", "-q", "--model-path", "crate::schema::wire"],
    );

    let codec = read(&directory, "src/generated/player_edge.rs");
    assert!(
        codec.contains("use crate::schema::wire::Player;\n"),
        "{codec}"
    );
}

/// The tree changed shape once already (one `include!`d file to a module tree).
/// A root left over from the old shape is exactly the kind of file somebody
/// finds later and tries to use, so it goes.
#[test]
fn a_root_from_an_older_layout_is_removed() {
    let directory = project("old-root");
    cyclonec(&directory, &["generate", "-q"]);

    // Pretend the previous run wrote a `cyclone.rs` root, the way 0.2.0-dev did.
    let stale = directory.join("src/generated/cyclone.rs");
    std::fs::write(
        &stale,
        "// GENERATED BY cyclonec\ninclude!(\"runtime.rs\");\n",
    )
    .expect("write");
    let graph = read(&directory, ".cyclone/build-graph.json");
    std::fs::write(
        directory.join(".cyclone/build-graph.json"),
        graph.replace(
            "\"path\": \"src/generated/mod.rs\"",
            "\"path\": \"src/generated/cyclone.rs\"",
        ),
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stale.exists(), "the old root is still there");
}

#[test]
fn every_generated_file_carries_the_header() {
    let directory = project("header");
    cyclonec(&directory, &["generate"]);

    let text = read(&directory, "src/generated/player_edge.rs");
    for line in [
        "// GENERATED BY cyclonec\n",
        "// DO NOT EDIT MANUALLY\n",
        "// source: src/models/player.rs\n",
        "// model: Player\n",
        "// codec: edge\n",
        "// fingerprint: sha256:",
        "// cyclonec-version: ",
        "// generated-at: ",
    ] {
        assert!(text.contains(line), "missing {line:?} in\n{text}");
    }
}

/// Two runs of the same source produce the same bytes - the timestamp aside,
/// which is why an unchanged file is not rewritten at all.
#[test]
fn generating_twice_changes_nothing() {
    let directory = project("determinism");
    cyclonec(&directory, &["generate"]);
    let first = read(&directory, "src/generated/player_edge.rs");
    let schema = read(&directory, ".cyclone/schema.json");

    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success());
    assert_eq!(read(&directory, "src/generated/player_edge.rs"), first);
    assert_eq!(read(&directory, ".cyclone/schema.json"), schema);
    // Nothing was written, so nothing was reported.
    assert!(!stderr(&output).contains("src/generated/player_edge.rs"));
}

/// Two runs an hour apart are still two runs of the same schema. Nothing may
/// depend on the clock - not the file contents, and not the digests in the
/// build graph.
#[test]
fn a_later_run_of_an_unchanged_schema_is_still_up_to_date() {
    let directory = project("later-run");
    cyclonec(&directory, &["generate"]);

    // What the tree would look like if it had been generated an hour ago.
    for path in [
        "src/generated/mod.rs",
        "src/generated/runtime.rs",
        "src/generated/handshake.rs",
        "src/generated/player_edge.rs",
    ] {
        let text = read(&directory, path);
        let aged: String = text
            .lines()
            .map(|line| {
                if line.starts_with("// generated-at: ") {
                    "// generated-at: 2001-09-09T01:46:40Z".to_owned()
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<String>>()
            .join("\n");
        std::fs::write(directory.join(path), format!("{aged}\n")).expect("write");
    }

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(
        output.status.success(),
        "a clock is not a schema change:\n{}",
        stderr(&output)
    );
}

#[test]
fn the_cli_overrides_cyclone_toml() {
    let directory = project("cli-over-config");
    let output = cyclonec(&directory, &["generate", "--out", "elsewhere"]);
    assert!(output.status.success(), "{}", stderr(&output));

    assert!(directory.join("elsewhere/mod.rs").exists());
    assert!(!directory.join("src/generated").exists());
}

/// Generating somewhere else for a moment must not delete the tree the project
/// actually uses. Only files inside the directory this run writes to are ever
/// removed.
#[test]
fn generating_elsewhere_leaves_the_real_tree_alone() {
    let directory = project("out-override");
    cyclonec(&directory, &["generate", "-q"]);
    assert!(directory.join("src/generated/player_edge.rs").exists());

    let output = cyclonec(&directory, &["generate", "-q", "--out", "elsewhere"]);
    assert!(output.status.success(), "{}", stderr(&output));

    assert!(directory.join("elsewhere/player_edge.rs").exists());
    assert!(
        directory.join("src/generated/player_edge.rs").exists(),
        "a one-off --out deleted the project's generated tree"
    );
}

#[test]
fn check_passes_when_the_tree_is_current_and_fails_when_it_is_not() {
    let directory = project("check");
    cyclonec(&directory, &["generate"]);

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(output.status.success(), "{}", stderr(&output));

    rewrite_player(
        &directory,
        &format!("{PLAYER_V1}\n    #[network(u32)]\n    #[codec(edge)]\n    pub level: u32,\n"),
    );
    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(!output.status.success(), "a stale tree must fail --check");
    assert!(stderr(&output).contains("stale:"), "{}", stderr(&output));
    // --check writes nothing.
    assert!(!read(&directory, "src/generated/player_edge.rs").contains("level"));
}

/// A codec whose model is gone leaves no file behind.
#[test]
fn a_removed_model_takes_its_generated_file_with_it() {
    let directory = project("obsolete");
    cyclonec(&directory, &["generate"]);
    assert!(directory.join("src/generated/team_edge.rs").exists());

    let source = read(&directory, "src/models/player.rs");
    let cut = source.find("/// Composites").expect("Team");
    std::fs::write(directory.join("src/models/player.rs"), &source[..cut]).expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!directory.join("src/generated/team_edge.rs").exists());
    assert!(!read(&directory, "src/generated/mod.rs").contains("team_edge"));
}

// ====================================================================== errors

#[test]
fn a_network_field_without_a_type_is_reported_with_its_line() {
    let directory = project("bad-field");
    std::fs::write(
        directory.join("src/models/broken.rs"),
        "#[network]\n#[codec(edge)]\nstruct Broken {\n    #[network]\n    #[codec(edge)]\n    id: u32,\n}\n",
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("broken.rs:4: #[network] field requires a network type"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_nested_field_routed_into_a_codec_the_referenced_model_lacks_is_reported() {
    let directory = project("bad-nesting");
    std::fs::write(
        directory.join("src/models/nested.rs"),
        "#[network]\n#[codec(orange_pi)]\nstruct Outer {\n    #[network(PlayerInfo)]\n    \
         #[codec(orange_pi)]\n    info: PlayerInfo,\n}\n",
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("PlayerInfoOrangePiCodec"),
        "{}",
        stderr(&output)
    );
    // Nothing was written: the schema failed before rendering.
    assert!(!directory.join("src/generated").exists());
}

// =============================================================== compatibility

#[test]
fn generate_reports_an_append_as_compatible_and_still_generates() {
    let directory = project("append");
    cyclonec(&directory, &["generate"]);

    rewrite_player(
        &directory,
        &format!("{PLAYER_V1}\n    #[network(u32)]\n    #[codec(edge)]\n    pub level: u32,\n"),
    );
    let output = cyclonec(&directory, &["generate"]);

    assert!(output.status.success());
    let report = stdout(&output);
    assert!(report.contains("Player.edge:"), "{report}");
    assert!(report.contains("+ level:u32 at index 3"), "{report}");
    assert!(report.contains("COMPATIBLE: append-only"), "{report}");
    assert!(read(&directory, "src/generated/player_edge.rs").contains("value.level"));
}

/// The one rule the brief is most explicit about: a breaking change is
/// reported, loudly, and generated anyway.
#[test]
fn generate_never_fails_because_of_a_breaking_change() {
    let directory = project("breaking");
    cyclonec(&directory, &["generate"]);

    // x and y swapped: same types, same bytes, different meaning.
    rewrite_player(
        &directory,
        "    #[network(u32)]\n    #[codec(edge)]\n    pub id: u32,\n\n\
             #[network(f32)]\n    #[codec(edge)]\n    pub y: f32,\n\n\
             #[network(f32)]\n    #[codec(edge)]\n    pub x: f32,\n",
    );
    let output = cyclonec(&directory, &["generate"]);

    assert!(
        output.status.success(),
        "a breaking change must not fail `generate`"
    );
    let report = stdout(&output);
    assert!(report.contains("field[1]:"), "{report}");
    assert!(report.contains("old: x:f32"), "{report}");
    assert!(report.contains("new: y:f32"), "{report}");
    assert!(report.contains("BREAKING: field order changed"), "{report}");
}

#[test]
fn generate_says_nothing_changed_when_nothing_changed() {
    let directory = project("unchanged");
    cyclonec(&directory, &["generate"]);
    let output = cyclonec(&directory, &["generate"]);
    assert!(
        stdout(&output).contains("✓ Player.edge unchanged"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn compat_exits_one_on_a_breaking_change_and_zero_otherwise() {
    let directory = project("compat");
    cyclonec(&directory, &["generate"]);
    std::fs::copy(
        directory.join(".cyclone/schema.json"),
        directory.join("base.json"),
    )
    .expect("copy base");

    // Appending is compatible.
    rewrite_player(
        &directory,
        &format!("{PLAYER_V1}\n    #[network(u32)]\n    #[codec(edge)]\n    pub level: u32,\n"),
    );
    let output = cyclonec(&directory, &["compat", "--base", "base.json"]);
    assert!(output.status.success(), "{}", stdout(&output));
    assert!(
        stdout(&output).trim_end().ends_with("COMPATIBLE"),
        "{}",
        stdout(&output)
    );

    // Deleting is not.
    rewrite_player(
        &directory,
        "    #[network(u32)]\n    #[codec(edge)]\n    pub id: u32,\n\n\
             #[network(f32)]\n    #[codec(edge)]\n    pub x: f32,\n",
    );
    let output = cyclonec(&directory, &["compat", "--base", "base.json"]);
    assert!(!output.status.success());
    assert!(stdout(&output).contains("BREAKING"), "{}", stdout(&output));
    assert!(
        stdout(&output).contains("- y:f32 at index 2"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn compat_compares_two_named_schemas_without_reading_source() {
    let directory = project("compat-two-files");
    cyclonec(&directory, &["generate"]);
    std::fs::copy(
        directory.join(".cyclone/schema.json"),
        directory.join("base.json"),
    )
    .expect("copy");

    let output = cyclonec(
        &directory,
        &[
            "compat",
            "--base",
            "base.json",
            "--head",
            ".cyclone/schema.json",
        ],
    );
    assert!(output.status.success());
    assert!(
        stdout(&output).trim_end().ends_with("CURRENT"),
        "{}",
        stdout(&output)
    );
}

// ========================================================================== ci

/// A git repository with the fixture committed on `develop`, and the change
/// under test on a branch - the shape a pull request actually has.
fn repository(name: &str, change: Option<&str>) -> Option<PathBuf> {
    let directory = project(name);
    let git = |arguments: &[&str]| {
        Command::new("git")
            .current_dir(&directory)
            .args(arguments)
            .output()
    };

    if git(&["init", "-b", "develop"]).is_err() {
        return None;
    }
    let _ = git(&["config", "user.email", "cyclone@example.test"]);
    let _ = git(&["config", "user.name", "Cyclone"]);

    cyclonec(&directory, &["generate", "-q"]);
    let _ = git(&["add", "-A"]);
    let _ = git(&["commit", "-m", "schema v1"]);

    if let Some(fields) = change {
        let _ = git(&["checkout", "-b", "feature/foo"]);
        rewrite_player(&directory, fields);
        cyclonec(&directory, &["generate", "-q"]);
        let _ = git(&["add", "-A"]);
        let _ = git(&["commit", "-m", "schema v2"]);
    }

    Some(directory)
}

#[test]
fn ci_compares_against_the_named_target_branch() {
    let Some(directory) = repository(
        "ci-compatible",
        Some(&format!(
            "{PLAYER_V1}\n    #[network(u32)]\n    #[codec(edge)]\n    pub level: u32,\n"
        )),
    ) else {
        return;
    };

    let output = cyclonec(&directory, &["ci", "--base-ref", "develop"]);
    let report = stdout(&output);
    assert!(output.status.success(), "{report}{}", stderr(&output));
    assert!(report.contains("matches the source"), "{report}");
    assert!(report.contains("COMPATIBLE"), "{report}");
}

#[test]
fn ci_fails_on_a_breaking_change_against_the_target_branch() {
    let Some(directory) = repository(
        "ci-breaking",
        Some(
            "    #[network(u32)]\n    #[codec(edge)]\n    pub id: u32,\n\n\
                 #[network(u64)]\n    #[codec(edge)]\n    pub x: f32,\n\n\
                 #[network(f32)]\n    #[codec(edge)]\n    pub y: f32,\n",
        ),
    ) else {
        return;
    };

    let output = cyclonec(&directory, &["ci", "--base-ref", "develop"]);
    let report = stdout(&output);
    assert!(!output.status.success(), "{report}");
    assert!(report.contains("BREAKING"), "{report}");
    assert!(report.contains("wire type changed"), "{report}");
}

/// A schema that was not regenerated after a source change makes every later
/// comparison meaningless, so it fails first and says so.
#[test]
fn ci_fails_when_the_committed_schema_does_not_match_the_source() {
    let Some(directory) = repository("ci-stale", None) else {
        return;
    };
    rewrite_player(
        &directory,
        &format!("{PLAYER_V1}\n    #[network(u32)]\n    #[codec(edge)]\n    pub level: u32,\n"),
    );

    let output = cyclonec(&directory, &["ci", "--base-ref", "develop"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("does not match the source"),
        "{}",
        stderr(&output)
    );
}

/// The baseline is never assumed. `main` does not exist in this repository, and
/// nothing pretends otherwise.
#[test]
fn ci_requires_a_base_ref() {
    let directory = project("ci-no-ref");
    let output = cyclonec(&directory, &["ci"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("--base-ref"),
        "{}",
        stderr(&output)
    );
}

// ===================================================================== inspect

#[test]
fn inspect_decodes_a_packet_through_a_named_schema() {
    let directory = project("inspect");
    cyclonec(&directory, &["generate", "-q"]);

    let output = inspect(
        &directory,
        &[
            "--schema",
            ".cyclone/schema.json",
            "--message",
            "Player",
            "--hex",
            "64000000 00002841 0000A041",
        ],
    );
    let report = stdout(&output);
    assert!(output.status.success(), "{}", stderr(&output));

    assert!(report.contains("Player.edge"), "{report}");
    assert!(report.contains("id      : u32 = 100"), "{report}");
    assert!(report.contains("offset: 0"), "{report}");
    assert!(report.contains("bytes: 64 00 00 00"), "{report}");
    assert!(report.contains("x       : f32 = 10.5"), "{report}");
    assert!(report.contains("offset: 4"), "{report}");
    assert!(report.contains("y       : f32 = 20.0"), "{report}");
}

#[test]
fn inspect_reads_a_binary_file_and_reports_trailing_bytes() {
    let directory = project("inspect-file");
    cyclonec(&directory, &["generate", "-q"]);
    std::fs::write(
        directory.join("packet.bin"),
        [
            0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x28, 0x41, 0x00, 0x00, 0xA0, 0x41, 0xFF, 0xFF,
        ],
    )
    .expect("write packet");

    let output = inspect(
        &directory,
        &[
            "--schema",
            ".cyclone/schema.json",
            "--message",
            "Player.edge",
            "--file",
            "packet.bin",
        ],
    );
    let report = stdout(&output);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(report.contains("2 trailing byte(s)"), "{report}");
}

#[test]
fn inspect_shows_an_absent_field_rather_than_inventing_one() {
    let directory = project("inspect-skew");
    cyclonec(&directory, &["generate", "-q"]);

    let output = inspect(
        &directory,
        &[
            "--schema",
            ".cyclone/schema.json",
            "--message",
            "Player",
            "--hex",
            "64000000",
        ],
    );
    let report = stdout(&output);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(report.contains("absent"), "{report}");
}

#[test]
fn inspect_reports_a_truncated_field_as_an_error() {
    let directory = project("inspect-truncated");
    cyclonec(&directory, &["generate", "-q"]);

    let output = inspect(
        &directory,
        &[
            "--schema",
            ".cyclone/schema.json",
            "--message",
            "Player",
            "--hex",
            "6400000000",
        ],
    );
    assert!(!output.status.success());
    assert!(stderr(&output).contains("truncated"), "{}", stderr(&output));
}

#[test]
fn inspect_needs_a_codec_when_the_model_has_more_than_one() {
    let directory = project("inspect-codec");
    cyclonec(&directory, &["generate", "-q"]);

    let output = inspect(
        &directory,
        &[
            "--schema",
            ".cyclone/schema.json",
            "--message",
            "DeviceState",
            "--hex",
            "2A000000",
        ],
    );
    assert!(!output.status.success());
    assert!(stderr(&output).contains("--codec"), "{}", stderr(&output));

    let output = inspect(
        &directory,
        &[
            "--schema",
            ".cyclone/schema.json",
            "--message",
            "DeviceState",
            "--codec",
            "edge",
            "--hex",
            "2A000000 0000AC41",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("temperature"),
        "{}",
        stdout(&output)
    );
}

/// The schema is named, never guessed - and a fingerprint can be demanded, so
/// that a packet captured from one build cannot be quietly read through
/// another.
#[test]
fn inspect_can_be_told_which_fingerprint_to_expect() {
    let directory = project("inspect-fingerprint");
    cyclonec(&directory, &["generate", "-q"]);

    let output = inspect(
        &directory,
        &[
            "--schema",
            ".cyclone/schema.json",
            "--message",
            "Player",
            "--hex",
            "64000000",
            "--expect",
            "0xDEADBEEFDEADBEEF",
        ],
    );
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("fingerprint mismatch"),
        "{}",
        stderr(&output)
    );
}

// ================================================================== the schema

#[test]
fn the_schema_carries_fingerprints_sources_and_messages() {
    let directory = project("schema-json");
    cyclonec(&directory, &["generate", "-q"]);
    let text = read(&directory, ".cyclone/schema.json");

    assert!(text.contains("\"schema_version\": 1"), "{text}");
    assert!(text.contains("\"fingerprint\": \"sha256:"), "{text}");
    assert!(
        text.contains("\"source\": \"src/models/player.rs\""),
        "{text}"
    );
    assert!(text.contains("\"messages\""), "{text}");
    assert!(text.contains("\"codecs\""), "{text}");
}

#[test]
fn the_build_graph_maps_a_source_to_what_was_generated_from_it() {
    let directory = project("build-graph");
    cyclonec(&directory, &["generate", "-q"]);
    let text = read(&directory, ".cyclone/build-graph.json");

    assert!(text.contains("\"src/models/player.rs\""), "{text}");
    assert!(text.contains("\"src/generated/player_edge.rs\""), "{text}");
    assert!(text.contains("\"model\": \"Player\""), "{text}");
    assert!(text.contains("\"codec\": \"edge\""), "{text}");
    assert!(text.contains("\"sha256\""), "{text}");
}

/// The schema is derived from source every run. An out-of-date `schema.json`
/// on disk changes what is *reported*, never what is generated.
#[test]
fn a_stale_schema_json_does_not_decide_what_is_generated() {
    let directory = project("schema-is-not-input");
    cyclonec(&directory, &["generate", "-q"]);

    let schema = read(&directory, ".cyclone/schema.json");
    std::fs::write(
        directory.join(".cyclone/schema.json"),
        schema.replace("\"name\": \"x\"", "\"name\": \"nonsense\""),
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let generated = read(&directory, "src/generated/player_edge.rs");
    assert!(generated.contains("value.x"), "{generated}");
    assert!(!generated.contains("nonsense"), "{generated}");
}

#[test]
fn the_generated_tree_matches_the_committed_fixture() {
    // `tests/generated.rs` compiles the committed tree; this checks it is still
    // what the generator would write today.
    let directory = project("fixture-is-current");
    cyclonec(&directory, &["generate", "-q"]);

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for path in [
        "src/generated/player_edge.rs",
        "src/generated/handshake.rs",
        ".cyclone/schema.json",
    ] {
        let fresh = read(&directory, path);
        let committed = std::fs::read_to_string(fixtures.join(path)).expect("committed fixture");
        assert!(
            same_but_for_timestamp(&fresh, &committed),
            "{path} in tests/fixtures/ is out of date - regenerate it"
        );
    }
}

/// The same rule `cyclonec` itself applies when deciding whether to rewrite a
/// file: only the `generated-at:` line may differ. Checked against both
/// spellings (`//` for Rust, Go and C#; `#` for GDScript, whose only comment
/// syntax that is) so one helper serves every backend's fixture comparison.
fn same_but_for_timestamp(left: &str, right: &str) -> bool {
    fn timestamp_line(line: &str) -> bool {
        line.starts_with("// generated-at: ") || line.starts_with("# generated-at: ")
    }

    left.lines()
        .zip(right.lines())
        .all(|(one, other)| one == other || (timestamp_line(one) && timestamp_line(other)))
        && left.lines().count() == right.lines().count()
}

// ========================================================================= Go

/// A clean copy of `tests/fixtures-go/` - the Go counterpart of [`project`]:
/// `go.mod`, `cyclone.toml`, and the annotated schema, in a directory of its
/// own so a test can edit it without disturbing the committed fixture.
fn go_project(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("src/models")).expect("create project");

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-go");
    for file in ["cyclone.toml", "go.mod"] {
        std::fs::copy(fixtures.join(file), directory.join(file))
            .unwrap_or_else(|error| panic!("copy {file}: {error}"));
    }
    for entry in std::fs::read_dir(fixtures.join("src/models")).expect("read fixtures") {
        let path = entry.expect("entry").path();
        std::fs::copy(
            &path,
            directory
                .join("src/models")
                .join(path.file_name().expect("name")),
        )
        .expect("copy schema");
    }

    directory
}

#[test]
fn go_generate_writes_one_file_per_model_per_codec_in_one_shared_package() {
    let directory = go_project("go-generate");
    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success(), "{}", stderr(&output));

    for file in [
        "src/generated/runtime.go",
        "src/generated/handshake.go",
        "src/generated/player_edge.go",
        "src/generated/player_unity.go",
        "src/generated/player_info_edge.go",
        "src/generated/team_edge.go",
    ] {
        let text = read(&directory, file);
        assert!(
            text.starts_with("// GENERATED BY cyclonec\n"),
            "{file}: {text}"
        );
        assert!(text.contains("package generated\n"), "{file}: {text}");
    }

    // The model type is imported and referenced qualified - the codec never
    // creates a type of its own.
    let codec = read(&directory, "src/generated/player_edge.go");
    assert!(codec.contains("models\""), "{codec}");
    assert!(codec.contains("*models.Player"), "{codec}");
    // Codecs share one package, so a nested codec is never imported.
    assert!(!codec.contains("PlayerInfoEdgeCodec\""), "{codec}");
}

#[test]
fn go_check_passes_when_current_and_fails_when_stale() {
    let directory = go_project("go-check");
    cyclonec(&directory, &["generate"]);

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let source = read(&directory, "src/models/player.go");
    std::fs::write(
        directory.join("src/models/player.go"),
        source.replace(
            "Unrouted uint32 `cyclone:\"u32\"`",
            "Unrouted uint32 `cyclone:\"u32\"`\n\tLevel uint32 `cyclone:\"u32\" codec:\"edge\"`",
        ),
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(!output.status.success(), "a stale tree must fail --check");
    assert!(stderr(&output).contains("stale:"), "{}", stderr(&output));
}

#[test]
fn go_backend_refuses_array_of_array_rather_than_generate_it_wrong() {
    let directory = go_project("go-nested-array");
    std::fs::write(
        directory.join("src/models/grid.go"),
        "package models\n\n//cyclone:model codec=edge\ntype Grid struct {\n\
         \tRows [][]uint8 `cyclone:\"Array<Array<u8>>\" codec:\"edge\"`\n}\n",
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(!output.status.success(), "nested arrays must be refused");
    assert!(
        stderr(&output).contains("Array<Array<T>>"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn go_backend_requires_go_mod_at_the_project_root() {
    let directory = go_project("go-no-mod");
    std::fs::remove_file(directory.join("go.mod")).expect("remove go.mod");

    let output = cyclonec(&directory, &["generate"]);
    assert!(!output.status.success(), "no go.mod must be refused");
    assert!(stderr(&output).contains("go.mod"), "{}", stderr(&output));
}

#[test]
fn mixed_rust_and_go_sources_in_one_run_are_rejected() {
    let directory = go_project("go-mixed");
    // A Rust model dropped into the same `--src` tree as the Go fixture.
    std::fs::write(
        directory.join("src/models/extra.rs"),
        "#[network]\n#[codec(edge)]\nstruct Extra {\n    #[network(u32)]\n    #[codec(edge)]\n    id: u32,\n}\n",
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(
        !output.status.success(),
        "mixing languages in one run must be refused"
    );
    let message = stderr(&output);
    assert!(
        message.contains("Rust") && message.contains("Go"),
        "{message}"
    );
}

#[test]
fn the_go_generated_tree_matches_the_committed_fixture() {
    let directory = go_project("go-fixture-is-current");
    cyclonec(&directory, &["generate", "-q"]);

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-go");
    for path in [
        "src/generated/player_edge.go",
        "src/generated/handshake.go",
        ".cyclone/schema.json",
    ] {
        let fresh = read(&directory, path);
        let committed = std::fs::read_to_string(fixtures.join(path)).expect("committed fixture");
        assert!(
            same_but_for_timestamp(&fresh, &committed),
            "{path} in tests/fixtures-go/ is out of date - regenerate it"
        );
    }
}

// ========================================================================= C#

/// A clean copy of `tests/fixtures-cs/` - the C# counterpart of [`project`]
/// and [`go_project`]: `cyclone.toml` and the annotated schema, in a
/// directory of its own so a test can edit it without disturbing the
/// committed fixture. Unlike Go, C# needs no project file of its own - a
/// namespace is self-declared, so there is nothing here to copy beyond the
/// schema and `cyclone.toml`.
fn cs_project(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("src/models")).expect("create project");

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-cs");
    std::fs::copy(
        fixtures.join("cyclone.toml"),
        directory.join("cyclone.toml"),
    )
    .expect("copy cyclone.toml");
    for entry in std::fs::read_dir(fixtures.join("src/models")).expect("read fixtures") {
        let path = entry.expect("entry").path();
        std::fs::copy(
            &path,
            directory
                .join("src/models")
                .join(path.file_name().expect("name")),
        )
        .expect("copy schema");
    }

    directory
}

#[test]
fn cs_generate_writes_one_file_per_model_per_codec_in_one_shared_namespace() {
    let directory = cs_project("cs-generate");
    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success(), "{}", stderr(&output));

    for file in [
        "src/generated/runtime.cs",
        "src/generated/handshake.cs",
        "src/generated/player_edge.cs",
        "src/generated/player_unity.cs",
        "src/generated/player_info_edge.cs",
        "src/generated/team_edge.cs",
    ] {
        let text = read(&directory, file);
        assert!(
            text.starts_with("// GENERATED BY cyclonec\n"),
            "{file}: {text}"
        );
        assert!(text.contains("namespace Generated\n"), "{file}: {text}");
    }

    // The model type is qualified by its own namespace - the codec never
    // creates a type of its own.
    let codec = read(&directory, "src/generated/player_edge.cs");
    assert!(codec.contains("Models.Player value"), "{codec}");
    // Same namespace as the generated tree: a nested codec is never qualified.
    let team = read(&directory, "src/generated/team_edge.cs");
    assert!(
        team.contains("PlayerInfoEdgeCodec.Encode(writer, value.Captain);"),
        "{team}"
    );
    // No `using` is ever written - see generator::csharp's module docs.
    assert!(!codec.contains("using "), "{codec}");
}

#[test]
fn cs_check_passes_when_current_and_fails_when_stale() {
    let directory = cs_project("cs-check");
    cyclonec(&directory, &["generate"]);

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let source = read(&directory, "src/models/Player.cs");
    std::fs::write(
        directory.join("src/models/Player.cs"),
        source.replace(
            "public uint Unrouted { get; set; }",
            "public uint Unrouted { get; set; }\n\n    [Network(\"u32\")]\n    [Codec(\"edge\")]\n    public uint Level { get; set; }",
        ),
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(!output.status.success(), "a stale tree must fail --check");
    assert!(stderr(&output).contains("stale:"), "{}", stderr(&output));
}

#[test]
fn cs_model_path_overrides_the_namespace_the_source_declares() {
    let directory = cs_project("cs-model-path");
    cyclonec(&directory, &["generate", "-q", "--model-path", "Game.Wire"]);

    let codec = read(&directory, "src/generated/player_edge.cs");
    assert!(codec.contains("Game.Wire.Player value"), "{codec}");
}

#[test]
fn mixed_rust_and_csharp_sources_in_one_run_are_rejected() {
    let directory = cs_project("cs-mixed");
    // A Rust model dropped into the same `--src` tree as the C# fixture.
    std::fs::write(
        directory.join("src/models/extra.rs"),
        "#[network]\n#[codec(edge)]\nstruct Extra {\n    #[network(u32)]\n    #[codec(edge)]\n    id: u32,\n}\n",
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(
        !output.status.success(),
        "mixing languages in one run must be refused"
    );
    let message = stderr(&output);
    assert!(
        message.contains("Rust") && message.contains("C#"),
        "{message}"
    );
}

#[test]
fn the_cs_generated_tree_matches_the_committed_fixture() {
    let directory = cs_project("cs-fixture-is-current");
    cyclonec(&directory, &["generate", "-q"]);

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-cs");
    for path in [
        "src/generated/player_edge.cs",
        "src/generated/handshake.cs",
        ".cyclone/schema.json",
    ] {
        let fresh = read(&directory, path);
        let committed = std::fs::read_to_string(fixtures.join(path)).expect("committed fixture");
        assert!(
            same_but_for_timestamp(&fresh, &committed),
            "{path} in tests/fixtures-cs/ is out of date - regenerate it"
        );
    }
}

// ================================================================== GDScript

/// A clean copy of `tests/fixtures-gd/` - the GDScript counterpart of
/// [`cs_project`]/[`go_project`]: `cyclone.toml` and the annotated models, in
/// a directory of its own so a test can edit it without disturbing the
/// committed fixture. One model per file, since a `.gd` file may declare only
/// one `class_name` - see `tests/fixtures-gd/src/models/`.
fn gd_project(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("src/models")).expect("create project");

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-gd");
    std::fs::copy(
        fixtures.join("cyclone.toml"),
        directory.join("cyclone.toml"),
    )
    .expect("copy cyclone.toml");
    for entry in std::fs::read_dir(fixtures.join("src/models")).expect("read fixtures") {
        let path = entry.expect("entry").path();
        std::fs::copy(
            &path,
            directory
                .join("src/models")
                .join(path.file_name().expect("name")),
        )
        .expect("copy schema");
    }

    directory
}

#[test]
fn gd_generate_writes_one_file_per_model_per_codec_with_no_qualification_at_all() {
    let directory = gd_project("gd-generate");
    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success(), "{}", stderr(&output));

    for (file, class_name) in [
        ("src/generated/runtime.gd", "CycloneRuntime"),
        ("src/generated/handshake.gd", "CycloneHandshake"),
        ("src/generated/player_edge.gd", "PlayerEdgeCodec"),
        ("src/generated/player_unity.gd", "PlayerUnityCodec"),
        ("src/generated/player_info_edge.gd", "PlayerInfoEdgeCodec"),
        ("src/generated/team_edge.gd", "TeamEdgeCodec"),
    ] {
        let text = read(&directory, file);
        assert!(
            text.starts_with("# GENERATED BY cyclonec\n"),
            "{file}: {text}"
        );
        assert!(
            text.contains(&format!("class_name {class_name}\n")),
            "{file}: {text}"
        );
    }

    // The model type is named bare - never qualified, never a DTO of its own.
    let codec = read(&directory, "src/generated/player_edge.gd");
    assert!(codec.contains("value: Player)"), "{codec}");
    // A nested codec is called bare too - there is nothing to import or
    // qualify in GDScript, unlike Go's package or C#'s namespace.
    let team = read(&directory, "src/generated/team_edge.gd");
    assert!(
        team.contains("PlayerInfoEdgeCodec.encode(writer, value.captain)"),
        "{team}"
    );
    assert!(!codec.contains("preload("), "{codec}");
}

#[test]
fn gd_check_passes_when_current_and_fails_when_stale() {
    let directory = gd_project("gd-check");
    cyclonec(&directory, &["generate"]);

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let source = read(&directory, "src/models/player.gd");
    std::fs::write(
        directory.join("src/models/player.gd"),
        source.replace(
            "var cache: String = \"\"",
            "var cache: String = \"\"\n\n# cyclone:u32 codec=edge\nvar level: int = 0",
        ),
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(!output.status.success(), "a stale tree must fail --check");
    assert!(stderr(&output).contains("stale:"), "{}", stderr(&output));
}

#[test]
fn gd_model_path_has_no_effect_gdscript_needs_nothing_to_override() {
    let directory = gd_project("gd-model-path");
    cyclonec(&directory, &["generate", "-q"]);
    let without = read(&directory, "src/generated/player_edge.gd");

    let with_override = gd_project("gd-model-path-override");
    cyclonec(
        &with_override,
        &["generate", "-q", "--model-path", "Game.Wire"],
    );
    let with = read(&with_override, "src/generated/player_edge.gd");

    assert!(
        same_but_for_timestamp(&without, &with),
        "--model-path must have no effect on the GDScript backend:\n{without}\n---\n{with}"
    );
}

#[test]
fn mixed_rust_and_gdscript_sources_in_one_run_are_rejected() {
    let directory = gd_project("gd-mixed");
    // A Rust model dropped into the same `--src` tree as the GDScript fixture.
    std::fs::write(
        directory.join("src/models/extra.rs"),
        "#[network]\n#[codec(edge)]\nstruct Extra {\n    #[network(u32)]\n    #[codec(edge)]\n    id: u32,\n}\n",
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(
        !output.status.success(),
        "mixing languages in one run must be refused"
    );
    let message = stderr(&output);
    assert!(
        message.contains("Rust") && message.contains("GDScript"),
        "{message}"
    );
}

#[test]
fn the_gd_generated_tree_matches_the_committed_fixture() {
    let directory = gd_project("gd-fixture-is-current");
    cyclonec(&directory, &["generate", "-q"]);

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-gd");
    for path in [
        "src/generated/player_edge.gd",
        "src/generated/team_edge.gd",
        "src/generated/handshake.gd",
        ".cyclone/schema.json",
    ] {
        let fresh = read(&directory, path);
        let committed = std::fs::read_to_string(fixtures.join(path)).expect("committed fixture");
        assert!(
            same_but_for_timestamp(&fresh, &committed),
            "{path} in tests/fixtures-gd/ is out of date - regenerate it"
        );
    }
}

// ======================================================================= C++

/// A clean copy of `tests/fixtures-cpp/`'s schema - the C++ counterpart of
/// [`cs_project`]/[`go_project`]: `cyclone.toml` and the annotated models, in
/// a directory of its own so a test can edit it without disturbing the
/// committed fixture. `include/cyclone.h` is not copied: `cyclonec` never
/// resolves a `#include` line, only skips it as a preprocessor directive (see
/// `parser::cpp`'s lexer), so nothing here needs it to exist on disk - only
/// the g++ compile step in CI does.
fn cpp_project(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("src/models")).expect("create project");

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-cpp");
    std::fs::copy(
        fixtures.join("cyclone.toml"),
        directory.join("cyclone.toml"),
    )
    .expect("copy cyclone.toml");
    for entry in std::fs::read_dir(fixtures.join("src/models")).expect("read fixtures") {
        let path = entry.expect("entry").path();
        std::fs::copy(
            &path,
            directory
                .join("src/models")
                .join(path.file_name().expect("name")),
        )
        .expect("copy schema");
    }

    directory
}

#[test]
fn cpp_generate_writes_one_file_per_model_per_codec_in_one_shared_namespace() {
    let directory = cpp_project("cpp-generate");
    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success(), "{}", stderr(&output));

    for file in [
        "src/generated/runtime.hpp",
        "src/generated/handshake.hpp",
        "src/generated/player_edge.hpp",
        "src/generated/player_unity.hpp",
        "src/generated/player_info_edge.hpp",
        "src/generated/team_edge.hpp",
    ] {
        let text = read(&directory, file);
        assert!(
            text.starts_with("// GENERATED BY cyclonec\n"),
            "{file}: {text}"
        );
        assert!(text.contains("#pragma once\n"), "{file}: {text}");
        assert!(text.contains("namespace generated {\n"), "{file}: {text}");
    }

    // The model type is qualified by its own namespace, and the header that
    // declares it is `#include`d by its own source path.
    let codec = read(&directory, "src/generated/player_edge.hpp");
    assert!(codec.contains("const ::models::Player& value"), "{codec}");
    assert!(
        codec.contains("#include \"src/models/player.hpp\"\n"),
        "{codec}"
    );
    // A nested codec is called bare - same generated namespace, so it is
    // never qualified, only `#include`d by its own generated file name.
    let team = read(&directory, "src/generated/team_edge.hpp");
    assert!(
        team.contains("PlayerInfoEdgeCodec::encode(writer, value.Captain);"),
        "{team}"
    );
    assert!(
        team.contains("#include \"player_info_edge.hpp\"\n"),
        "{team}"
    );
}

#[test]
fn cpp_check_passes_when_current_and_fails_when_stale() {
    let directory = cpp_project("cpp-check");
    cyclonec(&directory, &["generate"]);

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let source = read(&directory, "src/models/player.hpp");
    std::fs::write(
        directory.join("src/models/player.hpp"),
        source.replace(
            "uint32_t Unrouted = 0;",
            "uint32_t Unrouted = 0;\n\n    CYCLONE_FIELD(u32)\n    CYCLONE_CODEC(\"edge\")\n    uint32_t Level = 0;",
        ),
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(!output.status.success(), "a stale tree must fail --check");
    assert!(stderr(&output).contains("stale:"), "{}", stderr(&output));
}

#[test]
fn cpp_model_path_overrides_the_namespace_the_source_declares() {
    let directory = cpp_project("cpp-model-path");
    cyclonec(
        &directory,
        &["generate", "-q", "--model-path", "Game::Wire"],
    );

    let codec = read(&directory, "src/generated/player_edge.hpp");
    assert!(
        codec.contains("const ::Game::Wire::Player& value"),
        "{codec}"
    );
    // The `#include` path is never affected by `--model-path` - it is always
    // the model's own physical source location.
    assert!(
        codec.contains("#include \"src/models/player.hpp\"\n"),
        "{codec}"
    );
}

#[test]
fn mixed_rust_and_cpp_sources_in_one_run_are_rejected() {
    let directory = cpp_project("cpp-mixed");
    // A Rust model dropped into the same `--src` tree as the C++ fixture.
    std::fs::write(
        directory.join("src/models/extra.rs"),
        "#[network]\n#[codec(edge)]\nstruct Extra {\n    #[network(u32)]\n    #[codec(edge)]\n    id: u32,\n}\n",
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(
        !output.status.success(),
        "mixing languages in one run must be refused"
    );
    let message = stderr(&output);
    assert!(
        message.contains("Rust") && message.contains("C++"),
        "{message}"
    );
}

#[test]
fn the_cpp_generated_tree_matches_the_committed_fixture() {
    let directory = cpp_project("cpp-fixture-is-current");
    cyclonec(&directory, &["generate", "-q"]);

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-cpp");
    for path in [
        "src/generated/player_edge.hpp",
        "src/generated/team_edge.hpp",
        "src/generated/handshake.hpp",
        ".cyclone/schema.json",
    ] {
        let fresh = read(&directory, path);
        let committed = std::fs::read_to_string(fixtures.join(path)).expect("committed fixture");
        assert!(
            same_but_for_timestamp(&fresh, &committed),
            "{path} in tests/fixtures-cpp/ is out of date - regenerate it"
        );
    }
}

// ========================================================================= C

/// A clean copy of `tests/fixtures-c/`'s schema - the C counterpart of
/// [`cpp_project`]. `include/cyclone.h` is not copied, for the same reason
/// [`cpp_project`]'s doc comment gives: `cyclonec` never resolves a
/// `#include` line, only skips it, so nothing here needs it to exist on disk
/// - only the gcc compile step in CI does.
fn c_project(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("src/models")).expect("create project");

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-c");
    std::fs::copy(
        fixtures.join("cyclone.toml"),
        directory.join("cyclone.toml"),
    )
    .expect("copy cyclone.toml");
    for entry in std::fs::read_dir(fixtures.join("src/models")).expect("read fixtures") {
        let path = entry.expect("entry").path();
        std::fs::copy(
            &path,
            directory
                .join("src/models")
                .join(path.file_name().expect("name")),
        )
        .expect("copy schema");
    }

    directory
}

#[test]
fn c_generate_writes_one_file_per_model_per_codec_plus_arrays_and_free_files() {
    let directory = c_project("c-generate");
    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success(), "{}", stderr(&output));

    for file in [
        "src/generated/runtime.h",
        "src/generated/arrays.h",
        "src/generated/handshake.h",
        "src/generated/player_edge.h",
        "src/generated/player_unity.h",
        "src/generated/player_cyclone.h",
        "src/generated/player_info_edge.h",
        "src/generated/player_info_cyclone.h",
        "src/generated/team_edge.h",
        "src/generated/team_cyclone.h",
    ] {
        let text = read(&directory, file);
        assert!(
            text.starts_with("// GENERATED BY cyclonec\n"),
            "{file}: {text}"
        );
        assert!(text.contains("#pragma once\n"), "{file}: {text}");
        // C has no namespace at all - unlike every other backend's shared
        // file, nothing here opens one.
        assert!(!text.contains("namespace"), "{file}: {text}");
    }

    // A model reference is always `struct Name`, never bare - see
    // `generator::c::struct_type`'s doc comment - and reached by the
    // model's own physical `#include`, exactly like C++.
    let codec = read(&directory, "src/generated/player_edge.h");
    assert!(
        codec.contains(
            "static inline bool PlayerEdgeCodec_encode(CycloneWriter *writer, \
                         const struct Player *value)"
        ),
        "{codec}"
    );
    assert!(
        codec.contains("#include \"src/models/player.h\"\n"),
        "{codec}"
    );

    // A nested codec is called through its own free function, never a
    // method - the same free-function shape every generated function here
    // has.
    let team = read(&directory, "src/generated/team_edge.h");
    assert!(
        team.contains("PlayerInfoEdgeCodec_encode(writer, &value->Captain)"),
        "{team}"
    );
    assert!(team.contains("#include \"player_info_edge.h\"\n"), "{team}");

    // `<Model>_free` lives in its own file, once per model - not once per
    // codec.
    let free = read(&directory, "src/generated/player_cyclone.h");
    assert!(
        free.contains("static inline void Player_free(struct Player *value)"),
        "{free}"
    );
}

#[test]
fn c_check_passes_when_current_and_fails_when_stale() {
    let directory = c_project("c-check");
    cyclonec(&directory, &["generate"]);

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let source = read(&directory, "src/models/player.h");
    std::fs::write(
        directory.join("src/models/player.h"),
        source.replace(
            "int Cache;",
            "int Cache;\n\n    CYCLONE_FIELD(u32)\n    CYCLONE_CODEC(\"edge\")\n    uint32_t Level;",
        ),
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(!output.status.success(), "a stale tree must fail --check");
    assert!(stderr(&output).contains("stale:"), "{}", stderr(&output));
}

#[test]
fn c_model_path_has_no_effect_since_there_is_no_namespace_to_override() {
    let directory = c_project("c-model-path");
    cyclonec(&directory, &["generate", "-q"]);
    let without_override = read(&directory, "src/generated/player_edge.h");

    let with_override = c_project("c-model-path-override");
    cyclonec(
        &with_override,
        &["generate", "-q", "--model-path", "Game::Wire"],
    );
    let overridden = read(&with_override, "src/generated/player_edge.h");

    assert_eq!(without_override, overridden);
    // The `#include` path is, as ever, always the model's own physical
    // source location.
    assert!(
        overridden.contains("#include \"src/models/player.h\"\n"),
        "{overridden}"
    );
}

#[test]
fn mixed_rust_and_c_sources_in_one_run_are_rejected() {
    let directory = c_project("c-mixed");
    // A Rust model dropped into the same `--src` tree as the C fixture.
    std::fs::write(
        directory.join("src/models/extra.rs"),
        "#[network]\n#[codec(edge)]\nstruct Extra {\n    #[network(u32)]\n    #[codec(edge)]\n    id: u32,\n}\n",
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(
        !output.status.success(),
        "mixing languages in one run must be refused"
    );
    let message = stderr(&output);
    assert!(message.contains("Rust, C"), "{message}");
}

#[test]
fn the_c_generated_tree_matches_the_committed_fixture() {
    let directory = c_project("c-fixture-is-current");
    cyclonec(&directory, &["generate", "-q"]);

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-c");
    for path in [
        "src/generated/player_edge.h",
        "src/generated/team_edge.h",
        "src/generated/team_cyclone.h",
        "src/generated/arrays.h",
        "src/generated/handshake.h",
        ".cyclone/schema.json",
    ] {
        let fresh = read(&directory, path);
        let committed = std::fs::read_to_string(fixtures.join(path)).expect("committed fixture");
        assert!(
            same_but_for_timestamp(&fresh, &committed),
            "{path} in tests/fixtures-c/ is out of date - regenerate it"
        );
    }
}

// =================================================================== TypeScript

/// A clean copy of `tests/fixtures-ts/` - the TypeScript counterpart of
/// [`project`] and [`go_project`]: `cyclone.toml` and the annotated schema,
/// in a directory of its own so a test can edit it without disturbing the
/// committed fixture. TypeScript needs no project file of its own - a
/// generated codec reaches a model through a relative `import` computed
/// straight from the model's own source path - so there is nothing here to
/// copy beyond the schema and `cyclone.toml`.
fn ts_project(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("src/models")).expect("create project");

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-ts");
    std::fs::copy(
        fixtures.join("cyclone.toml"),
        directory.join("cyclone.toml"),
    )
    .expect("copy cyclone.toml");
    for entry in std::fs::read_dir(fixtures.join("src/models")).expect("read fixtures") {
        let path = entry.expect("entry").path();
        std::fs::copy(
            &path,
            directory
                .join("src/models")
                .join(path.file_name().expect("name")),
        )
        .expect("copy schema");
    }

    directory
}

#[test]
fn ts_generate_writes_one_file_per_model_per_codec() {
    let directory = ts_project("ts-generate");
    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success(), "{}", stderr(&output));

    for file in [
        "src/generated/runtime.ts",
        "src/generated/handshake.ts",
        "src/generated/player_edge.ts",
        "src/generated/player_unity.ts",
        "src/generated/player_info_edge.ts",
        "src/generated/team_edge.ts",
        "src/generated/device_state_edge.ts",
        "src/generated/device_state_unity.ts",
    ] {
        let text = read(&directory, file);
        assert!(
            text.starts_with("// GENERATED BY cyclonec\n"),
            "{file}: {text}"
        );
    }

    // The model type is imported and referenced by name - no DTO, and every
    // codec file names the exact type the user wrote.
    let codec = read(&directory, "src/generated/player_edge.ts");
    assert!(
        codec.contains("import { Player } from \"../models/player\";"),
        "{codec}"
    );
    assert!(
        codec.contains("static encode(writer: Writer, value: Player): void {"),
        "{codec}"
    );
    // A bare nested-model field: the codec for it, and its own type too (see
    // generator::typescript's module docs for why, unlike Rust/Go).
    let team = read(&directory, "src/generated/team_edge.ts");
    assert!(
        team.contains("import { PlayerInfoEdgeCodec } from \"./player_info_edge\";"),
        "{team}"
    );
}

/// At minimum, issue.md §14 asks that this exact class parse and generate
/// correctly.
#[test]
fn ts_the_brief_s_device_state_example_parses_and_generates() {
    let directory = ts_project("ts-device-state");
    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let edge = read(&directory, "src/generated/device_state_edge.ts");
    assert!(edge.contains("writer.writeU32(value.Id);"), "{edge}");
    assert!(
        edge.contains("writer.writeF32(value.Temperature);"),
        "{edge}"
    );
    assert!(
        !edge.contains("DisplayName"),
        "the edge codec does not carry DisplayName:\n{edge}"
    );

    let unity = read(&directory, "src/generated/device_state_unity.ts");
    assert!(
        unity.contains("writer.writeString(value.DisplayName);"),
        "{unity}"
    );
    assert!(
        !unity.contains("value.Temperature"),
        "the unity codec does not carry Temperature:\n{unity}"
    );
}

#[test]
fn ts_check_passes_when_current_and_fails_when_stale() {
    let directory = ts_project("ts-check");
    cyclonec(&directory, &["generate"]);

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let source = read(&directory, "src/models/player.ts");
    std::fs::write(
        directory.join("src/models/player.ts"),
        source.replace(
            "Unrouted: number = 0;",
            "Unrouted: number = 0;\n\n    // CYCLONE_FIELD(u32)\n    \
             // CYCLONE_CODEC(\"edge\")\n    Level: number = 0;",
        ),
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate", "--check"]);
    assert!(!output.status.success(), "a stale tree must fail --check");
    assert!(stderr(&output).contains("stale:"), "{}", stderr(&output));
}

#[test]
fn ts_backend_refuses_array_of_array_rather_than_generate_it_wrong() {
    let directory = ts_project("ts-nested-array");
    std::fs::write(
        directory.join("src/models/grid.ts"),
        "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass Grid {\n    \
         // CYCLONE_FIELD(Array<Array<u8>>)\n    // CYCLONE_CODEC(\"edge\")\n    \
         Rows: number[][] = [];\n}\n",
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(!output.status.success(), "nested arrays must be refused");
    assert!(
        stderr(&output).contains("Array<Array<T>>"),
        "{}",
        stderr(&output)
    );
}

/// issue.md §13's own list of invalid examples, each reported rather than
/// silently guessed at.
#[test]
fn ts_invalid_annotation_examples_are_reported_not_silently_ignored() {
    for (file, contents, expect_in_error) in [
        (
            "cyclone_model_without_a_class.ts",
            "// CYCLONE_MODEL\nfunction notAClass() {}\n",
            "class",
        ),
        (
            "cyclone_field_without_a_field.ts",
            "// CYCLONE_MODEL\nclass S {\n    // CYCLONE_FIELD(u32)\n    doStuff(): void {}\n}\n",
            "CYCLONE_FIELD",
        ),
        (
            "missing_cyclone_field.ts",
            "// CYCLONE_MODEL\nclass S {\n    // CYCLONE_CODEC(\"edge\")\n    id: number;\n}\n",
            "CYCLONE_FIELD",
        ),
        (
            "invalid_wire_type.ts",
            "// CYCLONE_MODEL\nclass S {\n    // CYCLONE_FIELD(Vec<u32>)\n    \
             // CYCLONE_CODEC(\"edge\")\n    xs: number[];\n}\n",
            "Cyclone type",
        ),
        (
            "malformed_codec.ts",
            "// CYCLONE_MODEL\n// CYCLONE_CODEC(edge)\nclass S {}\n",
            "quoted",
        ),
        (
            "duplicate_field.ts",
            "// CYCLONE_MODEL\nclass S {\n    // CYCLONE_FIELD(u32)\n    \
             // CYCLONE_FIELD(f32)\n    id: number;\n}\n",
            "duplicate",
        ),
    ] {
        let directory = ts_project(&format!("ts-invalid-{}", file.trim_end_matches(".ts")));
        std::fs::write(directory.join("src/models").join(file), contents).expect("write");

        let output = cyclonec(&directory, &["generate"]);
        assert!(!output.status.success(), "{file} must be refused");
        assert!(
            stderr(&output).contains(expect_in_error),
            "{file}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn ts_model_path_overrides_every_model_with_one_shared_specifier() {
    let without_override = ts_project("ts-model-path-default");
    cyclonec(&without_override, &["generate", "-q"]);
    let default_codec = read(&without_override, "src/generated/player_edge.ts");
    assert!(
        default_codec.contains("from \"../models/player\";"),
        "{default_codec}"
    );

    let with_override = ts_project("ts-model-path-override");
    cyclonec(
        &with_override,
        &["generate", "-q", "--model-path", "@/models"],
    );
    let overridden = read(&with_override, "src/generated/player_edge.ts");
    assert!(overridden.contains("from \"@/models\";"), "{overridden}");
}

#[test]
fn mixed_rust_and_typescript_sources_in_one_run_are_rejected() {
    let directory = ts_project("ts-mixed");
    std::fs::write(
        directory.join("src/models/extra.rs"),
        "#[network]\n#[codec(edge)]\nstruct Extra {\n    #[network(u32)]\n    #[codec(edge)]\n    id: u32,\n}\n",
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(
        !output.status.success(),
        "mixing languages in one run must be refused"
    );
    let message = stderr(&output);
    assert!(
        message.contains("Rust") && message.contains("TypeScript"),
        "{message}"
    );
}

#[test]
fn the_ts_generated_tree_matches_the_committed_fixture() {
    let directory = ts_project("ts-fixture-is-current");
    cyclonec(&directory, &["generate", "-q"]);

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-ts");
    for path in [
        "src/generated/player_edge.ts",
        "src/generated/handshake.ts",
        ".cyclone/schema.json",
    ] {
        let fresh = read(&directory, path);
        let committed = std::fs::read_to_string(fixtures.join(path)).expect("committed fixture");
        assert!(
            same_but_for_timestamp(&fresh, &committed),
            "{path} in tests/fixtures-ts/ is out of date - regenerate it"
        );
    }
}

// =================================================================== JavaScript

/// A clean copy of `tests/fixtures-js/` - the JavaScript counterpart of
/// [`ts_project`].
fn js_project(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("src/models")).expect("create project");

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-js");
    std::fs::copy(
        fixtures.join("cyclone.toml"),
        directory.join("cyclone.toml"),
    )
    .expect("copy cyclone.toml");
    for entry in std::fs::read_dir(fixtures.join("src/models")).expect("read fixtures") {
        let path = entry.expect("entry").path();
        std::fs::copy(
            &path,
            directory
                .join("src/models")
                .join(path.file_name().expect("name")),
        )
        .expect("copy schema");
    }

    directory
}

#[test]
fn js_generate_writes_one_file_per_model_per_codec() {
    let directory = js_project("js-generate");
    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success(), "{}", stderr(&output));

    for file in [
        "src/generated/runtime.js",
        "src/generated/handshake.js",
        "src/generated/player_edge.js",
        "src/generated/player_unity.js",
        "src/generated/player_info_edge.js",
        "src/generated/team_edge.js",
    ] {
        let text = read(&directory, file);
        assert!(
            text.starts_with("// GENERATED BY cyclonec\n"),
            "{file}: {text}"
        );
    }

    // No type annotation anywhere, and every relative import ends in `.js` -
    // this file is meant to run directly under Node's ESM loader.
    let codec = read(&directory, "src/generated/player_edge.js");
    assert!(codec.contains("static encode(writer, value) {"), "{codec}");
    assert!(!codec.contains(": Writer"), "{codec}");
    let team = read(&directory, "src/generated/team_edge.js");
    assert!(
        team.contains("import { PlayerInfoEdgeCodec } from \"./player_info_edge.js\";"),
        "{team}"
    );
}

#[test]
fn js_the_brief_s_device_state_example_parses_and_generates() {
    let directory = js_project("js-device-state");
    let output = cyclonec(&directory, &["generate"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let edge = read(&directory, "src/generated/device_state_edge.js");
    assert!(edge.contains("writer.writeU32(value.Id);"), "{edge}");
    assert!(
        edge.contains("writer.writeF32(value.Temperature);"),
        "{edge}"
    );

    let unity = read(&directory, "src/generated/device_state_unity.js");
    assert!(
        unity.contains("writer.writeString(value.DisplayName);"),
        "{unity}"
    );
}

#[test]
fn js_backend_refuses_array_of_array_rather_than_generate_it_wrong() {
    let directory = js_project("js-nested-array");
    std::fs::write(
        directory.join("src/models/grid.js"),
        "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass Grid {\n    \
         // CYCLONE_FIELD(Array<Array<u8>>)\n    // CYCLONE_CODEC(\"edge\")\n    Rows = [];\n}\n",
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(!output.status.success(), "nested arrays must be refused");
    assert!(
        stderr(&output).contains("Array<Array<T>>"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn mixed_typescript_and_javascript_sources_in_one_run_are_rejected() {
    // JavaScript and TypeScript share one annotation concept (issue.md §9),
    // but they are still two languages as far as `--src`/`--out` is
    // concerned - each needs its own, the same as any other pair.
    let directory = js_project("js-mixed-with-ts");
    std::fs::write(
        directory.join("src/models/extra.ts"),
        "// CYCLONE_MODEL\n// CYCLONE_CODEC(\"edge\")\nclass Extra {\n    \
         // CYCLONE_FIELD(u32)\n    // CYCLONE_CODEC(\"edge\")\n    Id: number = 0;\n}\n",
    )
    .expect("write");

    let output = cyclonec(&directory, &["generate"]);
    assert!(
        !output.status.success(),
        "mixing languages in one run must be refused"
    );
    let message = stderr(&output);
    assert!(
        message.contains("JavaScript") && message.contains("TypeScript"),
        "{message}"
    );
}

#[test]
fn the_js_generated_tree_matches_the_committed_fixture() {
    let directory = js_project("js-fixture-is-current");
    cyclonec(&directory, &["generate", "-q"]);

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-js");
    for path in [
        "src/generated/player_edge.js",
        "src/generated/handshake.js",
        ".cyclone/schema.json",
    ] {
        let fresh = read(&directory, path);
        let committed = std::fs::read_to_string(fixtures.join(path)).expect("committed fixture");
        assert!(
            same_but_for_timestamp(&fresh, &committed),
            "{path} in tests/fixtures-js/ is out of date - regenerate it"
        );
    }
}

// ==================================================================== --watch
//
// `tests/watch.rs` drives `cyclonec::watch::run` directly and covers each
// scenario (creation, deletion, a parse error fixed, one save that fires
// twice, the output directory never feeding back in) far faster than a real
// subprocess could. What only a real subprocess proves is that `--watch`,
// typed on an actual command line, actually starts, actually regenerates,
// and actually stops on the normal termination signal - so that much is
// covered here, once, the way the rest of this file covers everything else.

/// Spawns `cyclonec` with `arguments` and returns immediately, without
/// waiting for it to exit - unlike [`cyclonec`], which is for commands that
/// are expected to finish on their own.
fn spawn(directory: &Path, arguments: &[&str]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_cyclonec"))
        .current_dir(directory)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cyclonec")
}

fn wait_until(mut condition: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        if condition() {
            return true;
        }
        if start.elapsed() >= timeout {
            return condition();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Sends `child` the normal termination signal - `SIGTERM` on Unix, the only
/// signal Windows' `TerminateProcess` amounts to on the rest - and waits for
/// it to exit. Panics if it has not within `timeout`: a `--watch` process
/// that is still there is one that is hanging, or has left something of
/// itself running in the background, either of which is exactly what a
/// "shuts down cleanly" test exists to catch.
fn terminate_and_wait(mut child: Child, timeout: Duration) {
    #[cfg(unix)]
    {
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        const SIGTERM: i32 = 15;
        unsafe {
            kill(child.id() as i32, SIGTERM);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }

    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            return;
        }
        assert!(
            start.elapsed() < timeout,
            "cyclonec --watch did not exit within {timeout:?} of being sent the normal \
             termination signal"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn watch_generates_immediately_regenerates_on_change_and_shuts_down_on_signal() {
    let directory = project("watch-real-binary");
    let child = spawn(&directory, &["--watch", "-q"]);

    assert!(
        wait_until(
            || read_opt(&directory, "src/generated/player_edge.rs").is_some(),
            Duration::from_secs(10)
        ),
        "cyclonec --watch never produced its initial generation"
    );

    rewrite_player(
        &directory,
        "    #[network(u32)]\n    #[codec(edge)]\n    pub id: u32,\n\n    \
         #[network(f64)]\n    #[codec(edge)]\n    pub x: f64,\n",
    );
    assert!(
        wait_until(
            || read_opt(&directory, "src/generated/player_edge.rs")
                .is_some_and(|text| text.contains("write_f64")),
            Duration::from_secs(10)
        ),
        "cyclonec --watch never regenerated after src/models/player.rs changed"
    );

    terminate_and_wait(child, Duration::from_secs(10));
}

#[test]
fn watch_and_check_are_refused_together() {
    let directory = project("watch-and-check-refused");
    let output = cyclonec(&directory, &["generate", "--watch", "--check"]);
    assert!(!output.status.success());
    let message = stderr(&output);
    assert!(
        message.contains("--watch") && message.contains("--check"),
        "{message}"
    );
}
