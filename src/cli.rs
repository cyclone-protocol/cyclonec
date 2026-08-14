//! Command-line arguments, parsed by hand.
//!
//! Three commands, because they are three different jobs with three different
//! failure modes:
//!
//! - `generate` reads source and writes the tree. It **warns** about
//!   compatibility and never fails for it - a developer breaking their own
//!   schema on purpose is a normal afternoon.
//! - `compat` compares two schemas and says what changed. It fails on a
//!   breaking change, because somebody asked it the question.
//! - `ci` does what a pull request needs: check the committed schema still
//!   matches source, fetch the *target branch's* schema, compare, and exit 1 on
//!   a breaking change.

use std::ffi::OsString;
use std::path::PathBuf;

/// The usage text, printed by `--help` and on a usage error.
pub const USAGE: &str = "\
cyclonec - the official Cyclone source generator

Reads Cyclone attributes from Rust, Go, C#, GDScript, C++, C, TypeScript or
JavaScript sources - never more than one language in one run - and writes one
codec file per model per codec, plus the schema, fingerprints and build graph
that go with them.

USAGE:
    cyclonec generate [--src <PATH>]... [--out <PATH>] [--check] [-q]
    cyclonec compat --base <SCHEMA> [--head <SCHEMA>] [--src <PATH>]...
    cyclonec ci --base-ref <REF> [--src <PATH>]... [--out <PATH>]

COMMANDS:
    generate    Read source, write the tree, .cyclone/schema.json and
                .cyclone/build-graph.json. Warns about schema changes; never
                fails because of one.
    compat      Compare a base schema against the current source (or against
                --head). Exits 1 on a BREAKING change.
    ci          Verify .cyclone/schema.json matches source, then compare it
                against the target branch's. Exits 1 on either problem.

OPTIONS:
        --src <PATH>     A directory to scan recursively, or a single file.
                         Repeatable. Default: cyclone.toml's `src`, else `src`.
                         Every file found must be one language - `.rs`, `.go`,
                         `.cs`, `.gd`, `.hpp`/`.cpp`/`.cc`/`.cxx` (C++),
                         `.c`/`.h` (C), or `.ts`/`.js` (TypeScript/
                         JavaScript) - never a mix; separate projects sharing
                         one schema each get their own `--src`/`--out` (and
                         usually their own cyclone.toml).
    -o, --out <PATH>     Where generated source goes.
                         Default: cyclone.toml's `out`, else `generated`.
        --model-path <PATH>
                         Where a generated codec reaches your models.
                         Rust: a module path, e.g. `crate::models` - default is
                         the one the source layout implies (src/models/player.rs
                         is crate::models::player).
                         Go: an import path, e.g. `github.com/acme/game/models`
                         - default is computed from the nearest go.mod's
                         `module` line plus the source's own directory.
                         C#: a namespace, e.g. `Game.Models` - default is the
                         `namespace` the model's own source declares (or none,
                         for a model in no namespace at all).
                         C++: a namespace too, e.g. `Game::Models` - default
                         is the first `namespace` the model's own source
                         opens (or none). Never affects the `#include` path a
                         generated header needs, which is always the model's
                         own source path (e.g. `src/models/player.hpp`) -
                         point your compiler's `-I` at the directory that
                         path is itself relative to, typically the project
                         root.
                         GDScript and C: no effect. GDScript, because a
                         model's own `class_name` is already reachable
                         project-wide with nothing to override; C, because it
                         has no namespace at all, only the same physical
                         `#include` path C++ has above (and, like C++'s,
                         never overridden by this option).
                         TypeScript/JavaScript: an ES module specifier, e.g.
                         `@/models` - every model is imported from that one
                         module (a barrel re-exporting each of them) in place
                         of the default, which computes a relative `import`
                         straight from each model's own source path.
        --check          Write nothing; exit 1 if anything on disk is out of
                         date. For CI, and for a pre-commit hook.
        --base <SCHEMA>  A schema.json to compare against. `compat` only.
        --head <SCHEMA>  A schema.json to compare, instead of reading source.
        --base-ref <REF> The target branch a pull request merges into, e.g.
                         `origin/develop`. `ci` only, and never defaulted -
                         hard-coding `main` is how a develop-based repository
                         gets a green CI that compared nothing.
    -q, --quiet          Report only what is wrong.
    -h, --help           Print this message
    -V, --version        Print the version

EXAMPLES:
    cyclonec generate --src src --out generated
    cyclonec generate --check
    cyclonec compat --base .cyclone/schema.json
    cyclonec ci --base-ref origin/${GITHUB_BASE_REF}

A model declares which codecs to generate; there is no flag for it. Rust:

    #[network]
    #[codec(edge, unity)]        ->  PlayerEdgeCodec, PlayerUnityCodec
    struct Player {
        #[network(u32)]
        #[codec(edge, unity)]    ->  in both
        id: u32,

        #[network(f32)]
        #[codec(edge)]           ->  in the edge codec only
        x: f32,
    }

Go has no attributes, so a `//cyclone:model` comment directive and
`cyclone:\"...\"` / `codec:\"...\"` struct tags say the same thing:

    //cyclone:model codec=edge,unity
    type Player struct {
        ID uint32  `cyclone:\"u32\" codec:\"edge,unity\"`
        X  float32 `cyclone:\"f32\" codec:\"edge\"`
    }

C# spells the same declaration with attributes, the same shape as Rust's:

    [Network]
    [Codec(\"edge\", \"unity\")]      ->  PlayerEdgeCodec, PlayerUnityCodec
    public class Player
    {
        [Network(\"u32\")]
        [Codec(\"edge\", \"unity\")]  ->  in both
        public uint Id { get; set; }

        [Network(\"f32\")]
        [Codec(\"edge\")]             ->  in the edge codec only
        public float X { get; set; }
    }

GDScript has no attributes either, and an unrecognized `@name` is a parse
error in Godot itself - so, like Go, a `# cyclone:` comment directive says it
instead:

    # cyclone:model codec=edge,unity
    class_name Player

    # cyclone:u32 codec=edge,unity
    var id: int

    # cyclone:f32 codec=edge
    var x: float

C++ has no attributes either, and no comment-directive syntax to fall back
on - it spells the same declaration with three macros a small header (see
`cyclone.h` in the brief) defines to expand to nothing, so an annotated
struct compiles unchanged whether or not cyclonec ever runs over it:

    CYCLONE_MODEL
    CYCLONE_CODEC(\"edge\", \"unity\")      ->  PlayerEdgeCodec, PlayerUnityCodec
    struct Player
    {
        CYCLONE_FIELD(u32)
        CYCLONE_CODEC(\"edge\", \"unity\")  ->  in both
        uint32_t Id;

        CYCLONE_FIELD(f32)
        CYCLONE_CODEC(\"edge\")             ->  in the edge codec only
        float X;
    };

C reads the same three macros, from the same header - a `string` field's
host type is always `const char *` (heap-owned once decoded; see the C
section of the README for why), and there is no `namespace` to open at all:

    CYCLONE_MODEL
    CYCLONE_CODEC(\"edge\", \"unity\")      ->  PlayerEdgeCodec, PlayerUnityCodec
    struct Player
    {
        CYCLONE_FIELD(u32)
        CYCLONE_CODEC(\"edge\", \"unity\")  ->  in both
        uint32_t Id;

        CYCLONE_FIELD(string)
        CYCLONE_CODEC(\"edge\")             ->  in the edge codec only
        const char *Name;
    };

TypeScript and JavaScript have neither attributes nor macros usable without a
runtime dependency, so - like Go and GDScript - a comment directive says it,
read the same way for both languages (`.ts` and `.js`) and requiring no
decorator and no package to install:

    // CYCLONE_MODEL
    // CYCLONE_CODEC(\"edge\", \"unity\")      ->  PlayerEdgeCodec, PlayerUnityCodec
    class Player {
        // CYCLONE_FIELD(u32)
        // CYCLONE_CODEC(\"edge\", \"unity\")  ->  in both
        Id: number;

        // CYCLONE_FIELD(f32)
        // CYCLONE_CODEC(\"edge\")             ->  in the edge codec only
        X: number;
    }

The TypeScript host type (`number`, above) is never consulted - `number`
cannot say whether a field is `u32`, `i32`, `f32` or `f64` - only
CYCLONE_FIELD's own argument is. A JavaScript model writes the identical
directives with no type annotation at all (`Id;` in place of `Id: number;`)
and means exactly the same thing.
";

/// Where to read from and where to write to - shared by every command that
/// touches source.
#[derive(Debug, Default, Clone)]
pub struct Paths {
    /// `--src`, empty if the flag was never given.
    pub src: Vec<PathBuf>,
    /// `--out`, if given.
    pub out: Option<PathBuf>,
    /// `--model-path`, if given: overrides how a generated codec reaches your
    /// models - a module path in Rust, an import path in Go, a namespace in
    /// C# or C++ - in place of the one the source layout (Rust), `go.mod`
    /// (Go), or the model's own `namespace` (C#, C++) implies. No effect on
    /// GDScript or C, neither of which has anything to override.
    pub model_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GenerateArgs {
    pub paths: Paths,
    /// Report staleness instead of writing.
    pub check: bool,
    pub quiet: bool,
}

#[derive(Debug, Clone)]
pub struct CompatArgs {
    /// The schema being evolved from.
    pub base: PathBuf,
    /// The schema being evolved to. `None` reads the current source instead,
    /// which is what makes this useful before anything has been committed.
    pub head: Option<PathBuf>,
    pub paths: Paths,
    pub quiet: bool,
}

#[derive(Debug, Clone)]
pub struct CiArgs {
    /// The git ref of the branch this change would merge into.
    pub base_ref: String,
    pub paths: Paths,
    pub quiet: bool,
}

/// What the command line asked for.
#[derive(Debug, Clone)]
pub enum Command {
    Generate(GenerateArgs),
    Compat(CompatArgs),
    Ci(CiArgs),
    Help,
    Version,
}

/// Parses the command line.
///
/// # Errors
///
/// An unknown flag, a missing value, or a required argument that was not given.
pub fn parse(argv: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let mut argv: Vec<String> = argv
        .into_iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();

    // No command at all means `generate`: it is the one that runs every build.
    let command = match argv.first().map(String::as_str) {
        Some("generate") | Some("compat") | Some("ci") => argv.remove(0),
        _ => "generate".to_owned(),
    };

    let mut paths = Paths::default();
    let mut check = false;
    let mut quiet = false;
    let mut base = None;
    let mut head = None;
    let mut base_ref = None;

    let mut arguments = argv.into_iter();
    while let Some(argument) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .ok_or_else(|| format!("{argument} needs a value"))
        };

        match argument.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--check" => check = true,
            "-q" | "--quiet" => quiet = true,
            "--src" => paths.src.push(PathBuf::from(value()?)),
            "-o" | "--out" => paths.out = Some(PathBuf::from(value()?)),
            "--model-path" => paths.model_path = Some(value()?),
            "--base" => base = Some(PathBuf::from(value()?)),
            "--head" => head = Some(PathBuf::from(value()?)),
            "--base-ref" => base_ref = Some(value()?),
            other => return Err(format!("unknown option `{other}`")),
        }
    }

    match command.as_str() {
        "generate" => Ok(Command::Generate(GenerateArgs {
            paths,
            check,
            quiet,
        })),
        "compat" => Ok(Command::Compat(CompatArgs {
            base: base.ok_or("compat needs --base <SCHEMA>: the schema to compare against")?,
            head,
            paths,
            quiet,
        })),
        "ci" => Ok(Command::Ci(CiArgs {
            base_ref: base_ref.ok_or(
                "ci needs --base-ref <REF>: the branch this change merges into, \
                 e.g. `origin/${GITHUB_BASE_REF}`",
            )?,
            paths,
            quiet,
        })),
        other => Err(format!("unknown command `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{parse, Command};

    fn command(argv: &[&str]) -> Command {
        parse(argv.iter().map(OsString::from)).expect("parse")
    }

    #[test]
    fn generate_is_the_default_command() {
        let Command::Generate(arguments) = command(&[]) else {
            panic!("not generate");
        };
        assert!(arguments.paths.src.is_empty());
        assert!(!arguments.check);
    }

    #[test]
    fn generate_takes_its_paths() {
        let Command::Generate(arguments) = command(&["generate", "--src", "src", "--out", "gen"])
        else {
            panic!("not generate");
        };
        assert_eq!(arguments.paths.src, [PathBuf::from("src")]);
        assert_eq!(arguments.paths.out, Some(PathBuf::from("gen")));
    }

    #[test]
    fn src_is_repeatable() {
        let Command::Generate(arguments) = command(&["--src", "a", "--src", "b"]) else {
            panic!("not generate");
        };
        assert_eq!(
            arguments.paths.src,
            [PathBuf::from("a"), PathBuf::from("b")]
        );
    }

    #[test]
    fn compat_requires_a_base() {
        assert!(parse(["compat"].iter().map(OsString::from)).is_err());

        let Command::Compat(arguments) = command(&["compat", "--base", "old.json"]) else {
            panic!("not compat");
        };
        assert_eq!(arguments.base, PathBuf::from("old.json"));
        assert_eq!(arguments.head, None);
    }

    #[test]
    fn ci_requires_a_base_ref_and_never_defaults_it() {
        let error = parse(["ci"].iter().map(OsString::from)).expect_err("no base ref");
        assert!(error.contains("--base-ref"), "{error}");
        assert!(!error.contains("main"), "{error}");

        let Command::Ci(arguments) = command(&["ci", "--base-ref", "origin/develop"]) else {
            panic!("not ci");
        };
        assert_eq!(arguments.base_ref, "origin/develop");
    }

    #[test]
    fn an_unknown_flag_is_refused() {
        assert!(parse(["--codec", "edge"].iter().map(OsString::from)).is_err());
    }

    #[test]
    fn help_and_version_win_wherever_they_appear() {
        assert!(matches!(command(&["generate", "--help"]), Command::Help));
        assert!(matches!(command(&["-V"]), Command::Version));
    }
}
