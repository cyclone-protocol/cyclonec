//! The official Cyclone **source generator**.
//!
//! ```text
//! parse  →  collect  →  generate
//! ```
//!
//! `cyclonec` is not a compiler. It reads Cyclone attributes out of source
//! files — Rust's `#[network]` / `#[codec(...)]`, or C#'s `[Network]` /
//! `[Codec(...)]` — and writes one self-contained file per language holding
//! that language's Cyclone runtime and the Encode / Decode calls each model
//! declared, then exits — the way `protoc` does.
//!
//! It builds no schema, no IR, no type graph, no codec graph and no dependency
//! graph; it runs no semantic analysis and makes no second pass. There is no
//! registry, no reflection and no runtime resolution. The runtime each backend
//! emits is a fixed block written once against RFC-0002 (see [`generator`]) —
//! nothing about the wire format is worked out here, in either language.
//!
//! Rust and C# are read by independent scanners ([`parser::rust`],
//! [`parser::csharp`]) into the identical [`model::Model`] shape, so a schema
//! written in either language produces the same codec names, the same field
//! routing, and the same bytes on the wire.

mod cli;
mod generator;
mod model;
mod parser;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cli::Parsed;
use model::{Language, Model};

fn main() -> ExitCode {
    let parsed = match cli::parse(std::env::args_os().skip(1)) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("cyclonec: {message}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };

    let arguments = match parsed {
        Parsed::Run(arguments) => arguments,
        Parsed::Help => {
            print!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Parsed::Version => {
            println!("cyclonec {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
    };

    match run(&arguments) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("cyclonec: {message}");
            ExitCode::FAILURE
        }
    }
}

/// One language's worth of what there is to write: the rendered text, the
/// destination, and the codec names for the report line.
struct Unit {
    contents: String,
    output: Option<PathBuf>,
    codecs: Vec<String>,
}

fn run(arguments: &cli::Args) -> Result<ExitCode, String> {
    let mut sources = Vec::new();
    for path in &arguments.paths {
        collect(path, &mut sources)?;
    }
    sources.sort();
    sources.dedup();

    // Every source feeds one of two output files (one per language), so both
    // destinations are resolved before anything is read: a run that cannot
    // write should not parse first.
    let (rust_out, csharp_out) = arguments.out.as_deref().map(resolve_outputs).unzip();

    let rust_file_name = file_name_of(rust_out.as_deref(), generator::rust::DEFAULT_FILE_NAME);
    let csharp_file_name =
        file_name_of(csharp_out.as_deref(), generator::csharp::DEFAULT_FILE_NAME);

    let mut rust_models = Vec::new();
    let mut csharp_models = Vec::new();
    let mut rust_sources = Vec::new();
    let mut csharp_sources = Vec::new();
    let mut failures = 0;

    for source in &sources {
        match read(source) {
            Ok(parsed) => {
                let source_name =
                    source.file_name().unwrap_or_default().to_string_lossy().into_owned();

                // A file parses entirely as one language — the extension picked
                // the scanner — so every model it yielded shares a language.
                match parsed.first().map(|model| model.language) {
                    Some(Language::Rust) => rust_sources.push(source_name),
                    Some(Language::CSharp) => csharp_sources.push(source_name),
                    None => {}
                }

                for model in parsed {
                    match model.language {
                        Language::Rust => rust_models.push(model),
                        Language::CSharp => csharp_models.push(model),
                    }
                }
            }
            Err(message) => {
                eprintln!("error: {message}");
                failures += 1;
            }
        }
    }

    if failures > 0 {
        return Err(format!("{failures} file(s) failed"));
    }

    let rust_unit = generator::rust::render(&rust_sources, &rust_models, &rust_file_name)
        .map(|contents| Unit { contents, output: rust_out, codecs: codec_names(&rust_models) });
    let csharp_unit =
        generator::csharp::render(&csharp_sources, &csharp_models, &csharp_file_name).map(
            |contents| Unit { contents, output: csharp_out, codecs: codec_names(&csharp_models) },
        );

    let units: Vec<Unit> = [rust_unit, csharp_unit].into_iter().flatten().collect();

    if units.is_empty() {
        if !arguments.quiet {
            eprintln!("cyclonec: no model declared a codec; nothing to write");
        }
        return Ok(ExitCode::SUCCESS);
    }

    if arguments.stdout {
        return print_all(&units);
    }

    if arguments.check {
        return check_all(&units, arguments.quiet);
    }
    write_all(&units, arguments.quiet)
}

/// Turns `--out` into a destination for each language: `(rust, csharp)`.
///
/// A path ending in `.rs` is Rust's exact file, and C#'s becomes the same path
/// with `.cs` in its place (used only if C# models are actually present). A
/// path ending in `.cs` is the mirror image. Anything else — most commonly a
/// directory — holds both languages' default names side by side. The rule is
/// the extension and not whether the path happens to exist, so a first run and
/// a second run agree.
fn resolve_outputs(out: &Path) -> (PathBuf, PathBuf) {
    match out.extension().and_then(|extension| extension.to_str()) {
        Some(generator::rust::EXTENSION) => (out.to_path_buf(), out.with_extension("cs")),
        Some(generator::csharp::EXTENSION) => (out.with_extension("rs"), out.to_path_buf()),
        _ => {
            (out.join(generator::rust::DEFAULT_FILE_NAME), out.join(generator::csharp::DEFAULT_FILE_NAME))
        }
    }
}

fn file_name_of(path: Option<&Path>, default: &str) -> String {
    path.and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| default.to_owned())
}

/// Reads one source file into its models.
fn read(source: &Path) -> Result<Vec<Model>, String> {
    let text =
        fs::read_to_string(source).map_err(|error| format!("{}: {error}", source.display()))?;
    parser::parse(source, &text).map_err(|error| error.to_string())
}

/// Collects `.rs` and `.cs` files, skipping `target` and the generator's own
/// output.
fn collect(path: &Path, sources: &mut Vec<PathBuf>) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;

    // A file named explicitly is read even if its name says otherwise; the
    // caller said so. Discovery inside a directory is the filtered case.
    if metadata.is_file() {
        sources.push(path.to_path_buf());
        return Ok(());
    }

    if path.file_name().is_some_and(|name| name == "target") {
        return Ok(());
    }

    let entries = fs::read_dir(path).map_err(|error| format!("{}: {error}", path.display()))?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", path.display()))?.path();

        if entry.is_dir() {
            collect(&entry, sources)?;
        } else if is_source(&entry) {
            sources.push(entry);
        }
    }

    Ok(())
}

/// Whether a discovered file is source the generator did not write itself.
///
/// Reading its own output back in would be a loop, and the output now carries a
/// runtime that would parse as a pile of ordinary types.
fn is_source(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    (name.ends_with(".rs") && !name.ends_with(".codec.rs"))
        || (name.ends_with(".cs") && !name.ends_with(".codec.cs"))
}

fn codec_names(models: &[Model]) -> Vec<String> {
    models
        .iter()
        .flat_map(|item| {
            item.codecs
                .iter()
                .map(|codec| format!("{}{}Codec", item.name, model::pascal_case(codec)))
        })
        .collect()
}

fn print_all(units: &[Unit]) -> Result<ExitCode, String> {
    let mut stdout = std::io::stdout().lock();
    for unit in units {
        write!(stdout, "{}", unit.contents).map_err(|error| error.to_string())?;
    }
    Ok(ExitCode::SUCCESS)
}

/// Reports whether each file on disk still matches its sources.
///
/// The CI mode: an attribute change that was not regenerated means the
/// committed file encodes something other than what the source declares.
fn check_all(units: &[Unit], quiet: bool) -> Result<ExitCode, String> {
    let mut failed = false;

    for unit in units {
        let output = unit.output.as_deref().expect("--out is required unless --stdout");

        if fs::read_to_string(output).ok().as_deref() == Some(unit.contents.as_str()) {
            if !quiet {
                eprintln!("cyclonec: {} is up to date", output.display());
            }
            continue;
        }

        eprintln!("stale: {} does not match its sources", output.display());
        failed = true;
    }

    if failed {
        eprintln!("cyclonec: run `cyclonec` to regenerate");
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

/// Writes each file, unless it is already identical.
///
/// Skipping matters: rewriting an unchanged file bumps its mtime and makes
/// every build that watches it rebuild for nothing.
fn write_all(units: &[Unit], quiet: bool) -> Result<ExitCode, String> {
    for unit in units {
        let output = unit.output.as_deref().expect("--out is required unless --stdout");

        if let Some(directory) = output.parent() {
            if !directory.as_os_str().is_empty() {
                fs::create_dir_all(directory)
                    .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
            }
        }

        if fs::read_to_string(output).ok().as_deref() == Some(unit.contents.as_str()) {
            if !quiet {
                eprintln!("cyclonec: {} unchanged", output.display());
            }
            continue;
        }

        fs::write(output, &unit.contents)
            .map_err(|error| format!("cannot write {}: {error}", output.display()))?;

        if !quiet {
            eprintln!("cyclonec: {} ({})", output.display(), unit.codecs.join(", "));
        }
    }

    Ok(ExitCode::SUCCESS)
}
