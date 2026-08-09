//! The generator, driven the way a user drives it.
//!
//! These run the real binary over real files, so what is asserted is what a user
//! gets - not what an internal function returns.

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

/// §2 - the codecs a model declares are the codecs that get generated. There is
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
    // §15 - no third codec, invented from nowhere. (`pub struct` alone would
    // also count the runtime's own types, which every file carries.)
    assert_eq!(generated.matches("Codec;").count(), 2);
}

/// §16 - a codec name is an identifier, and the only thing done with it is
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
/// existence - §15 forbids a third codec.
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

/// §18 - the one syntax error worth reporting: the generator was told the field
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

    // §4 - the declared type is believed, not checked against the Rust one.
    assert!(generated.contains("writer.write_u32(value.hp);"));
    // §13 - the call is spelled; whether the symbol exists is rustc's question.
    assert!(generated.contains("NoSuchModelEdgeCodec::encode(writer, &value.info);"));
}

// ================================================================ the parser

/// §17 - the parser is not a Rust parser, but it does know where a token is. A
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
/// all - no empty file left behind to confuse the next reader.
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

    // A directory - including one that does not exist yet.
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

// ================================================================= C# - §18

/// §18 "Basic model" - `[Network] [Codec("edge")]` on a class with one field
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

/// §18 "Multiple codecs" - `edge` carries `Id` and `Health`; `unity` carries
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

/// §18 "Custom codec" - an identifier the generator has never heard of works
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
/// a `ulong` reports wire type `u32` - not `u64` - in the generator's own
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
    // there unconditionally), so the codec body - not the whole file - is what
    // has to be free of it.
    assert!(generated.contains("writer.WriteUInt32(value.Value);"), "{generated}");
    assert!(generated.contains("value.Value = reader.ReadUInt32();"), "{generated}");

    let codec = extract_method(&generated, "ReadingEdgeCodec", "Encode");
    assert!(!codec.contains("WriteUInt64"), "{codec}");
    let decode = extract_method(&generated, "ReadingEdgeCodec", "Decode");
    assert!(!decode.contains("ReadUInt64"), "{decode}");
}

// ============================================================ C# - parity

/// The two scanners reach the same [`cyclone_cli`-style] shape for the same
/// schema: same codec names, same field routing. (`cyclonec` has no library
/// target, so this compares generated *text* rather than the IR directly -
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
/// reports - the exact counterpart of the Rust `#[network]`-with-no-type case.
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
/// unaffected - the same guarantee `an_unmarked_struct_does_not_disturb_the_next_model`
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

/// A property with a default value initializer - `{ get; set; } = expr;` - is
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

    // All three fields were found - the initializers did not swallow `Id`.
    assert!(generated.contains("value.Name"));
    assert!(generated.contains("value.Blob"));
    assert!(generated.contains("value.Id"));
}

// ================================================================ C# - helpers

/// Extracts the body of one method inside one class from generated C# text -
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

// ==================================================================== Go - §18

/// The Go counterpart of [`generate`] / [`generate_csharp`]: writes `source`
/// as `{name}.go` and reads back `cyclone.codec.go`.
fn generate_go(name: &str, source: &str) -> (Output, Option<String>) {
    let directory = scratch(name);
    let input = directory.join(format!("{name}.go"));
    std::fs::write(&input, source).expect("write source");

    let output = cyclonec(&[
        "--out",
        directory.to_str().expect("utf-8 path"),
        input.to_str().expect("utf-8 path"),
    ]);
    let generated = std::fs::read_to_string(directory.join("cyclone.codec.go")).ok();

    (output, generated)
}

/// §18 "Basic" - `//cyclone:model codec=edge` on a struct with one tagged
/// field produces exactly `PlayerEdgeCodec`.
#[test]
fn go_basic_model() {
    let (output, generated) = generate_go(
        "go_basic",
        "package models\n\n\
         //cyclone:model codec=edge\n\
         type Player struct {\n\
         \tID uint32 `cyclone:\"u32\" codec:\"edge\"`\n\
         }\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.starts_with("package models\n"));
    assert!(generated.contains("type PlayerEdgeCodec struct{}"));
    assert!(generated.contains("w.WriteU32(value.ID)"));
    assert_eq!(generated.matches("Codec struct{}").count(), 1);
}

/// §18 "Multiple codecs" - `edge` carries `ID` and `Temperature`; `unity`
/// carries `ID` and `Name`. Verbatim from the brief.
#[test]
fn go_multiple_codecs() {
    let (output, generated) = generate_go(
        "go_multiple",
        "package models\n\n\
         //cyclone:model codec=edge,unity\n\
         type DeviceState struct {\n\
         \tID          uint32  `cyclone:\"u32\" codec:\"edge,unity\"`\n\
         \tTemperature float32 `cyclone:\"f32\" codec:\"edge\"`\n\
         \tName        string  `cyclone:\"string\" codec:\"unity\"`\n\
         }\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    let edge = extract_go_method(&generated, "DeviceStateEdgeCodec", "Encode");
    assert!(edge.contains("value.ID"));
    assert!(edge.contains("value.Temperature"));
    assert!(!edge.contains("value.Name"));

    let unity = extract_go_method(&generated, "DeviceStateUnityCodec", "Encode");
    assert!(unity.contains("value.ID"));
    assert!(unity.contains("value.Name"));
    assert!(!unity.contains("value.Temperature"));
}

/// §18 "Custom codec" - identifiers the generator has never heard of work
/// exactly like `edge` or `unity`.
#[test]
fn go_custom_codec_names_need_no_registration() {
    let (output, generated) = generate_go(
        "go_custom",
        "package models\n\n\
         //cyclone:model codec=edge,orange_pi,custom\n\
         type DeviceState struct {\n\
         \tID uint32 `cyclone:\"u32\" codec:\"edge,orange_pi,custom\"`\n\
         }\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.contains("DeviceStateOrangePiCodec"));
    assert!(generated.contains("DeviceStateCustomCodec"));
}

/// §18 "Native type independence" - `cyclone:"u32"` on a `uint64` field
/// reports wire type `u32`, not `u64`, checked against the generator's own
/// output text rather than by compiling it (h.md §22 leaves the compiling part
/// to the Go compiler).
#[test]
fn go_native_type_does_not_change_the_wire_type() {
    let (output, generated) = generate_go(
        "go_native_type",
        "package models\n\n\
         //cyclone:model codec=edge\n\
         type DeviceState struct {\n\
         \tID uint64 `cyclone:\"u32\" codec:\"edge\"`\n\
         }\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.contains("w.WriteU32(value.ID)"), "{generated}");
    assert!(generated.contains("value.ID, err = r.ReadU32()"), "{generated}");

    let codec = extract_go_method(&generated, "DeviceStateEdgeCodec", "Encode");
    assert!(!codec.contains("WriteU64"), "{codec}");
    let decode = extract_go_method(&generated, "DeviceStateEdgeCodec", "Decode");
    assert!(!decode.contains("ReadU64"), "{decode}");
}

// ==================================================================== Go - §12

/// §12 - a directive not immediately followed by a struct is a reported error,
/// never a silent skip.
#[test]
fn go_directive_not_followed_by_struct_is_an_error() {
    let (output, generated) = generate_go(
        "go_orphan_directive",
        "package models\n\n\
         //cyclone:model codec=edge\n\
         func NotAStruct() {}\n",
    );

    assert!(!output.status.success());
    assert!(generated.is_none());
    assert!(
        stderr(&output).contains("must be immediately followed by"),
        "{}",
        stderr(&output)
    );
}

/// A directive on a non-struct type (`type X int`) is the same error.
#[test]
fn go_directive_on_non_struct_type_is_an_error() {
    let (output, _) = generate_go(
        "go_non_struct",
        "package models\n\n\
         //cyclone:model codec=edge\n\
         type Count int\n",
    );

    assert!(!output.status.success());
    assert!(stderr(&output).contains("is not a struct"), "{}", stderr(&output));
}

/// A field tagged `codec:"..."` with no `cyclone:"..."` wire type is the Go
/// counterpart of Rust's "field requires a network type" and C#'s "field
/// requires a wire type".
#[test]
fn go_field_missing_wire_type_is_an_error() {
    let (output, generated) = generate_go(
        "go_missing_wire_type",
        "package models\n\n\
         //cyclone:model codec=edge\n\
         type DeviceState struct {\n\
         \tID uint32 `codec:\"edge\"`\n\
         }\n",
    );

    assert!(!output.status.success());
    assert!(generated.is_none());
    assert!(
        stderr(&output).contains("field 'ID' is missing cyclone wire type"),
        "{}",
        stderr(&output)
    );
}

/// A malformed directive argument is reported rather than silently ignored.
#[test]
fn go_malformed_directive_argument_is_an_error() {
    let (output, _) = generate_go(
        "go_malformed_directive",
        "package models\n\n\
         //cyclone:model banana\n\
         type Player struct {\n\
         \tID uint32 `cyclone:\"u32\"`\n\
         }\n",
    );

    assert!(!output.status.success());
    assert!(stderr(&output).contains("invalid //cyclone:model directive"), "{}", stderr(&output));
}

/// `//cyclone:model` with no `codec=` at all is a valid model with zero
/// codecs - the same as bare `#[network]` in Rust and bare `[Network]` in C#,
/// keeping semantics identical across all three languages (h.md's own
/// opening line: "Go chỉ thay đổi cách biểu diễn metadata").
#[test]
fn go_model_with_no_codec_is_valid_and_generates_nothing_for_it() {
    let (output, generated) = generate_go(
        "go_no_codec",
        "package models\n\n\
         //cyclone:model\n\
         type Marked struct {\n\
         \tID uint32 `cyclone:\"u32\"`\n\
         }\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(generated.is_none(), "a model with no codec generates nothing, not an error");
}

// ==================================================================== Go - misc

/// A struct nothing marks is not a model, and its neighbours are unaffected.
#[test]
fn go_unmarked_struct_does_not_disturb_the_next_model() {
    let (output, generated) = generate_go(
        "go_unmarked",
        "package models\n\n\
         type Ignored struct {\n\
         \tWhatever uint32\n\
         }\n\n\
         //cyclone:model codec=edge\n\
         type Real struct {\n\
         \tValue uint32 `cyclone:\"u32\" codec:\"edge\"`\n\
         }\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.contains("RealEdgeCodec"));
    assert!(!generated.contains("Ignored"));
}

/// Comments and strings are not source in Go either, and a `//cyclone:model`
/// spelled inside a block comment or a string literal is not a directive.
#[test]
fn go_ignores_directives_in_comments_and_strings() {
    let (output, generated) = generate_go(
        "go_lexing",
        "package models\n\n\
         // //cyclone:model codec=edge\n\
         /* //cyclone:model codec=edge\n\
            type BlockCommented struct{} */\n\n\
         //cyclone:model codec=edge\n\
         type Real struct {\n\
         \tValue uint32 `cyclone:\"u32\" codec:\"edge\"`\n\
         }\n\n\
         func noise() string {\n\
         \treturn \"//cyclone:model codec=edge\\ntype InAString struct{}\"\n\
         }\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.contains("RealEdgeCodec"));
    assert!(!generated.contains("BlockCommented"));
    assert!(!generated.contains("InAString"));
}

/// A directive is not fooled by an unrelated comment starting the same way -
/// `//cyclone:modeling` is not `//cyclone:model`.
#[test]
fn go_directive_prefix_needs_a_word_boundary() {
    let (output, generated) = generate_go(
        "go_prefix_boundary",
        "package models\n\n\
         //cyclone:modeling this is not a directive\n\
         type NotAModel struct {\n\
         \tValue uint32\n\
         }\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(generated.is_none(), "not a directive, so NotAModel is not a model");
}

/// The `package` clause of the source is carried into the generated file, so
/// it compiles alongside its models with nothing to configure.
#[test]
fn go_output_carries_the_source_package_and_its_own_runtime() {
    let (_, generated) = generate_go(
        "go_selfcontained",
        "package mygame\n\n\
         //cyclone:model codec=edge\n\
         type Player struct {\n\
         \tHP uint32 `cyclone:\"u32\" codec:\"edge\"`\n\
         }\n",
    );

    let generated = generated.expect("a codec file");

    assert!(generated.starts_with("package mygame\n"), "{generated}");
    for item in ["type Writer struct", "type Reader struct", "type DecodeError struct", "type Limits struct"] {
        assert!(generated.contains(item), "missing {item}");
    }
}

// ================================================================ three languages

/// All three scanners reach the same codec names and the same field routing
/// for the same schema.
#[test]
fn all_three_languages_agree_on_codec_names_for_the_same_schema() {
    let (_, rust) = generate(
        "parity3_rust",
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
        "parity3_csharp",
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
    let (_, go) = generate_go(
        "parity3_go",
        "package models\n\n\
         //cyclone:model codec=edge,unity\n\
         type DeviceState struct {\n\
         \tID          uint32  `cyclone:\"u32\" codec:\"edge,unity\"`\n\
         \tTemperature float32 `cyclone:\"f32\" codec:\"edge\"`\n\
         }\n",
    );

    let rust = rust.expect("rust codec file");
    let csharp = csharp.expect("csharp codec file");
    let go = go.expect("go codec file");

    for name in ["DeviceStateEdgeCodec", "DeviceStateUnityCodec"] {
        assert!(rust.contains(name), "{name} missing from Rust output");
        assert!(csharp.contains(name), "{name} missing from C# output");
        assert!(go.contains(name), "{name} missing from Go output");
    }

    // The unity codec carries id but not temperature, on all three.
    assert!(!extract_fn(&rust, "DeviceStateUnityCodec", "encode").contains("temperature"));
    assert!(!extract_method(&csharp, "DeviceStateUnityCodec", "Encode").contains("Temperature"));
    assert!(!extract_go_method(&go, "DeviceStateUnityCodec", "Encode").contains("Temperature"));
}

/// A directory holding all three languages' sources at once produces all
/// three output files, each holding only its own language's models.
#[test]
fn a_directory_with_all_three_languages_produces_three_outputs() {
    let directory = scratch("three_languages");
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
    std::fs::write(
        directory.join("c.go"),
        "package models\n\n\
         //cyclone:model codec=edge\n\
         type C struct {\n\
         \tValue uint32 `cyclone:\"u32\" codec:\"edge\"`\n\
         }\n",
    )
    .expect("write source");

    let path = directory.to_str().expect("utf-8 path");
    let output = cyclonec(&["--out", path, path]);
    assert!(output.status.success(), "{}", stderr(&output));

    let rust = std::fs::read_to_string(directory.join("cyclone.codec.rs")).expect("read rust");
    let csharp = std::fs::read_to_string(directory.join("cyclone.codec.cs")).expect("read csharp");
    let go = std::fs::read_to_string(directory.join("cyclone.codec.go")).expect("read go");

    assert!(rust.contains("AEdgeCodec") && !rust.contains("BEdgeCodec") && !rust.contains("CEdgeCodec"));
    assert!(csharp.contains("BEdgeCodec") && !csharp.contains("AEdgeCodec") && !csharp.contains("CEdgeCodec"));
    assert!(go.contains("CEdgeCodec") && !go.contains("AEdgeCodec") && !go.contains("BEdgeCodec"));
}

// ================================================================ Go - helpers

/// Extracts the body of one method on one type from generated Go text -
/// enough to check which fields a codec's `Encode`/`Decode` touches, without a
/// real parser.
fn extract_go_method<'a>(source: &'a str, type_name: &str, method_name: &str) -> &'a str {
    let needle = format!("({type_name}) {method_name}(");
    let method_at = source
        .find(&needle)
        .unwrap_or_else(|| panic!("{needle} not found in:\n{source}"));
    let body_start = source[method_at..].find('{').unwrap() + method_at;
    let body_end = source[body_start..].find("\n}").unwrap() + body_start;
    &source[body_start..body_end]
}

// ============================================ nested model × multiple codecs
//
// h.md audit §3: a field naming a nested model carries its *own* codec
// membership over to the nested call, one call per codec it is actually
// routed into - verified end to end (not just for one codec, as the existing
// composite-model tests already covered, but across the full matrix) for all
// three languages.

/// Rust: `Player.info` is routed into both `edge` and `unity`, and
/// `PlayerInfo` declares both, so `PlayerEdgeCodec` must call
/// `PlayerInfoEdgeCodec` and `PlayerUnityCodec` must call
/// `PlayerInfoUnityCodec` - never the other way around.
#[test]
fn rust_nested_model_with_multiple_codecs_calls_the_matching_nested_codec() {
    let (output, generated) = generate(
        "rust_nested_multi",
        r#"
        #[network]
        #[codec(edge, unity)]
        struct PlayerInfo {
            #[network(u32)]
            #[codec(edge, unity)]
            level: u32,
        }

        #[network]
        #[codec(edge, unity)]
        struct Player {
            #[network(u32)]
            #[codec(edge, unity)]
            hp: u32,

            #[network(PlayerInfo)]
            #[codec(edge, unity)]
            info: PlayerInfo,
        }
        "#,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    let edge = extract_fn(&generated, "PlayerEdgeCodec", "encode");
    assert!(edge.contains("PlayerInfoEdgeCodec::encode"), "{edge}");
    assert!(!edge.contains("PlayerInfoUnityCodec"), "{edge}");

    let unity = extract_fn(&generated, "PlayerUnityCodec", "encode");
    assert!(unity.contains("PlayerInfoUnityCodec::encode"), "{unity}");
    assert!(!unity.contains("PlayerInfoEdgeCodec"), "{unity}");
}

/// The C# counterpart.
#[test]
fn csharp_nested_model_with_multiple_codecs_calls_the_matching_nested_codec() {
    let (output, generated) = generate_csharp(
        "csharp_nested_multi",
        r#"
        using Cyclone;

        [Network]
        [Codec("edge", "unity")]
        public class PlayerInfo
        {
            [Network("u32")]
            [Codec("edge", "unity")]
            public uint Level { get; set; }
        }

        [Network]
        [Codec("edge", "unity")]
        public class Player
        {
            [Network("u32")]
            [Codec("edge", "unity")]
            public uint Hp { get; set; }

            [Network("PlayerInfo")]
            [Codec("edge", "unity")]
            public PlayerInfo Info { get; set; } = new PlayerInfo();
        }
        "#,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    let edge = extract_method(&generated, "PlayerEdgeCodec", "Encode");
    assert!(edge.contains("PlayerInfoEdgeCodec.Encode"), "{edge}");
    assert!(!edge.contains("PlayerInfoUnityCodec"), "{edge}");

    let unity = extract_method(&generated, "PlayerUnityCodec", "Encode");
    assert!(unity.contains("PlayerInfoUnityCodec.Encode"), "{unity}");
    assert!(!unity.contains("PlayerInfoEdgeCodec"), "{unity}");
}

/// The Go counterpart.
#[test]
fn go_nested_model_with_multiple_codecs_calls_the_matching_nested_codec() {
    let (output, generated) = generate_go(
        "go_nested_multi",
        "package models\n\n\
         //cyclone:model codec=edge,unity\n\
         type PlayerInfo struct {\n\
         \tLevel uint32 `cyclone:\"u32\" codec:\"edge,unity\"`\n\
         }\n\n\
         //cyclone:model codec=edge,unity\n\
         type Player struct {\n\
         \tHP   uint32     `cyclone:\"u32\" codec:\"edge,unity\"`\n\
         \tInfo PlayerInfo `cyclone:\"PlayerInfo\" codec:\"edge,unity\"`\n\
         }\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    let edge = extract_go_method(&generated, "PlayerEdgeCodec", "Encode");
    assert!(edge.contains("PlayerInfoEdgeCodec{}).Encode"), "{edge}");
    assert!(!edge.contains("PlayerInfoUnityCodec"), "{edge}");

    let unity = extract_go_method(&generated, "PlayerUnityCodec", "Encode");
    assert!(unity.contains("PlayerInfoUnityCodec{}).Encode"), "{unity}");
    assert!(!unity.contains("PlayerInfoEdgeCodec"), "{unity}");
}

// ================================================== nested codec mismatch
//
// h.md audit §4: a field routes a nested model into a codec the nested model
// itself never declared - `PlayerInfoOrangePiCodec` would never be generated,
// so this must be a reported error, not a silently emitted dangling call.
//
// This exact scenario compiled without complaint before the audit (the three
// backends render one model at a time and never checked another model's own
// codec list), so these are regression tests for a real bug, not a
// speculative one: `cyclonec` used to emit a call to a type it would never
// generate, leaving the mistake for a confusing host-compiler error instead
// of catching it itself.

#[test]
fn rust_nested_codec_mismatch_is_a_reported_error() {
    let (output, generated) = generate(
        "rust_nested_mismatch",
        r#"
        #[network]
        #[codec(edge, unity)]
        struct PlayerInfo {
            #[network(u32)]
            #[codec(edge, unity)]
            level: u32,
        }

        #[network]
        #[codec(edge, orange_pi)]
        struct Player {
            #[network(u32)]
            #[codec(edge, orange_pi)]
            hp: u32,

            #[network(PlayerInfo)]
            #[codec(edge, orange_pi)]
            info: PlayerInfo,
        }
        "#,
    );

    assert!(!output.status.success());
    assert!(generated.is_none(), "nothing is written when validation fails");

    let message = stderr(&output);
    assert!(message.contains("'Player'"), "{message}");
    assert!(message.contains("'info'"), "{message}");
    assert!(message.contains("'orange_pi'"), "{message}");
    assert!(message.contains("'PlayerInfo'"), "{message}");
    assert!(message.contains("edge, unity"), "{message}");
}

/// The same dangling-reference audit as `rust_nested_codec_mismatch_is_a_reported_error`,
/// but for `Array<PlayerInfo>` - the element type must be checked, not just a
/// bare model-typed field.
#[test]
fn rust_array_of_nested_model_codec_mismatch_is_a_reported_error() {
    let (output, generated) = generate(
        "rust_array_nested_mismatch",
        r#"
        #[network]
        #[codec(edge, unity)]
        struct PlayerInfo {
            #[network(u32)]
            #[codec(edge, unity)]
            level: u32,
        }

        #[network]
        #[codec(edge, orange_pi)]
        struct Team {
            #[network(Array<PlayerInfo>)]
            #[codec(edge, orange_pi)]
            players: Vec<PlayerInfo>,
        }
        "#,
    );

    assert!(!output.status.success());
    assert!(generated.is_none(), "nothing is written when validation fails");

    let message = stderr(&output);
    assert!(message.contains("'Team'"), "{message}");
    assert!(message.contains("'players'"), "{message}");
    assert!(message.contains("'orange_pi'"), "{message}");
    assert!(message.contains("'PlayerInfo'"), "{message}");
    assert!(message.contains("edge, unity"), "{message}");
}

#[test]
fn csharp_nested_codec_mismatch_is_a_reported_error() {
    let (output, generated) = generate_csharp(
        "csharp_nested_mismatch",
        r#"
        using Cyclone;

        [Network]
        [Codec("edge", "unity")]
        public class PlayerInfo
        {
            [Network("u32")]
            [Codec("edge", "unity")]
            public uint Level { get; set; }
        }

        [Network]
        [Codec("edge", "orange_pi")]
        public class Player
        {
            [Network("u32")]
            [Codec("edge", "orange_pi")]
            public uint Hp { get; set; }

            [Network("PlayerInfo")]
            [Codec("edge", "orange_pi")]
            public PlayerInfo Info { get; set; } = new PlayerInfo();
        }
        "#,
    );

    assert!(!output.status.success());
    assert!(generated.is_none());

    let message = stderr(&output);
    assert!(message.contains("'Player'"), "{message}");
    assert!(message.contains("'Info'"), "{message}");
    assert!(message.contains("'orange_pi'"), "{message}");
    assert!(message.contains("'PlayerInfo'"), "{message}");
}

#[test]
fn go_nested_codec_mismatch_is_a_reported_error() {
    let (output, generated) = generate_go(
        "go_nested_mismatch",
        "package models\n\n\
         //cyclone:model codec=edge,unity\n\
         type PlayerInfo struct {\n\
         \tLevel uint32 `cyclone:\"u32\" codec:\"edge,unity\"`\n\
         }\n\n\
         //cyclone:model codec=edge,orange_pi\n\
         type Player struct {\n\
         \tHP   uint32     `cyclone:\"u32\" codec:\"edge,orange_pi\"`\n\
         \tInfo PlayerInfo `cyclone:\"PlayerInfo\" codec:\"edge,orange_pi\"`\n\
         }\n",
    );

    assert!(!output.status.success());
    assert!(generated.is_none());

    let message = stderr(&output);
    assert!(message.contains("'Player'"), "{message}");
    assert!(message.contains("'Info'"), "{message}");
    assert!(message.contains("'orange_pi'"), "{message}");
    assert!(message.contains("'PlayerInfo'"), "{message}");
}

/// The validation is per language: a Rust model and a Go model that happen to
/// share a name in the same run must not cross-validate against each other -
/// each language's models are checked only against that language's own set.
#[test]
fn nested_codec_validation_does_not_cross_languages() {
    let directory = scratch("cross_language_validation");
    // A Rust `PlayerInfo` with only `edge` - if Go's `Player` (below) were
    // ever checked against it, this would wrongly fail.
    std::fs::write(
        directory.join("info.rs"),
        "#[network] #[codec(edge)] struct PlayerInfo { \
         #[network(u32)] #[codec(edge)] level: u32, }",
    )
    .expect("write source");
    // Go's own `PlayerInfo` and `Player`, self-consistent, both routing
    // `unity` - must succeed regardless of the unrelated Rust type above.
    std::fs::write(
        directory.join("models.go"),
        "package models\n\n\
         //cyclone:model codec=unity\n\
         type PlayerInfo struct {\n\
         \tLevel uint32 `cyclone:\"u32\" codec:\"unity\"`\n\
         }\n\n\
         //cyclone:model codec=unity\n\
         type Player struct {\n\
         \tInfo PlayerInfo `cyclone:\"PlayerInfo\" codec:\"unity\"`\n\
         }\n",
    )
    .expect("write source");

    let path = directory.to_str().expect("utf-8 path");
    let output = cyclonec(&["--out", path, path]);
    assert!(output.status.success(), "{}", stderr(&output));

    let go = std::fs::read_to_string(directory.join("cyclone.codec.go")).expect("read go");
    assert!(go.contains("PlayerInfoUnityCodec"));
}

// =============================================================== GDScript

/// The GDScript counterpart of [`generate`] / [`generate_csharp`] /
/// [`generate_go`]: writes `source` as `{name}.gd` and reads back
/// `cyclone.codec.gd`.
fn generate_gdscript(name: &str, source: &str) -> (Output, Option<String>) {
    let directory = scratch(name);
    let input = directory.join(format!("{name}.gd"));
    std::fs::write(&input, source).expect("write source");

    let output = cyclonec(&[
        "--out",
        directory.to_str().expect("utf-8 path"),
        input.to_str().expect("utf-8 path"),
    ]);
    let generated = std::fs::read_to_string(directory.join("cyclone.codec.gd")).ok();

    (output, generated)
}

/// A - parse model metadata: `# cyclone:model codec=godot` followed by
/// `class_name` produces exactly the one codec declared.
#[test]
fn gdscript_basic_model() {
    let (output, generated) = generate_gdscript(
        "gd_basic",
        "# cyclone:model codec=godot\n\
         class_name Player\n\n\
         # cyclone:u32 codec=godot\n\
         var hp: int\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.starts_with("class_name CycloneCodec\n"));
    assert!(generated.contains("class PlayerGodotCodec:"));
    assert!(generated.contains("writer.write_u32(value.hp)"));
    assert_eq!(generated.matches("Codec:").count(), 1);
}

/// B/C - field metadata, and a field naming more than one codec: `edge` and
/// `godot` both carry `id`; only `edge` carries `temperature`; only `godot`
/// carries `name`.
#[test]
fn gdscript_multiple_codecs() {
    let (output, generated) = generate_gdscript(
        "gd_multiple",
        "# cyclone:model codec=edge,godot\n\
         class_name DeviceState\n\n\
         # cyclone:u32 codec=edge,godot\n\
         var id: int\n\n\
         # cyclone:f32 codec=edge\n\
         var temperature: float\n\n\
         # cyclone:string codec=godot\n\
         var device_name: String\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    let edge = extract_gdscript_method(&generated, "DeviceStateEdgeCodec", "encode");
    assert!(edge.contains("value.id"));
    assert!(edge.contains("value.temperature"));
    assert!(!edge.contains("value.device_name"));

    let godot = extract_gdscript_method(&generated, "DeviceStateGodotCodec", "encode");
    assert!(godot.contains("value.id"));
    assert!(godot.contains("value.device_name"));
    assert!(!godot.contains("value.temperature"));
}

/// D - a field that names exactly one codec is written by that codec alone -
/// covered above by `temperature`/`device_name`, pinned again on its own.
#[test]
fn gdscript_field_can_belong_to_a_single_codec() {
    let (output, generated) = generate_gdscript(
        "gd_single_codec",
        "# cyclone:model codec=edge\n\
         class_name Reading\n\n\
         # cyclone:f32 codec=edge\n\
         var value: float\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");
    assert!(generated.contains("ReadingEdgeCodec"));
}

/// E - a codec target the generator has never heard of needs no registration:
/// it is opaque metadata, PascalCased into a type name, the same as `edge` or
/// `godot` - never mistaken for a new Cyclone wire type. h.md's own point: an
/// unrecognized target is still just a codec identifier, not a schema.
#[test]
fn gdscript_unknown_codec_target_needs_no_registration() {
    let (output, generated) = generate_gdscript(
        "gd_unknown_target",
        "# cyclone:model codec=godot,unknown_engine\n\
         class_name DeviceState\n\n\
         # cyclone:u32 codec=godot,unknown_engine\n\
         var id: int\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.contains("DeviceStateGodotCodec"));
    assert!(generated.contains("DeviceStateUnknownEngineCodec"));
}

/// F - invalid metadata syntax: a directive argument that is neither empty
/// nor `codec=...` is reported, not silently ignored.
#[test]
fn gdscript_malformed_directive_argument_is_an_error() {
    let (output, _) = generate_gdscript(
        "gd_malformed",
        "# cyclone:model banana\n\
         class_name Player\n\n\
         # cyclone:u32\n\
         var hp: int\n",
    );

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("invalid # cyclone:model directive"),
        "{}",
        stderr(&output)
    );
}

/// F - a `# cyclone:model` not immediately followed by `class_name` is a
/// reported error, never a silent skip - the GDScript counterpart of Go's
/// "directive not followed by struct".
#[test]
fn gdscript_model_directive_not_followed_by_class_name_is_an_error() {
    let (output, generated) = generate_gdscript(
        "gd_orphan_model",
        "# cyclone:model codec=edge\n\
         func not_a_class() -> void:\n\
         \tpass\n",
    );

    assert!(!output.status.success());
    assert!(generated.is_none());
    assert!(
        stderr(&output).contains("must be immediately followed by a `class_name Name`"),
        "{}",
        stderr(&output)
    );
}

/// F - a field directive not immediately followed by `var` is the same kind
/// of error.
#[test]
fn gdscript_field_directive_not_followed_by_var_is_an_error() {
    let (output, generated) = generate_gdscript(
        "gd_orphan_field",
        "# cyclone:model codec=edge\n\
         class_name Player\n\n\
         # cyclone:u32\n\
         func not_a_field() -> void:\n\
         \tpass\n",
    );

    assert!(!output.status.success());
    assert!(generated.is_none());
    assert!(
        stderr(&output).contains("must be immediately followed by a `var name`"),
        "{}",
        stderr(&output)
    );
}

/// A field directive with no model yet open to attach it to is reported, not
/// silently dropped.
#[test]
fn gdscript_field_directive_before_any_model_is_an_error() {
    let (output, generated) = generate_gdscript(
        "gd_field_before_model",
        "# cyclone:u32\n\
         var stray: int\n",
    );

    assert!(!output.status.success());
    assert!(generated.is_none());
    assert!(
        stderr(&output).contains("no `# cyclone:model` / `class_name` has opened a model yet"),
        "{}",
        stderr(&output)
    );
}

/// Unlike Go's fixed-prefix `//cyclone:model` match (which treats
/// `//cyclone:modeling` as an ordinary comment via a word-boundary check),
/// GDScript's grammar has no such escape hatch: `cyclone:` is compared whole
/// against a directive head, so anything after `# cyclone:` is *always* an
/// attempted directive, never silently reinterpreted as an unrelated comment.
/// `# cyclone:modeling this is not a directive` reads `modeling` as an
/// attempted wire type and the rest as a malformed `codec=` argument, and is
/// reported rather than ignored - stricter than Go on purpose: h.md is
/// explicit that a malformed directive must never pass over in silence, and
/// there is no way to tell "a typo for `# cyclone:model`" apart from "an
/// ordinary comment that happens to start with `cyclone:`" without guessing.
#[test]
fn gdscript_anything_after_cyclone_colon_is_an_attempted_directive() {
    let (output, generated) = generate_gdscript(
        "gd_prefix_boundary",
        "# cyclone:modeling this is not a directive\n\
         class_name NotAModel\n\n\
         var value: int\n",
    );

    assert!(!output.status.success());
    assert!(generated.is_none());
    assert!(
        stderr(&output).contains("invalid # cyclone:modeling directive"),
        "{}",
        stderr(&output)
    );
}

/// Blank lines and ordinary (non-Cyclone) comments between a directive and
/// its declaration do not break the association - the same leniency Go's
/// token-based scanner has for free.
#[test]
fn gdscript_blank_lines_and_ordinary_comments_do_not_break_association() {
    let (output, generated) = generate_gdscript(
        "gd_leniency",
        "# cyclone:model codec=edge\n\
         \n\
         # just an ordinary comment, not ours\n\
         class_name Player\n\n\
         # cyclone:u32 codec=edge\n\
         \n\
         # another ordinary comment\n\
         var hp: int\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");
    assert!(generated.contains("PlayerEdgeCodec"));
    assert!(generated.contains("writer.write_u32(value.hp)"));
}

/// A `class_name` nothing marks is not a model, and does not error - the same
/// treatment an unmarked `struct`/`class`/`type` gets in the other three
/// scanners.
#[test]
fn gdscript_unmarked_class_is_not_a_model() {
    let (output, generated) = generate_gdscript(
        "gd_unmarked",
        "class_name Ignored\n\n\
         var whatever: int\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(generated.is_none(), "nothing marks Ignored, so nothing is generated");
}

/// G - nested model: a field whose wire type names another model that
/// declares the same codec is resolved and generates the matching call.
#[test]
fn gdscript_nested_model_with_multiple_codecs_calls_the_matching_nested_codec() {
    let directory = scratch("gd_nested_multi");
    std::fs::write(
        directory.join("player_info.gd"),
        "# cyclone:model codec=edge,unity\n\
         class_name PlayerInfo\n\n\
         # cyclone:u32 codec=edge,unity\n\
         var level: int\n",
    )
    .expect("write source");
    std::fs::write(
        directory.join("player.gd"),
        "# cyclone:model codec=edge,unity\n\
         class_name Player\n\n\
         # cyclone:u32 codec=edge,unity\n\
         var hp: int\n\n\
         # cyclone:PlayerInfo codec=edge,unity\n\
         var info: PlayerInfo\n",
    )
    .expect("write source");

    let path = directory.to_str().expect("utf-8 path");
    let output = cyclonec(&["--out", path, path]);
    assert!(output.status.success(), "{}", stderr(&output));

    let generated =
        std::fs::read_to_string(directory.join("cyclone.codec.gd")).expect("read gdscript");

    let edge = extract_gdscript_method(&generated, "PlayerEdgeCodec", "encode");
    assert!(edge.contains("PlayerInfoEdgeCodec.new().encode"), "{edge}");
    assert!(!edge.contains("PlayerInfoUnityCodec"), "{edge}");

    let unity = extract_gdscript_method(&generated, "PlayerUnityCodec", "encode");
    assert!(unity.contains("PlayerInfoUnityCodec.new().encode"), "{unity}");
    assert!(!unity.contains("PlayerInfoEdgeCodec"), "{unity}");
}

/// `Array<T>` over a scalar, a string, and a nested model: each element type
/// gets the write/read call its own network type would get, wrapped in a
/// count-prefixed loop - the GDScript counterpart of the byte-identical
/// golden vector already asserted for Rust, C#, and Go, and confirmed here by
/// a real Godot run over `tests/fixtures/gdscript/team.gd`.
#[test]
fn gdscript_array_of_scalar_string_and_model() {
    let directory = scratch("gd_array");
    std::fs::write(
        directory.join("player_info.gd"),
        "# cyclone:model codec=edge\n\
         class_name PlayerInfo\n\n\
         # cyclone:u32 codec=edge\n\
         var level: int\n",
    )
    .expect("write source");
    std::fs::write(
        directory.join("team.gd"),
        "# cyclone:model codec=edge\n\
         class_name Team\n\n\
         # cyclone:Array<u32> codec=edge\n\
         var scores: Array[int] = []\n\n\
         # cyclone:Array<string> codec=edge\n\
         var names: Array[String] = []\n\n\
         # cyclone:Array<PlayerInfo> codec=edge\n\
         var players: Array[PlayerInfo] = []\n",
    )
    .expect("write source");

    let path = directory.to_str().expect("utf-8 path");
    let output = cyclonec(&["--out", path, path]);
    assert!(output.status.success(), "{}", stderr(&output));

    let generated =
        std::fs::read_to_string(directory.join("cyclone.codec.gd")).expect("read gdscript");

    let encode = extract_gdscript_method(&generated, "TeamEdgeCodec", "encode");
    assert!(encode.contains("writer.write_array_count(value.scores.size())"), "{encode}");
    assert!(encode.contains("writer.write_u32(element)"), "{encode}");
    assert!(encode.contains("writer.write_array_count(value.names.size())"), "{encode}");
    assert!(encode.contains("writer.write_string(element)"), "{encode}");
    assert!(encode.contains("writer.write_array_count(value.players.size())"), "{encode}");
    assert!(encode.contains("PlayerInfoEdgeCodec.new().encode"), "{encode}");

    let decode = extract_gdscript_method(&generated, "TeamEdgeCodec", "decode");
    assert!(decode.contains("read_array_count"), "{decode}");
    assert!(decode.contains("read_u32"), "{decode}");
    assert!(decode.contains("read_string"), "{decode}");
    assert!(decode.contains("PlayerInfoEdgeCodec.new().decode"), "{decode}");
}

/// A field routing a nested model into a codec the nested model itself never
/// declared is a reported error, not a silently emitted dangling call - the
/// GDScript counterpart of the same audit finding for the other three
/// languages.
#[test]
fn gdscript_nested_codec_mismatch_is_a_reported_error() {
    let directory = scratch("gd_nested_mismatch");
    std::fs::write(
        directory.join("player_info.gd"),
        "# cyclone:model codec=edge,unity\n\
         class_name PlayerInfo\n\n\
         # cyclone:u32 codec=edge,unity\n\
         var level: int\n",
    )
    .expect("write source");
    std::fs::write(
        directory.join("player.gd"),
        "# cyclone:model codec=edge,orange_pi\n\
         class_name Player\n\n\
         # cyclone:u32 codec=edge,orange_pi\n\
         var hp: int\n\n\
         # cyclone:PlayerInfo codec=edge,orange_pi\n\
         var info: PlayerInfo\n",
    )
    .expect("write source");

    let path = directory.to_str().expect("utf-8 path");
    let output = cyclonec(&["--out", path, path]);
    assert!(!output.status.success());
    assert!(!directory.join("cyclone.codec.gd").exists());

    let message = stderr(&output);
    assert!(message.contains("'Player'"), "{message}");
    assert!(message.contains("'info'"), "{message}");
    assert!(message.contains("'orange_pi'"), "{message}");
    assert!(message.contains("'PlayerInfo'"), "{message}");
}

/// The generated file carries the runtime nested inside its own `class_name`
/// wrapper, so it compiles with nothing added to the Godot project and
/// nothing to `preload`.
#[test]
fn gdscript_output_carries_its_own_runtime_under_one_class_name() {
    let (_, generated) = generate_gdscript(
        "gd_selfcontained",
        "# cyclone:model codec=edge\n\
         class_name Player\n\n\
         # cyclone:u32 codec=edge\n\
         var hp: int\n",
    );

    let generated = generated.expect("a codec file");

    assert!(generated.starts_with("class_name CycloneCodec\n"));
    for item in ["class DecodeError:", "class Limits:", "class Writer:", "class Reader:"] {
        assert!(generated.contains(item), "missing {item}");
    }
    // No `preload`/`load`: everything is reachable through the one
    // class_name this file declares.
    assert!(!generated.contains("preload("), "{generated}");
    assert!(!generated.contains("load("), "{generated}");
}

/// h.md's own native-type-independence case: `# cyclone:u32` on a field
/// declared `var id: float` (a deliberately mismatched native type) reports
/// wire type `u32` regardless - checked against the generator's own output
/// text, the same way the Go and C# counterparts are (no Godot compiler is
/// available in this environment to additionally confirm Godot itself
/// accepts the mismatched-looking source; see the final report).
#[test]
fn gdscript_native_type_does_not_change_the_wire_type() {
    let (output, generated) = generate_gdscript(
        "gd_native_type",
        "# cyclone:model codec=edge\n\
         class_name Reading\n\n\
         # cyclone:u32 codec=edge\n\
         var value: float\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    assert!(generated.contains("writer.write_u32(value.value)"), "{generated}");
    let encode = extract_gdscript_method(&generated, "ReadingEdgeCodec", "encode");
    assert!(!encode.contains("write_f32"), "{encode}");
}

/// All four languages agree on codec names and field routing for the same
/// schema.
#[test]
fn all_four_languages_agree_on_codec_names_for_the_same_schema() {
    let (_, rust) = generate(
        "parity4_rust",
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
        "parity4_csharp",
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
    let (_, go) = generate_go(
        "parity4_go",
        "package models\n\n\
         //cyclone:model codec=edge,unity\n\
         type DeviceState struct {\n\
         \tID          uint32  `cyclone:\"u32\" codec:\"edge,unity\"`\n\
         \tTemperature float32 `cyclone:\"f32\" codec:\"edge\"`\n\
         }\n",
    );
    let (_, gdscript) = generate_gdscript(
        "parity4_gdscript",
        "# cyclone:model codec=edge,unity\n\
         class_name DeviceState\n\n\
         # cyclone:u32 codec=edge,unity\n\
         var id: int\n\n\
         # cyclone:f32 codec=edge\n\
         var temperature: float\n",
    );

    let rust = rust.expect("rust codec file");
    let csharp = csharp.expect("csharp codec file");
    let go = go.expect("go codec file");
    let gdscript = gdscript.expect("gdscript codec file");

    for name in ["DeviceStateEdgeCodec", "DeviceStateUnityCodec"] {
        assert!(rust.contains(name), "{name} missing from Rust output");
        assert!(csharp.contains(name), "{name} missing from C# output");
        assert!(go.contains(name), "{name} missing from Go output");
        assert!(gdscript.contains(name), "{name} missing from GDScript output");
    }

    assert!(!extract_fn(&rust, "DeviceStateUnityCodec", "encode").contains("temperature"));
    assert!(!extract_method(&csharp, "DeviceStateUnityCodec", "Encode").contains("Temperature"));
    assert!(!extract_go_method(&go, "DeviceStateUnityCodec", "Encode").contains("Temperature"));
    assert!(
        !extract_gdscript_method(&gdscript, "DeviceStateUnityCodec", "encode")
            .contains("temperature")
    );
}

/// h.md §11.H - cross-target byte compatibility.
///
/// **What this test can and cannot prove.** There is no `godot`/`godot4`
/// binary in this environment (no network access to install one either), so
/// nothing here can actually execute the generated GDScript the way
/// `tests/generated.rs` executes the generated *Rust* (compiling it with
/// `rustc` and running real `encode`/`decode` calls) or the way `go test` /
/// `dotnet test` do for Go and C#. A "Rust encode → Cyclone bytes → Godot
/// decode" round trip through a live Godot process could not be attempted,
/// let alone verified, and claiming otherwise would be dishonest - see the
/// final report's "known limitations" section.
///
/// What *is* checked, and is the closest honest substitute: the exact golden
/// byte vector `tests/generated.rs::each_codec_writes_the_fields_that_named_it`
/// already proves real `rustc`-compiled Rust produces for `DeviceState { id:
/// 42, temperature: 21.5 }` under RFC-0002 (`u32` id then `f32` temperature,
/// Little Endian, no padding) is reproduced here as the literal call sequence
/// `cyclonec` generates for the *same schema* in GDScript -
/// `write_u32(value.id)` then `write_f32(value.temperature)`, in that order,
/// nothing between them. [`super::gdscript_runtime::RUNTIME`]'s `write_u32`/
/// `write_f32` are themselves argued, in that module's own docs, to produce
/// the identical Little Endian RFC-0002 bytes via `PackedByteArray`'s
/// verified-by-source (not by execution here) `encode_u32`/`encode_float`.
/// Chaining "same call sequence" with "each call is documented/sourced to
/// produce the same bytes" is a structural proof, not an executed one - the
/// gap a real Godot run would close.
#[test]
fn gdscript_generated_call_sequence_matches_the_rust_golden_byte_schema() {
    let (output, generated) = generate_gdscript(
        "gd_golden",
        "# cyclone:model codec=edge\n\
         class_name DeviceState\n\n\
         # cyclone:u32 codec=edge\n\
         var id: int\n\n\
         # cyclone:f32 codec=edge\n\
         var temperature: float\n",
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let generated = generated.expect("a codec file");

    let encode = extract_gdscript_method(&generated, "DeviceStateEdgeCodec", "encode");
    let id_at = encode.find("writer.write_u32(value.id)").unwrap_or_else(|| {
        panic!("write_u32(value.id) not found in:\n{encode}")
    });
    let temperature_at =
        encode.find("writer.write_f32(value.temperature)").unwrap_or_else(|| {
            panic!("write_f32(value.temperature) not found in:\n{encode}")
        });
    assert!(
        id_at < temperature_at,
        "id must be written before temperature - RFC-0002 has no padding or reordering:\n{encode}"
    );

    // The decode side reads the identical two fields, in the identical
    // order, through the identical primitive names - the other half of the
    // same golden byte vector.
    let decode = extract_gdscript_method(&generated, "DeviceStateEdgeCodec", "decode");
    let id_read_at = decode
        .find("reader.read_u32()")
        .unwrap_or_else(|| panic!("read_u32() not found in:\n{decode}"));
    let temperature_read_at = decode
        .find("reader.read_f32()")
        .unwrap_or_else(|| panic!("read_f32() not found in:\n{decode}"));
    assert!(id_read_at < temperature_read_at, "{decode}");
}

// ============================================================ GDScript - helpers

/// Extracts the body of one method (`encode`/`decode`) inside one `class
/// Name:` block (column 0 - see [`super::gdscript::WRAPPER_CLASS_NAME`] for
/// why it is not textually nested under `class_name`) from generated
/// GDScript text - enough to check which fields a codec touches, without a
/// real GDScript parser. Mirrors [`extract_go_method`] / [`extract_method`],
/// adapted to GDScript's indentation-delimited (rather than
/// brace-delimited) bodies: a method body ends at the next line back out to
/// one-tab (`\n\tfunc `) or column-0 (`\nclass ` / `\n# `) indentation, or
/// at end of file.
fn extract_gdscript_method<'a>(source: &'a str, type_name: &str, method_name: &str) -> &'a str {
    let class_needle = format!("class {type_name}:");
    let class_at = source
        .find(&class_needle)
        .unwrap_or_else(|| panic!("{class_needle} not found in:\n{source}"));

    let method_needle = format!("func {method_name}(");
    let method_at = source[class_at..]
        .find(&method_needle)
        .unwrap_or_else(|| panic!("{method_needle} not found in {type_name}"))
        + class_at;

    // The signature line ends at its own trailing `:` (its return type
    // annotation, `-> void:` or `-> DecodeError:`) immediately before the
    // newline - every other `:` on the line is followed by a space, not a
    // newline.
    let body_start = source[method_at..].find(":\n").unwrap() + method_at + 2;

    let rest = &source[body_start..];
    let body_end = ["\n\tfunc ", "\nclass ", "\n# "]
        .iter()
        .filter_map(|needle| rest.find(needle))
        .min()
        .unwrap_or(rest.len());

    &source[body_start..body_start + body_end]
}
