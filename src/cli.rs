//! Command-line arguments.
//!
//! Parsed by hand. The surface is six flags, and there is deliberately no
//! `--codec`: a model declares its codecs in the source, so asking for them
//! again on the command line could only ever disagree with it.

use std::ffi::OsString;
use std::path::PathBuf;

/// The usage text, printed by `--help` and on a usage error.
pub const USAGE: &str = "\
cyclonec - the official Cyclone source generator

Reads Cyclone attributes from Rust, C# and Go sources and writes one
self-contained file per language, holding that language's Cyclone runtime and
every codec the models declared. Nothing to import, nothing to add to
Cargo.toml, your .csproj, or go.mod.

USAGE:
    cyclonec --out <PATH> [OPTIONS] <PATH>...

ARGS:
    <PATH>...    Files, or directories to search recursively for `.rs`, `.cs`
                 and `.go` files. Each file's own extension picks its scanner
                 - Rust's `#[network]` for `.rs`, C#'s `[Network]` for `.cs`,
                 Go's `//cyclone:model` + struct tags for `.go`.

OPTIONS:
    -o, --out <PATH>   Where to write. Required.
                         a path ending in `.rs`  →  Rust's exact file
                         a path ending in `.cs`  →  C#'s exact file
                         a path ending in `.go`  →  Go's exact file
                         anything else           →  a directory, holding
                                                    `cyclone.codec.rs`,
                                                    `cyclone.codec.cs` and/or
                                                    `cyclone.codec.go`
                       If sources in a language the extension did not pick
                       are also found, that language's output lands beside it
                       (the same path with its own extension in place).
        --check        Report an out-of-date output file without writing;
                       exit 1 if any is stale. For CI.
        --stdout       Print the generated code instead of writing it.
                       Replaces --out rather than joining it.
    -q, --quiet        Report only errors
    -h, --help         Print this message
    -V, --version      Print the version

EXAMPLES:
    cyclonec --out src/ src/                  → src/cyclone.codec.{rs,cs,go}
    cyclonec --out src/net.rs src/models.rs   → src/net.rs
    cyclonec --stdout src/                    → stdout

A model declares which codecs to generate; there is no flag for it. Rust:

    #[network]
    #[codec(edge, unity)]        →  DeviceStateEdgeCodec, DeviceStateUnityCodec
    struct DeviceState {
        #[network(u32)]
        #[codec(edge, unity)]    →  in both
        id: u32,

        #[network(f32)]
        #[codec(edge)]           →  in the edge codec only
        temperature: f32,
    }

The same schema, in C# (from the `cyclone-attributes` package, namespace
`Cyclone`) - same codecs, same routing, same bytes on the wire:

    [Network]
    [Codec(\"edge\", \"unity\")]
    public class DeviceState
    {
        [Network(\"u32\")]
        [Codec(\"edge\", \"unity\")]
        public uint Id { get; set; }

        [Network(\"f32\")]
        [Codec(\"edge\")]
        public float Temperature { get; set; }
    }

And in Go, which has no attributes: a `//cyclone:model` comment directive
names the model and its codecs, and `cyclone:\"...\"` / `codec:\"...\"` struct
tags name each field's wire type and codec membership - same codecs, same
routing, same bytes:

    //cyclone:model codec=edge,unity
    type DeviceState struct {
        ID          uint32  `cyclone:\"u32\" codec:\"edge,unity\"`
        Temperature float32 `cyclone:\"f32\" codec:\"edge\"`
    }
";

/// What the generator was asked to do.
pub struct Args {
    /// The files and directories to read.
    pub paths: Vec<PathBuf>,
    /// Where to write. `None` only when [`stdout`](Self::stdout) is set.
    ///
    /// Required otherwise: the output is one file holding every codec, and
    /// guessing where a whole project's codecs belong is not the generator's
    /// call to make.
    pub out: Option<PathBuf>,
    /// Report staleness instead of writing.
    pub check: bool,
    /// Print instead of writing.
    pub stdout: bool,
    /// Suppress the per-file report.
    pub quiet: bool,
}

/// What to do instead of generating, when a flag says so.
pub enum Parsed {
    /// Generate with these arguments.
    Run(Args),
    /// Print the usage text and stop.
    Help,
    /// Print the version and stop.
    Version,
}

/// Parses the command line.
///
/// # Errors
///
/// An unknown flag, a missing value, or no input path.
pub fn parse(argv: impl IntoIterator<Item = OsString>) -> Result<Parsed, String> {
    let mut paths = Vec::new();
    let mut out = None;
    let mut check = false;
    let mut stdout = false;
    let mut quiet = false;

    let mut argv = argv.into_iter();
    while let Some(argument) = argv.next() {
        let text = argument.to_string_lossy().into_owned();
        match text.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "--check" => check = true,
            "--stdout" => stdout = true,
            "-q" | "--quiet" => quiet = true,
            "-o" | "--out" => {
                let value = argv.next().ok_or_else(|| format!("{text} needs a path"))?;
                out = Some(PathBuf::from(value));
            }
            flag if flag.starts_with('-') && flag != "-" => {
                return Err(format!("unknown option `{flag}`"));
            }
            _ => paths.push(PathBuf::from(argument)),
        }
    }

    if paths.is_empty() {
        return Err("no input path".to_owned());
    }
    if check && stdout {
        return Err("--check and --stdout do opposite things; pick one".to_owned());
    }
    if stdout && out.is_some() {
        return Err("--stdout writes nothing, so --out has no effect".to_owned());
    }
    if !stdout && out.is_none() {
        return Err(
            "--out is required: say where the generated file goes (a directory, or a \
             path ending in `.rs`)"
                .to_owned(),
        );
    }

    Ok(Parsed::Run(Args { paths, out, check, stdout, quiet }))
}
