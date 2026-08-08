//! The official Cyclone **source generator**.
//!
//! ```text
//! parse  →  collect  →  generate
//! ```
//!
//! `cyclonec` is not a compiler. It reads Cyclone attributes out of source
//! files - Rust's `#[network]` / `#[codec(...)]`, C#'s `[Network]` /
//! `[Codec(...)]`, or Go's `//cyclone:model` directive and `cyclone:"..."` /
//! `codec:"..."` struct tags - and writes one self-contained file per language
//! holding that language's Cyclone runtime and the Encode / Decode calls each
//! model declared, then exits - the way `protoc` does.
//!
//! It builds no schema, no IR, no type graph, no codec graph and no dependency
//! graph; it runs no semantic analysis and makes no second pass. There is no
//! registry, no reflection and no runtime resolution. The runtime each backend
//! emits is a fixed block written once against RFC-0002 (see [`generator`]) -
//! nothing about the wire format is worked out here, in any language.
//!
//! Rust, C# and Go are read by three independent scanners ([`parser::rust`],
//! [`parser::csharp`], [`parser::go`]) into the identical [`model::Model`]
//! shape, so a schema written in any of them produces the same codec names, the
//! same field routing, and the same bytes on the wire.

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

/// Everything gathered from the sources for one language, before rendering.
#[derive(Default)]
struct Collected {
    models: Vec<Model>,
    source_names: Vec<String>,
}

fn run(arguments: &cli::Args) -> Result<ExitCode, String> {
    let mut sources = Vec::new();
    for path in &arguments.paths {
        collect(path, &mut sources)?;
    }
    sources.sort();
    sources.dedup();

    // Every source feeds one of three output files (one per language), so all
    // three destinations are resolved before anything is read: a run that
    // cannot write should not parse first.
    let (rust_out, csharp_out, go_out, gdscript_out) =
        match arguments.out.as_deref().map(resolve_outputs) {
            Some((rust, csharp, go, gdscript)) => {
                (Some(rust), Some(csharp), Some(go), Some(gdscript))
            }
            None => (None, None, None, None),
        };

    let rust_file_name = file_name_of(rust_out.as_deref(), generator::rust::DEFAULT_FILE_NAME);
    let csharp_file_name =
        file_name_of(csharp_out.as_deref(), generator::csharp::DEFAULT_FILE_NAME);
    let go_file_name = file_name_of(go_out.as_deref(), generator::go::DEFAULT_FILE_NAME);
    let gdscript_file_name =
        file_name_of(gdscript_out.as_deref(), generator::gdscript::DEFAULT_FILE_NAME);

    let mut rust = Collected::default();
    let mut csharp = Collected::default();
    let mut go = Collected::default();
    let mut gdscript = Collected::default();
    let mut go_package: Option<String> = None;
    let mut failures = 0;

    for source in &sources {
        match read(source) {
            Ok((parsed, text)) => {
                let source_name =
                    source.file_name().unwrap_or_default().to_string_lossy().into_owned();

                // A file parses entirely as one language - the extension picked
                // the scanner - so every model it yielded shares a language.
                let collected = match parsed.first().map(|model| model.language) {
                    Some(Language::Rust) => Some(&mut rust),
                    Some(Language::CSharp) => Some(&mut csharp),
                    Some(Language::Go) => Some(&mut go),
                    Some(Language::GDScript) => Some(&mut gdscript),
                    None => None,
                };
                if let Some(collected) = collected {
                    collected.source_names.push(source_name);
                }

                // Go compiles by directory: every file in the output's package
                // must declare it, so the first Go source that names one lends
                // its name to the generated file too.
                if go_package.is_none() && source.extension().is_some_and(|ext| ext == "go") {
                    go_package = parser::go::package_name(&text);
                }

                for model in parsed {
                    match model.language {
                        Language::Rust => rust.models.push(model),
                        Language::CSharp => csharp.models.push(model),
                        Language::Go => go.models.push(model),
                        Language::GDScript => gdscript.models.push(model),
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

    // A nested-model field must route only into codecs the referenced model
    // itself declares, or the generated call names a type nothing will ever
    // generate. Checked per language, against that language's own models,
    // before any of them is rendered - see `model::check_nested_codecs`.
    model::check_nested_codecs(&rust.models)?;
    model::check_nested_codecs(&csharp.models)?;
    model::check_nested_codecs(&go.models)?;
    model::check_nested_codecs(&gdscript.models)?;

    let rust_unit = generator::rust::render(&rust.source_names, &rust.models, &rust_file_name)
        .map(|contents| Unit { contents, output: rust_out, codecs: codec_names(&rust.models) });
    let csharp_unit =
        generator::csharp::render(&csharp.source_names, &csharp.models, &csharp_file_name).map(
            |contents| Unit { contents, output: csharp_out, codecs: codec_names(&csharp.models) },
        );
    let go_unit = generator::go::render(
        &go.source_names,
        &go.models,
        &go_file_name,
        go_package.as_deref(),
    )
    .map(|contents| Unit { contents, output: go_out, codecs: codec_names(&go.models) });
    let gdscript_unit = generator::gdscript::render(
        &gdscript.source_names,
        &gdscript.models,
        &gdscript_file_name,
    )
    .map(|contents| Unit {
        contents,
        output: gdscript_out,
        codecs: codec_names(&gdscript.models),
    });

    let units: Vec<Unit> =
        [rust_unit, csharp_unit, go_unit, gdscript_unit].into_iter().flatten().collect();

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

/// Turns `--out` into a destination for each language: `(rust, csharp, go,
/// gdscript)`.
///
/// A path ending in one language's extension is that language's exact file,
/// and the other three become the same path with their own extension in place
/// (used only if that language's models are actually present). Anything else -
/// most commonly a directory - holds all four languages' default names side
/// by side. The rule is the extension and not whether the path happens to
/// exist, so a first run and a second run agree.
fn resolve_outputs(out: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    match out.extension().and_then(|extension| extension.to_str()) {
        Some(generator::rust::EXTENSION) => (
            out.to_path_buf(),
            out.with_extension("cs"),
            out.with_extension("go"),
            out.with_extension("gd"),
        ),
        Some(generator::csharp::EXTENSION) => (
            out.with_extension("rs"),
            out.to_path_buf(),
            out.with_extension("go"),
            out.with_extension("gd"),
        ),
        Some(generator::go::EXTENSION) => (
            out.with_extension("rs"),
            out.with_extension("cs"),
            out.to_path_buf(),
            out.with_extension("gd"),
        ),
        Some(generator::gdscript::EXTENSION) => (
            out.with_extension("rs"),
            out.with_extension("cs"),
            out.with_extension("go"),
            out.to_path_buf(),
        ),
        _ => (
            out.join(generator::rust::DEFAULT_FILE_NAME),
            out.join(generator::csharp::DEFAULT_FILE_NAME),
            out.join(generator::go::DEFAULT_FILE_NAME),
            out.join(generator::gdscript::DEFAULT_FILE_NAME),
        ),
    }
}

fn file_name_of(path: Option<&Path>, default: &str) -> String {
    path.and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| default.to_owned())
}

/// Reads one source file into its models, and the raw text (Go alone needs the
/// text again afterward, to find its `package` clause).
fn read(source: &Path) -> Result<(Vec<Model>, String), String> {
    let text =
        fs::read_to_string(source).map_err(|error| format!("{}: {error}", source.display()))?;
    let models = parser::parse(source, &text).map_err(|error| error.to_string())?;
    Ok((models, text))
}

/// Collects `.rs`, `.cs` and `.go` files, skipping `target` and the
/// generator's own output.
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
        || (name.ends_with(".go") && !name.ends_with(".codec.go"))
        || (name.ends_with(".gd") && !name.ends_with(".codec.gd"))
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
