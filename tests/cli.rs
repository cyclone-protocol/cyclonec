//! The generator, driven the way a user drives it.
//!
//! These run the real binary over real files, so what is asserted is what a user
//! gets — not what an internal function returns.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Runs `cyclonec` with the given arguments.
fn cyclonec(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cyclonec"))
        .args(arguments)
        .output()
        .expect("run cyclonec")
}

/// Writes `source` into a directory of its own and generates from it.
///
/// `--out` names the same directory, so the file lands at `cyclone.codec.rs`.
/// Returns the generated text, or `None` when nothing was written.
fn generate(name: &str, source: &str) -> (Output, Option<String>) {
    let directory = scratch(name);
    let input = directory.join(format!("{name}.rs"));
    std::fs::write(&input, source).expect("write source");

    let output = cyclonec(&[
        "--out",
        directory.to_str().expect("utf-8 path"),
        input.to_str().expect("utf-8 path"),
    ]);
    let generated = std::fs::read_to_string(directory.join("cyclone.codec.rs")).ok();

    (output, generated)
}

/// The C# counterpart of [`generate`]: writes `source` as `{name}.cs` and
/// reads back `cyclone.codec.cs`.
fn generate_csharp(name: &str, source: &str) -> (Output, Option<String>) {
    let directory = scratch(name);
    let input = directory.join(format!("{name}.cs"));
    std::fs::write(&input, source).expect("write source");

    let output = cyclonec(&[
        "--out",
        directory.to_str().expect("utf-8 path"),
        input.to_str().expect("utf-8 path"),
    ]);
    let generated = std::fs::read_to_string(directory.join("cyclone.codec.cs")).ok();

    (output, generated)
}

/// A clean directory under `target/`, so tests never see each other's files.
fn scratch(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/tests").join(name);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

// ================================================================== §2, §15

/// §2 — the codecs a model declares are the codecs that get generated. There is
/// no flag for it, and nothing else decides.
#[test]
fn a_model_declares_the_codecs_that_are_generated() {
    let (output, generated) = generate(
        "declared",
        r#"
        #[network]
        #[codec(edge, unity)]
        struct DeviceState {
            #[network(u32)]
            #[codec(edge, unity)]
            id: u32,

            #[network(f32)]
            #[codec(edge)]
            temperature: f32,

            #[network(string)]
            #[codec(unity)]
            display_name: String,
        }
        "#,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.contains("pub struct DeviceStateEdgeCodec;"));
    assert!(generated.contains("pub struct DeviceStateUnityCodec;"));
    // §15 — no third codec, invented from nowhere. (`pub struct` alone would
    // also count the runtime's own types, which every file carries.)
    assert_eq!(generated.matches("Codec;").count(), 2);
}

/// §16 — a codec name is an identifier, and the only thing done with it is
/// spelling a type. `custom_a` becomes `CustomA`, `orange_pi` becomes `OrangePi`.
#[test]
fn codec_names_become_pascal_case_type_names() {
    let (output, generated) = generate(
        "names",
        r#"
        #[network]
        #[codec(edge, orange_pi, unity, custom_a)]
        struct DeviceState {
            #[network(u32)]
            #[codec(edge, orange_pi, unity, custom_a)]
            id: u32,
        }
        "#,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    for name in [
        "DeviceStateEdgeCodec",
        "DeviceStateOrangePiCodec",
        "DeviceStateUnityCodec",
        "DeviceStateCustomACodec",
    ] {
        assert!(generated.contains(&format!("pub struct {name};")), "missing {name}");
    }
}

/// A field naming a codec the model never declared cannot conjure one into
/// existence — §15 forbids a third codec.
#[test]
fn a_field_cannot_invent_a_codec() {
    let (_, generated) = generate(
        "invented",
        r#"
        #[network]
        #[codec(edge)]
        struct DeviceState {
            #[network(u32)]
            #[codec(edge, unity)]
            id: u32,
        }
        "#,
    );

    let generated = generated.expect("a codec file");
    assert!(generated.contains("DeviceStateEdgeCodec"));
    assert!(!generated.contains("UnityCodec"), "{generated}");
}

// ====================================================================== §18

/// §18 — the one syntax error worth reporting: the generator was told the field
/// is on the wire, but not what to write for it.
#[test]
fn a_field_network_attribute_needs_a_type() {
    let (output, generated) = generate(
        "no_type",
        r#"
        #[network]
        #[codec(edge)]
        struct Player {
            #[network]
            #[codec(edge)]
            hp: u32,
        }
        "#,
    );

    assert!(!output.status.success());
    assert!(generated.is_none(), "nothing is written when generation fails");

    let stderr = stderr(&output);
    assert!(
        stderr.contains("#[network] field requires a network type"),
        "{stderr}"
    );
    // The line of the `#[network]` itself, so it reads like a compiler error
    // rather than a shrug.
    assert!(stderr.contains("no_type.rs:5"), "{stderr}");
}

/// Everything else is `rustc`'s to report. A field whose Rust type cannot hold
/// its network type is not this generator's business.
#[test]
fn nothing_else_is_validated() {
    let (output, generated) = generate(
        "unvalidated",
        r#"
        #[network]
        #[codec(edge)]
        struct Player {
            #[network(u32)]
            #[codec(edge)]
            hp: u64,

            #[network(NoSuchModel)]
            #[codec(edge)]
            info: NoSuchModel,
        }
        "#,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    // §4 — the declared type is believed, not checked against the Rust one.
    assert!(generated.contains("writer.write_u32(value.hp);"));
    // §13 — the call is spelled; whether the symbol exists is rustc's question.
    assert!(generated.contains("NoSuchModelEdgeCodec::encode(writer, &value.info);"));
}

// ================================================================ the parser

/// §17 — the parser is not a Rust parser, but it does know where a token is. A
/// `struct` in a comment and a `#[network]` in a string are not source.
#[test]
fn comments_and_strings_are_not_source() {
    let (output, generated) = generate(
        "lexing",
        // A longer fence, because the source under test contains a raw string.
        r##"
        // #[network] struct Commented { }
        /* #[network]
           struct BlockCommented { } */

        #[network]
        #[codec(edge)]
        struct Real {
            #[network(u32)]
            #[codec(edge)]
            id: u32,
        }

        fn noise() -> &'static str {
            let _ = '}';
            let _ = "#[network] struct InAString { }";
            r#"#[network] struct InARawString { }"#
        }
        "##,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.contains("RealEdgeCodec"));
    for ghost in ["Commented", "BlockCommented", "InAString", "InARawString"] {
        assert!(!generated.contains(ghost), "{ghost} is not a model");
    }
}

/// A file with no models, or none that declared a codec, produces nothing at
/// all — no empty file left behind to confuse the next reader.
#[test]
fn a_file_with_nothing_to_generate_writes_nothing() {
    let (output, generated) = generate("empty", "pub struct Ordinary { pub id: u32 }");
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(generated.is_none());

    let (output, generated) = generate(
        "no_codecs",
        r#"
        #[network]
        struct Marked {
            #[network(u32)]
            id: u32,
        }
        "#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(generated.is_none(), "a model that declared no codec generates nothing");
}

// =================================================================== the CLI

/// Generating twice produces byte-identical output, which is what makes
/// `--check` mean something.
#[test]
fn generation_is_deterministic() {
    let source = r#"
        #[network]
        #[codec(edge, unity)]
        struct DeviceState {
            #[network(u32)]
            #[codec(edge, unity)]
            id: u32,
        }
        "#;

    // The same file name in two directories: the header names the sources, so a
    // different name would differ for a reason that is not the generator's.
    let mut rendered = Vec::new();
    for run in ["deterministic_a", "deterministic_b"] {
        let directory = scratch(run);
        let input = directory.join("same_name.rs");
        std::fs::write(&input, source).expect("write source");
        assert!(cyclonec(&[
            "--out",
            directory.to_str().expect("utf-8 path"),
            input.to_str().expect("utf-8 path"),
        ])
        .status
        .success());
        rendered.push(std::fs::read_to_string(directory.join("cyclone.codec.rs")).expect("read"));
    }

    assert_eq!(rendered[0], rendered[1]);
}

/// `--check` reports staleness and writes nothing; a second run, after
/// generating, is clean.
#[test]
fn check_reports_stale_files() {
    let directory = scratch("check");
    let input = directory.join("check.rs");
    std::fs::write(
        &input,
        r#"
        #[network]
        #[codec(edge)]
        struct Player {
            #[network(u32)]
            #[codec(edge)]
            hp: u32,
        }
        "#,
    )
    .expect("write source");

    let path = input.to_str().expect("utf-8 path");
    let out = directory.to_str().expect("utf-8 path");

    let stale = cyclonec(&["--check", "--out", out, path]);
    assert!(!stale.status.success(), "a missing output file is stale");
    assert!(stderr(&stale).contains("stale"), "{}", stderr(&stale));
    assert!(!directory.join("cyclone.codec.rs").exists(), "--check writes nothing");

    assert!(cyclonec(&["--out", out, path]).status.success());

    let fresh = cyclonec(&["--check", "--out", out, path]);
    assert!(fresh.status.success(), "{}", stderr(&fresh));
    assert!(stderr(&fresh).contains("up to date"), "{}", stderr(&fresh));
}

/// `--stdout` prints instead of writing.
#[test]
fn stdout_writes_no_file() {
    let directory = scratch("stdout");
    let input = directory.join("stdout.rs");
    std::fs::write(
        &input,
        r#"
        #[network]
        #[codec(edge)]
        struct Player {
            #[network(u32)]
            #[codec(edge)]
            hp: u32,
        }
        "#,
    )
    .expect("write source");

    let output = cyclonec(&["--stdout", input.to_str().expect("utf-8 path")]);
    assert!(output.status.success(), "{}", stderr(&output));

    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(printed.contains("PlayerEdgeCodec"), "{printed}");
    assert!(!directory.join("cyclone.codec.rs").exists());
}

/// The generator never reads its own output back in.
#[test]
fn generated_files_are_not_read_again() {
    let directory = scratch("reread");
    std::fs::write(
        directory.join("model.rs"),
        r#"
        #[network]
        #[codec(edge)]
        struct Player {
            #[network(u32)]
            #[codec(edge)]
            hp: u32,
        }
        "#,
    )
    .expect("write source");

    let path = directory.to_str().expect("utf-8 path");
    assert!(cyclonec(&["--out", path, path]).status.success());

    // A second run over the directory now sees `cyclone.codec.rs` too, and must
    // skip it: it is the generator's own output, and it holds a runtime full of
    // ordinary-looking structs.
    let second = cyclonec(&["--out", path, path]);
    assert!(second.status.success(), "{}", stderr(&second));
    assert!(stderr(&second).contains("unchanged"), "{}", stderr(&second));
}

#[test]
fn usage_errors_are_reported() {
    let no_paths = cyclonec(&[]);
    assert_eq!(no_paths.status.code(), Some(2));
    assert!(stderr(&no_paths).contains("no input path"));

    let unknown = cyclonec(&["--nope", "-o", "out", "x.rs"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(stderr(&unknown).contains("unknown option"));

    assert!(cyclonec(&["--help"]).status.success());
    assert!(cyclonec(&["--version"]).status.success());
}

// ============================================================= where it writes

/// `--out` is required: the output is one file holding every codec, and guessing
/// where a whole project's codecs belong is not the generator's call.
#[test]
fn out_is_required() {
    let directory = scratch("required");
    let input = directory.join("model.rs");
    std::fs::write(&input, "#[network] #[codec(edge)] struct P { #[network(u32)] #[codec(edge)] a: u32, }")
        .expect("write source");

    let output = cyclonec(&[input.to_str().expect("utf-8 path")]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("--out is required"), "{}", stderr(&output));
}

/// A path ending in `.rs` is the file to write; anything else is a directory
/// holding `cyclone.codec.rs`. The rule is the extension, not whether the path
/// already exists, so a first run and a second run agree.
#[test]
fn out_names_a_file_or_a_directory() {
    let directory = scratch("destination");
    let input = directory.join("model.rs");
    std::fs::write(&input, "#[network] #[codec(edge)] struct P { #[network(u32)] #[codec(edge)] a: u32, }")
        .expect("write source");
    let source = input.to_str().expect("utf-8 path").to_owned();

    // A directory — including one that does not exist yet.
    let into_directory = directory.join("gen");
    let output = cyclonec(&["--out", into_directory.to_str().expect("utf-8 path"), &source]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(into_directory.join("cyclone.codec.rs").exists());

    // An exact file.
    let into_file = directory.join("net/codec.rs");
    let output = cyclonec(&["--out", into_file.to_str().expect("utf-8 path"), &source]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(into_file.exists());
    assert!(!directory.join("net/cyclone.codec.rs").exists());
}

/// Several sources produce one file, not one file each.
#[test]
fn every_source_lands_in_one_file() {
    let directory = scratch("aggregate");
    std::fs::write(
        directory.join("a.rs"),
        "#[network] #[codec(edge)] struct A { #[network(u32)] #[codec(edge)] a: u32, }",
    )
    .expect("write source");
    std::fs::write(
        directory.join("b.rs"),
        "#[network] #[codec(unity)] struct B { #[network(u32)] #[codec(unity)] b: u32, }",
    )
    .expect("write source");

    let path = directory.to_str().expect("utf-8 path");
    assert!(cyclonec(&["--out", path, path]).status.success());

    let generated = std::fs::read_to_string(directory.join("cyclone.codec.rs")).expect("read");
    assert!(generated.contains("AEdgeCodec"));
    assert!(generated.contains("BUnityCodec"));

    // Both sources are named in the header, sorted, so the file says where it
    // came from and two runs agree on the order.
    assert!(generated.contains("//     a.rs\n//     b.rs\n"), "{generated}");
}

/// The file carries the runtime, so it compiles with nothing imported and
/// nothing added to Cargo.toml.
#[test]
fn the_output_carries_the_runtime() {
    let (_, generated) = generate(
        "selfcontained",
        r#"
        #[network]
        #[codec(edge)]
        struct Player {
            #[network(u32)]
            #[codec(edge)]
            hp: u32,
        }
        "#,
    );

    let generated = generated.expect("a codec file");

    for item in [
        "pub struct Writer",
        "pub struct Reader",
        "pub enum DecodeError",
        "pub struct Limits",
    ] {
        assert!(generated.contains(item), "missing {item}");
    }

    // Carried, not derived: no `use` of a runtime crate, and nothing to add to
    // Cargo.toml.
    assert!(!generated.contains("use cyclone"), "{generated}");
}

// ================================================================= C# — §18

/// §18 "Basic model" — `[Network] [Codec("edge")]` on a class with one field
/// produces exactly `PlayerEdgeCodec`.
#[test]
fn csharp_basic_model() {
    let (output, generated) = generate_csharp(
        "basic",
        r#"
        using Cyclone;

        [Network]
        [Codec("edge")]
        public class Player
        {
            [Network("u32")]
            [Codec("edge")]
            public uint Id { get; set; }
        }
        "#,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.contains("public static class PlayerEdgeCodec"));
    assert!(generated.contains("writer.WriteUInt32(value.Id);"));
    assert_eq!(generated.matches("Codec\n").count(), 1);
}

/// §18 "Multiple codecs" — `edge` carries `Id` and `Health`; `unity` carries
/// `Id` and `Name`. Verbatim from the brief.
#[test]
fn csharp_multiple_codecs() {
    let (output, generated) = generate_csharp(
        "multiple",
        r#"
        using Cyclone;

        [Network]
        [Codec("edge", "unity")]
        public class Player
        {
            [Network("u32")]
            [Codec("edge", "unity")]
            public uint Id { get; set; }

            [Network("f32")]
            [Codec("edge")]
            public float Health { get; set; }

            [Network("string")]
            [Codec("unity")]
            public string Name { get; set; } = string.Empty;
        }
        "#,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    let edge = extract_method(&generated, "PlayerEdgeCodec", "Encode");
    assert!(edge.contains("value.Id"));
    assert!(edge.contains("value.Health"));
    assert!(!edge.contains("value.Name"));

    let unity = extract_method(&generated, "PlayerUnityCodec", "Encode");
    assert!(unity.contains("value.Id"));
    assert!(unity.contains("value.Name"));
    assert!(!unity.contains("value.Health"));
}

/// §18 "Custom codec" — an identifier the generator has never heard of works
/// exactly like `edge` or `unity`.
#[test]
fn csharp_custom_codec_names_need_no_registration() {
    let (output, generated) = generate_csharp(
        "custom",
        r#"
        using Cyclone;

        [Network]
        [Codec("orange_pi", "custom_protocol")]
        public class Player
        {
            [Network("u32")]
            [Codec("orange_pi", "custom_protocol")]
            public uint Id { get; set; }
        }
        "#,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.contains("PlayerOrangePiCodec"));
    assert!(generated.contains("PlayerCustomProtocolCodec"));
}

/// §18's exact native-type-independence case, and the reason it lives here
/// rather than in the compiled `tests/csharp/` fixture: `[Network("u32")]` on
/// a `ulong` reports wire type `u32` — not `u64` — in the generator's own
/// output, whether or not a C# compiler would accept the mismatch (h.md §2
/// leaves that question to the C# compiler, not to `cyclonec`).
#[test]
fn csharp_native_type_does_not_change_the_wire_type() {
    let (output, generated) = generate_csharp(
        "native_type",
        r#"
        using Cyclone;

        [Network]
        [Codec("edge")]
        public class Reading
        {
            [Network("u32")]
            [Codec("edge")]
            public ulong Value { get; set; }
        }
        "#,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    // u32, not u64: WriteUInt32/ReadUInt32, never WriteUInt64/ReadUInt64. The
    // runtime block always defines ReadUInt64 (every primitive method lives
    // there unconditionally), so the codec body — not the whole file — is what
    // has to be free of it.
    assert!(generated.contains("writer.WriteUInt32(value.Value);"), "{generated}");
    assert!(generated.contains("value.Value = reader.ReadUInt32();"), "{generated}");

    let codec = extract_method(&generated, "ReadingEdgeCodec", "Encode");
    assert!(!codec.contains("WriteUInt64"), "{codec}");
    let decode = extract_method(&generated, "ReadingEdgeCodec", "Decode");
    assert!(!decode.contains("ReadUInt64"), "{decode}");
}

// ============================================================ C# — parity

/// The two scanners reach the same [`cyclone_cli`-style] shape for the same
/// schema: same codec names, same field routing. (`cyclonec` has no library
/// target, so this compares generated *text* rather than the IR directly —
/// the same black-box guarantee a user gets.)
#[test]
fn csharp_and_rust_agree_on_codec_names_for_the_same_schema() {
    let (_, rust) = generate(
        "parity_rust",
        r#"
        #[network]
        #[codec(edge, unity)]
        struct DeviceState {
            #[network(u32)]
            #[codec(edge, unity)]
            id: u32,
            #[network(f32)]
            #[codec(edge)]
            temperature: f32,
        }
        "#,
    );
    let (_, csharp) = generate_csharp(
        "parity_csharp",
        r#"
        using Cyclone;

        [Network]
        [Codec("edge", "unity")]
        public class DeviceState
        {
            [Network("u32")]
            [Codec("edge", "unity")]
            public uint Id { get; set; }

            [Network("f32")]
            [Codec("edge")]
            public float Temperature { get; set; }
        }
        "#,
    );

    let rust = rust.expect("a codec file");
    let csharp = csharp.expect("a codec file");

    for name in ["DeviceStateEdgeCodec", "DeviceStateUnityCodec"] {
        assert!(rust.contains(name), "{name} missing from Rust output");
        assert!(csharp.contains(name), "{name} missing from C# output");
    }

    // The unity codec carries id but not temperature, on both sides.
    assert!(!extract_fn(&rust, "DeviceStateUnityCodec", "encode").contains("temperature"));
    assert!(!extract_method(&csharp, "DeviceStateUnityCodec", "Encode").contains("Temperature"));
}

/// A directory holding both `.rs` and `.cs` sources produces both output
/// files, each holding only its own language's models.
#[test]
fn a_directory_with_both_languages_produces_both_outputs() {
    let directory = scratch("both_languages");
    std::fs::write(
        directory.join("a.rs"),
        "#[network] #[codec(edge)] struct A { #[network(u32)] #[codec(edge)] a: u32, }",
    )
    .expect("write source");
    std::fs::write(
        directory.join("b.cs"),
        r#"using Cyclone;
        [Network] [Codec("edge")]
        public class B { [Network("u32")] [Codec("edge")] public uint Value { get; set; } }
        "#,
    )
    .expect("write source");

    let path = directory.to_str().expect("utf-8 path");
    let output = cyclonec(&["--out", path, path]);
    assert!(output.status.success(), "{}", stderr(&output));

    let rust = std::fs::read_to_string(directory.join("cyclone.codec.rs")).expect("read rust");
    let csharp = std::fs::read_to_string(directory.join("cyclone.codec.cs")).expect("read csharp");

    assert!(rust.contains("AEdgeCodec"));
    assert!(!rust.contains("BEdgeCodec"), "the Rust file must not carry C# models");
    assert!(csharp.contains("BEdgeCodec"));
    assert!(!csharp.contains("AEdgeCodec"), "the C# file must not carry Rust models");
}

/// An explicit `.cs` destination is C#'s exact file; a Rust sibling is written
/// alongside it only if Rust models are also present.
#[test]
fn explicit_cs_destination_is_exact_and_has_no_rust_sibling_when_none_is_needed() {
    let directory = scratch("cs_destination");
    std::fs::write(
        directory.join("model.cs"),
        r#"using Cyclone;
        [Network] [Codec("edge")]
        public class Player { [Network("u32")] [Codec("edge")] public uint Hp { get; set; } }
        "#,
    )
    .expect("write source");

    let out = directory.join("net.cs");
    let output = cyclonec(&[
        "--out",
        out.to_str().expect("utf-8 path"),
        directory.join("model.cs").to_str().expect("utf-8 path"),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    assert!(out.exists());
    assert!(
        !out.with_extension("rs").exists(),
        "no Rust models exist, so no Rust sibling should be written"
    );
}

/// The C# runtime block is carried the same way the Rust one is: no
/// `using Cyclone.Runtime` (there is no such assembly), just the classes
/// themselves, ready to compile.
#[test]
fn csharp_output_carries_its_own_runtime() {
    let (_, generated) = generate_csharp(
        "csharp_selfcontained",
        r#"
        using Cyclone;

        [Network]
        [Codec("edge")]
        public class Player
        {
            [Network("u32")]
            [Codec("edge")]
            public uint Hp { get; set; }
        }
        "#,
    );

    let generated = generated.expect("a codec file");

    for item in ["public sealed class Writer", "public ref struct Reader", "public sealed class DecodeException", "public struct Limits"] {
        assert!(generated.contains(item), "missing {item}");
    }
}

/// A `[Network]` field with no wire type is the one error the C# scanner
/// reports — the exact counterpart of the Rust `#[network]`-with-no-type case.
#[test]
fn csharp_network_field_needs_a_wire_type() {
    let (output, generated) = generate_csharp(
        "no_wire_type",
        r#"
        using Cyclone;

        [Network]
        [Codec("edge")]
        public class Player
        {
            [Network]
            [Codec("edge")]
            public uint Hp { get; set; }
        }
        "#,
    );

    assert!(!output.status.success());
    assert!(generated.is_none());
    assert!(stderr(&output).contains("requires a wire type"), "{}", stderr(&output));
}

/// A struct nothing marks is not a model in C# either, and its neighbours are
/// unaffected — the same guarantee `an_unmarked_struct_does_not_disturb_the_next_model`
/// checks on the Rust side.
#[test]
fn csharp_unmarked_class_does_not_disturb_the_next_model() {
    let (output, generated) = generate_csharp(
        "csharp_unmarked",
        r#"
        using Cyclone;

        public class Ignored
        {
            public uint Whatever { get; set; }
        }

        [Network]
        [Codec("edge")]
        public class Real
        {
            [Network("u32")]
            [Codec("edge")]
            public uint Value { get; set; }
        }
        "#,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.contains("RealEdgeCodec"));
    assert!(!generated.contains("Ignored"));
}

/// Comments and strings are not source in C# either.
#[test]
fn csharp_ignores_braces_in_comments_and_strings() {
    let (output, generated) = generate_csharp(
        "csharp_lexing",
        r####"
        using Cyclone;

        // [Network] public class Commented { }
        /* [Network]
           public class BlockCommented { } */

        [Network]
        [Codec("edge")]
        public class Real
        {
            [Network("string")]
            [Codec("edge")]
            public string Name { get; set; } =
                "a } brace \" in a string { [Network] public class InAString {}";

            [Network("string")]
            [Codec("edge")]
            public string Verbatim { get; set; } = @"another } one [Network]";
        }
        "####,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.contains("RealEdgeCodec"));
    assert!(!generated.contains("Commented"));
    assert!(!generated.contains("InAString"));
}

/// A property with a default value initializer — `{ get; set; } = expr;` — is
/// read correctly, including the initializer expression itself, which is
/// stepped over rather than misread as the end of the class body.
#[test]
fn csharp_property_initializers_are_skipped_correctly() {
    let (output, generated) = generate_csharp(
        "csharp_initializers",
        r#"
        using Cyclone;

        [Network]
        [Codec("edge")]
        public class Player
        {
            [Network("string")]
            [Codec("edge")]
            public string Name { get; set; } = string.Empty;

            [Network("bytes")]
            [Codec("edge")]
            public byte[] Blob { get; set; } = System.Array.Empty<byte>();

            [Network("u32")]
            [Codec("edge")]
            public uint Id { get; set; }
        }
        "#,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    // All three fields were found — the initializers did not swallow `Id`.
    assert!(generated.contains("value.Name"));
    assert!(generated.contains("value.Blob"));
    assert!(generated.contains("value.Id"));
}

// ================================================================ C# — helpers

/// Extracts the body of one method inside one class from generated C# text —
/// enough to check which fields a codec's `Encode` touches, without a real
/// parser.
fn extract_method<'a>(source: &'a str, class_name: &str, method_name: &str) -> &'a str {
    let class_at = source.find(&format!("class {class_name}")).unwrap_or_else(|| {
        panic!("{class_name} not found in:\n{source}")
    });
    let method_at = source[class_at..].find(method_name).unwrap_or_else(|| {
        panic!("{method_name} not found in {class_name}")
    }) + class_at;
    let body_start = source[method_at..].find('{').unwrap() + method_at;
    let body_end = source[body_start..].find("\n    }").unwrap() + body_start;
    &source[body_start..body_end]
}

/// The Rust counterpart of [`extract_method`], for the parity test.
fn extract_fn<'a>(source: &'a str, struct_name: &str, fn_name: &str) -> &'a str {
    let struct_at = source.find(&format!("impl {struct_name}")).unwrap_or_else(|| {
        panic!("{struct_name} not found in:\n{source}")
    });
    let fn_at =
        source[struct_at..].find(&format!("fn {fn_name}")).unwrap() + struct_at;
    let body_start = source[fn_at..].find('{').unwrap() + fn_at;
    let body_end = source[body_start..].find("\n    }").unwrap() + body_start;
    &source[body_start..body_end]
}
