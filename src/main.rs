//! The official Cyclone **source generator**.
//!
//! ```text
//! parse  →  collect  →  generate
//! ```
//!
//! `cyclonec` is not a compiler. It reads Cyclone attributes out of source files
//! and writes one self-contained file holding the Cyclone runtime and the
//! Encode / Decode calls each model declared, then exits — the way `protoc` does.
//!
//! It builds no schema, no IR, no type graph, no codec graph and no dependency
//! graph; it runs no semantic analysis and makes no second pass. There is no
//! registry, no reflection and no runtime resolution. The runtime it emits is a
//! fixed block written once against RFC-0002 (see [`runtime`]) — nothing about
//! the wire format is worked out here.

mod cli;
mod generator;
mod model;
mod parser;
mod runtime;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cli::Parsed;

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

fn run(arguments: &cli::Args) -> Result<ExitCode, String> {
    let mut sources = Vec::new();
    for path in &arguments.paths {
        collect(path, &mut sources)?;
    }
    sources.sort();
    sources.dedup();

    // Every source feeds one output file, so the output path is resolved before
    // anything is read: a run that cannot write should not parse first.
    let output = arguments.out.as_deref().map(resolve_output);
    let file_name = output
        .as_deref()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| generator::DEFAULT_FILE_NAME.to_owned());

    let mut models = Vec::new();
    let mut names = Vec::new();
    let mut failures = 0;

    for source in &sources {
        match read(source) {
            Ok(parsed) => {
                if !parsed.is_empty() {
                    names.push(
                        source.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                    );
                }
                models.extend(parsed);
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

    let Some(contents) = generator::render(&names, &models, &file_name) else {
        if !arguments.quiet {
            eprintln!("cyclonec: no model declared a codec; nothing to write");
        }
        return Ok(ExitCode::SUCCESS);
    };

    if arguments.stdout {
        let mut stdout = std::io::stdout().lock();
        return write!(stdout, "{contents}")
            .map(|()| ExitCode::SUCCESS)
            .map_err(|error| error.to_string());
    }

    let output = output.expect("--out is required unless --stdout");
    let codecs = codec_names(&models);

    if arguments.check {
        return check(&output, &contents, arguments.quiet);
    }
    write_output(&output, &contents, &codecs, arguments.quiet)
}

/// Turns the `--out` path into the file to write.
///
/// A path ending in `.rs` is that file; anything else is a directory holding
/// [`generator::DEFAULT_FILE_NAME`]. The rule is the extension and not whether
/// the path happens to exist, so a first run and a second run agree.
fn resolve_output(out: &Path) -> PathBuf {
    if out.extension().is_some_and(|extension| extension == "rs") {
        out.to_path_buf()
    } else {
        out.join(generator::DEFAULT_FILE_NAME)
    }
}

/// Reads one source file into its models.
fn read(source: &Path) -> Result<Vec<model::Model>, String> {
    let text =
        fs::read_to_string(source).map_err(|error| format!("{}: {error}", source.display()))?;
    parser::parse(source, &text).map_err(|error| error.to_string())
}

/// Collects `.rs` files, skipping `target` and the generator's own output.
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

/// Whether a discovered file is Rust source the generator did not write.
///
/// Reading its own output back in would be a loop, and the output now carries a
/// runtime that would parse as a pile of ordinary structs.
fn is_source(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.ends_with(".rs") && !name.ends_with(".codec.rs")
}

fn codec_names(models: &[model::Model]) -> Vec<String> {
    models
        .iter()
        .flat_map(|item| {
            item.codecs
                .iter()
                .map(|codec| format!("{}{}Codec", item.name, model::pascal_case(codec)))
        })
        .collect()
}

/// Reports whether the file on disk still matches its sources.
///
/// The CI mode: an attribute change that was not regenerated means the committed
/// file encodes something other than what the source declares.
fn check(output: &Path, contents: &str, quiet: bool) -> Result<ExitCode, String> {
    if fs::read_to_string(output).ok().as_deref() == Some(contents) {
        if !quiet {
            eprintln!("cyclonec: {} is up to date", output.display());
        }
        return Ok(ExitCode::SUCCESS);
    }

    eprintln!("stale: {} does not match its sources", output.display());
    eprintln!("cyclonec: run `cyclonec` to regenerate");
    Ok(ExitCode::FAILURE)
}

/// Writes the file, unless it is already identical.
///
/// Skipping matters: rewriting an unchanged file bumps its mtime and makes every
/// build that watches it rebuild for nothing.
fn write_output(
    output: &Path,
    contents: &str,
    codecs: &[String],
    quiet: bool,
) -> Result<ExitCode, String> {
    if let Some(directory) = output.parent() {
        if !directory.as_os_str().is_empty() {
            fs::create_dir_all(directory)
                .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        }
    }

    if fs::read_to_string(output).ok().as_deref() == Some(contents) {
        if !quiet {
            eprintln!("cyclonec: {} unchanged", output.display());
        }
        return Ok(ExitCode::SUCCESS);
    }

    fs::write(output, contents)
        .map_err(|error| format!("cannot write {}: {error}", output.display()))?;

    if !quiet {
        eprintln!("cyclonec: {} ({})", output.display(), codecs.join(", "));
    }

    Ok(ExitCode::SUCCESS)
}
