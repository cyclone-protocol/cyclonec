//! The pipeline: source in, tree out.
//!
//! ```text
//! discover  →  parse  →  IR  →  render  →  compare  →  write
//! ```
//!
//! `compare` sits between rendering and writing on purpose. The schema that is
//! compared is the one this run just derived from source, and the schema it is
//! compared *against* is the artifact of the previous run - never the other way
//! round, and never an input to what gets generated.
//!
//! And the comparison cannot fail the command. Breaking a schema on purpose is
//! a normal thing to do on a branch; being told about it is useful, being
//! stopped is not. CI is where that becomes an error, because CI is where the
//! other end of the wire is somebody else's running code.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::buildgraph::{self, Artifact, Shared};
use crate::cli::Paths;
use crate::config::Config;
use crate::generator;
use crate::gomod;
use crate::ir::Schema;
use crate::json::Json;
use crate::model::Model;
use crate::schema;

/// The language a source file is read as - and so the backend that generates
/// from it. Read off the file's own extension, the same way `parser::parse`
/// picks its scanner; never mixed within one run (see [`plan`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Rust,
    Go,
    CSharp,
    GDScript,
    Cpp,
    C,
}

impl Language {
    fn of(path: &Path) -> Language {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("go") => Language::Go,
            Some("cs") => Language::CSharp,
            Some("gd") => Language::GDScript,
            // `.h` is plain C's, not C++'s: the two share no other extension,
            // and a C project's models live in headers as often as not, so
            // giving `.h` to C++ (as an earlier revision of this generator
            // did) would have left plain-C header-only models undiscoverable.
            // A C++ project's headers use `.hpp` instead - one extension,
            // one language, the same rule `Language::of` already applies to
            // everything else.
            Some("c") | Some("h") => Language::C,
            Some("hpp") | Some("cpp") | Some("cc") | Some("cxx") => Language::Cpp,
            _ => Language::Rust,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Language::Rust => "Rust",
            Language::Go => "Go",
            Language::CSharp => "C#",
            Language::GDScript => "GDScript",
            Language::Cpp => "C++",
            Language::C => "C",
        }
    }
}

/// Everything a run needs to know about where things are.
#[derive(Debug, Clone)]
pub struct Options {
    /// Directories or files to scan.
    pub src: Vec<PathBuf>,
    /// Where generated source goes.
    pub out: PathBuf,
    /// The directory `.cyclone/` lives in.
    pub root: PathBuf,
    /// Overrides how a generated codec reaches your models, in place of the
    /// default. Rust: a module path (`crate::models` makes every model
    /// `crate::models::<Model>`). Go: an import path (the package a model's
    /// source declares is still read from that source, not guessed from this
    /// path). C#: a namespace (every model is treated as declared in it,
    /// overriding whatever `namespace` its own source actually declares).
    /// C++: a namespace too, the same way - the `#include` path a generated
    /// header needs is always the model's own source path and is never
    /// affected by this option, since that is a physical fact about the
    /// build, not a logical one this option is for. GDScript and C: nothing -
    /// GDScript because a model's own `class_name` is already globally
    /// reachable with nothing to override, C because it has no namespace at
    /// all to begin with, only the same physical `#include` C++ has (and,
    /// like C++'s, never affected by this option). This option has no effect
    /// on either backend.
    pub model_path: Option<String>,
    /// Whether generated frames carry `[MessageId][MessageFingerprint]`.
    pub validate_message_fingerprint: bool,
}

impl Options {
    /// Merges the command line over `cyclone.toml` over the defaults.
    ///
    /// # Errors
    ///
    /// A `cyclone.toml` that cannot be read or understood.
    pub fn resolve(paths: &Paths) -> Result<Options, String> {
        let root = PathBuf::from(".");
        let config = Config::load(&root)?;

        let src = if paths.src.is_empty() {
            if config.src.is_empty() {
                vec![PathBuf::from("src")]
            } else {
                config.src.clone()
            }
        } else {
            paths.src.clone()
        };

        let out = paths
            .out
            .clone()
            .or(config.out)
            .unwrap_or_else(|| PathBuf::from("generated"));

        Ok(Options {
            src,
            out,
            root,
            model_path: paths.model_path.clone().or(config.model_path),
            validate_message_fingerprint: config.validate_message_fingerprint.unwrap_or(false),
        })
    }

    /// The path of `.cyclone/schema.json`.
    pub fn schema_path(&self) -> PathBuf {
        self.root.join(schema::PATH)
    }

    /// The path of `.cyclone/build-graph.json`.
    pub fn build_graph_path(&self) -> PathBuf {
        self.root.join(buildgraph::PATH)
    }
}

/// One file this run would write.
#[derive(Debug, Clone)]
pub struct PlannedFile {
    pub path: PathBuf,
    pub contents: String,
    /// Whether two versions differing only in their `generated-at:` line count
    /// as the same file. True for generated source, false for the JSON
    /// artifacts, which carry no timestamp at all.
    pub timestamped: bool,
}

/// What a run would do, worked out without touching the output tree.
pub struct Plan {
    /// The schema this run derived from source - the source of truth.
    pub schema: Schema,
    /// Every file to write, in the order to write it.
    pub files: Vec<PlannedFile>,
    /// Files a previous run wrote that this one does not: a codec whose model
    /// or codec is gone.
    pub obsolete: Vec<PathBuf>,
}

/// Reads the sources and works out everything that would be written.
///
/// # Errors
///
/// A source that cannot be read, an annotation that cannot be understood, or a
/// schema that does not check out. Nothing is written before every one of those
/// has passed.
pub fn plan(options: &Options) -> Result<Plan, String> {
    let sources = discover(options)?;

    let mut parsed: Vec<(PathBuf, Vec<Model>)> = Vec::new();
    // The `package` clause each Go source declares, keyed by its full path
    // (`model.source`'s own format) - read once, here, while the text is
    // still in hand, rather than re-read later.
    let mut go_packages: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    // The `namespace` each C# source declares, if any - the C# counterpart of
    // `go_packages`. `None` means the source declared no namespace at all
    // (C#'s global namespace), which is a valid answer, not a missing one.
    let mut csharp_namespaces: std::collections::BTreeMap<String, Option<String>> =
        std::collections::BTreeMap::new();
    // The first `namespace` each C++ source opens, if any - the C++
    // counterpart of `csharp_namespaces`, read the same way and for the same
    // reason (see `generator::cpp::ModelLocation::namespace`).
    let mut cpp_namespaces: std::collections::BTreeMap<String, Option<String>> =
        std::collections::BTreeMap::new();
    let mut has_rust = false;
    let mut has_go = false;
    let mut has_csharp = false;
    let mut has_gdscript = false;
    let mut has_cpp = false;
    // Plain C has no `namespace` to read - a global, flat symbol space is
    // exactly what a generated C tree already needs (see `generator::c`), so
    // unlike `cpp_namespaces` there is nothing to collect here.
    let mut has_c = false;
    let mut failures = 0;
    for (root, path) in &sources {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("error: {}: {error}", path.display());
                failures += 1;
                continue;
            }
        };
        match crate::parser::parse(path, &text) {
            Ok(models) => {
                if !models.is_empty() {
                    match Language::of(path) {
                        Language::Go => {
                            has_go = true;
                            if let Some(package) = crate::parser::go::package_name(&text) {
                                go_packages.insert(display(path), package);
                            }
                        }
                        Language::CSharp => {
                            has_csharp = true;
                            csharp_namespaces.insert(
                                display(path),
                                crate::parser::csharp::namespace_name(&text),
                            );
                        }
                        Language::GDScript => has_gdscript = true,
                        Language::Cpp => {
                            has_cpp = true;
                            cpp_namespaces
                                .insert(display(path), crate::parser::cpp::namespace_name(&text));
                        }
                        Language::C => has_c = true,
                        Language::Rust => has_rust = true,
                    }
                    parsed.push((relative(root, path), models));
                }
            }
            Err(error) => {
                eprintln!("error: {error}");
                failures += 1;
            }
        }
    }

    if failures > 0 {
        return Err(format!("{failures} file(s) failed"));
    }
    let found: Vec<&str> = [
        (has_rust, Language::Rust),
        (has_go, Language::Go),
        (has_csharp, Language::CSharp),
        (has_gdscript, Language::GDScript),
        (has_cpp, Language::Cpp),
        (has_c, Language::C),
    ]
    .into_iter()
    .filter(|(present, _)| *present)
    .map(|(_, language)| language.name())
    .collect();
    if found.len() > 1 {
        return Err(format!(
            "found {} models under {} - each backend needs its own `--src` / `--out` (and \
             usually its own cyclone.toml); point this run at one language at a time",
            found.join(", "),
            options
                .src
                .iter()
                .map(|path| display(path))
                .collect::<Vec<String>>()
                .join(", "),
        ));
    }
    let language = if has_go {
        Language::Go
    } else if has_csharp {
        Language::CSharp
    } else if has_gdscript {
        Language::GDScript
    } else if has_cpp {
        Language::Cpp
    } else if has_c {
        Language::C
    } else {
        Language::Rust
    };

    let models: Vec<Model> = parsed
        .iter()
        .flat_map(|(_, models)| models.iter().cloned())
        .collect();
    let schema = Schema::build(&models)?;

    let (mut files, artifacts, shared) = match language {
        Language::Rust => plan_rust(options, &schema, &parsed)?,
        Language::Go => plan_go(options, &schema, &go_packages)?,
        Language::CSharp => plan_csharp(options, &schema, &csharp_namespaces)?,
        Language::GDScript => plan_gdscript(options, &schema)?,
        Language::Cpp => plan_cpp(options, &schema, &cpp_namespaces)?,
        Language::C => plan_c(options, &schema)?,
    };

    files.push(PlannedFile {
        path: options.schema_path(),
        contents: schema::to_json(&schema),
        timestamped: false,
    });
    files.push(PlannedFile {
        path: options.build_graph_path(),
        contents: buildgraph::to_json(&schema, &artifacts, &shared),
        timestamped: false,
    });

    let obsolete = obsolete(options, &files);

    Ok(Plan {
        schema,
        files,
        obsolete,
    })
}

/// A backend's half of [`plan`]: the files it writes, the codec artifacts
/// among them (for the build graph), and which of them are shared rather than
/// tied to one model (the runtime, the handshake, and - Rust only - the
/// module root).
type BackendPlan = (Vec<PlannedFile>, Vec<Artifact>, Vec<Shared>);

/// The Rust half of [`plan`]: the module tree, exactly as before.
fn plan_rust(
    options: &Options,
    schema: &Schema,
    parsed: &[(PathBuf, Vec<Model>)],
) -> Result<BackendPlan, String> {
    // Where each model's type can be reached from inside the generated tree.
    let types = model_paths(options, parsed);
    let imports = generator::rust::Imports {
        types: &types,
        root: "super",
    };

    let mut files = Vec::new();
    let mut artifacts = Vec::new();
    let mut modules = vec!["runtime".to_owned(), "handshake".to_owned()];
    let mut codecs: Vec<(String, String)> = Vec::new();

    files.push(PlannedFile {
        path: options.out.join("runtime.rs"),
        contents: runtime_file(),
        timestamped: true,
    });

    let handshake =
        generator::handshake::handshake_file(schema, options.validate_message_fingerprint)?;
    files.push(PlannedFile {
        path: options.out.join(generator::handshake::FILE_NAME),
        contents: handshake,
        timestamped: true,
    });

    // The tree is flat: one module per model per codec, all siblings of the
    // runtime. Model names are unique in a schema, so `<model>_<codec>` is too,
    // and a flat tree is a tree every file can reach with one `super::`.
    for model in &schema.models {
        for message in &model.messages {
            let module = generator::rust::module_name(&model.name, &message.codec);
            let file = options
                .out
                .join(generator::rust::file_name(&model.name, &message.codec));
            let contents = generator::rust::codec_file(model, message, &imports);

            artifacts.push(Artifact {
                path: display(&file),
                source: model.source.clone(),
                model: model.name.clone(),
                codec: message.codec.clone(),
                fingerprint: message.fingerprint,
                sha256: buildgraph::digest(&contents),
            });
            files.push(PlannedFile {
                path: file,
                contents,
                timestamped: true,
            });
            codecs.push((
                module.clone(),
                generator::rust::codec_type_name(&model.name, &message.codec),
            ));
            modules.push(module);
        }
    }

    check_module_names(&modules)?;
    files.push(PlannedFile {
        path: options.out.join(generator::MODULE_ROOT),
        contents: generator::module_root(&modules, &codecs),
        timestamped: true,
    });

    let shared: Vec<Shared> = files
        .iter()
        .filter(|file| {
            matches!(
                file.path.file_name().and_then(|name| name.to_str()),
                Some("runtime.rs") | Some("handshake.rs") | Some(generator::MODULE_ROOT)
            )
        })
        .map(|file| Shared {
            path: display(&file.path),
            sha256: buildgraph::digest(&file.contents),
            kind: match file.path.file_name().and_then(|name| name.to_str()) {
                Some("runtime.rs") => "runtime",
                Some("handshake.rs") => "handshake",
                _ => "root",
            },
        })
        .collect();

    Ok((files, artifacts, shared))
}

/// The runtime file: the header, then the RFC-0002 block verbatim.
fn runtime_file() -> String {
    let mut out = generator::Header {
        note: Some(
            "The Cyclone runtime - Writer, Reader, DecodeError, Limits - carried\n\
             verbatim from RFC-0002. Identical in every project cyclonec generates\n\
             for: nothing in it is derived from your models.",
        ),
        ..generator::Header::default()
    }
    .render();
    out.push_str(generator::FILE_ATTRIBUTES);
    out.push_str(generator::rust_runtime::RUNTIME);
    out
}

// ==================================================================== Go plan

/// The Go half of [`plan`].
///
/// Go compiles by package, not by file, so - unlike [`plan_rust`] - there is
/// no module root to write and codecs never import one another: every
/// generated file here shares one `package` clause (derived from `--out`'s
/// own directory name), and only the *model* types they name ever need an
/// `import`.
///
/// # Errors
///
/// No `go.mod` at the project root (the Go backend needs one to compute
/// import paths - see [`gomod`]), a model with `Array<Array<T>>` (a known gap,
/// see [`generator::go`]), or two codecs that would collide on one file name.
fn plan_go(
    options: &Options,
    schema: &Schema,
    go_packages: &std::collections::BTreeMap<String, String>,
) -> Result<BackendPlan, String> {
    for model in &schema.models {
        generator::go::check_no_nested_arrays(model)?;
    }

    let (module_root, module) = gomod::find(&options.root)?.ok_or_else(|| {
        format!(
            "no go.mod found at {} - the Go backend needs one there to compute import paths",
            display(&options.root)
        )
    })?;
    if module_root != options.root {
        return Err(format!(
            "go.mod is at {}, not {} - the Go backend only looks for one in the project root \
             (where cyclone.toml lives); run cyclonec from there",
            display(&module_root),
            display(&options.root),
        ));
    }

    let package = generator::go::package_name_from_out(&options.out);
    let own_import_path = gomod::import_path_of_dir(&module, &options.out);

    // Where each model's Go type can be reached from, and under what package
    // name it was declared - read from source rather than derived from the
    // path, because the two need not match.
    let mut locations = std::collections::BTreeMap::new();
    for model in &schema.models {
        let import_path = match &options.model_path {
            Some(prefix) => prefix.clone(),
            None => gomod::import_path(&module, Path::new(&model.source)),
        };
        let package_name = go_packages.get(&model.source).cloned().unwrap_or_else(|| {
            import_path
                .rsplit('/')
                .next()
                .unwrap_or(&import_path)
                .to_owned()
        });
        locations.insert(
            model.name.clone(),
            generator::go::ModelLocation {
                import_path,
                package: package_name,
            },
        );
    }
    let imports = generator::go::Imports {
        locations: &locations,
        own_import_path: &own_import_path,
    };

    let mut seen_files: BTreeSet<String> = BTreeSet::new();
    for model in &schema.models {
        for message in &model.messages {
            let name = generator::go::file_name(&model.name, &message.codec);
            if !seen_files.insert(name.clone()) {
                return Err(format!(
                    "two codecs would both be generated as `{name}` - rename one of the models \
                     or codecs involved"
                ));
            }
        }
    }

    let mut files = Vec::new();
    let mut artifacts = Vec::new();

    files.push(PlannedFile {
        path: options.out.join("runtime.go"),
        contents: go_runtime_file(&package),
        timestamped: true,
    });

    let handshake = generator::go_handshake::handshake_file(
        schema,
        &package,
        options.validate_message_fingerprint,
    )?;
    files.push(PlannedFile {
        path: options.out.join(generator::go_handshake::FILE_NAME),
        contents: handshake,
        timestamped: true,
    });

    for model in &schema.models {
        for message in &model.messages {
            let file = options
                .out
                .join(generator::go::file_name(&model.name, &message.codec));
            let contents = generator::go::codec_file(model, message, &package, &imports);

            artifacts.push(Artifact {
                path: display(&file),
                source: model.source.clone(),
                model: model.name.clone(),
                codec: message.codec.clone(),
                fingerprint: message.fingerprint,
                sha256: buildgraph::digest(&contents),
            });
            files.push(PlannedFile {
                path: file,
                contents,
                timestamped: true,
            });
        }
    }

    let shared: Vec<Shared> = files
        .iter()
        .filter(|file| {
            matches!(
                file.path.file_name().and_then(|name| name.to_str()),
                Some("runtime.go") | Some(generator::go_handshake::FILE_NAME)
            )
        })
        .map(|file| Shared {
            path: display(&file.path),
            sha256: buildgraph::digest(&file.contents),
            kind: match file.path.file_name().and_then(|name| name.to_str()) {
                Some("runtime.go") => "runtime",
                _ => "handshake",
            },
        })
        .collect();

    Ok((files, artifacts, shared))
}

/// The Go runtime file: the header, the package clause, then the RFC-0002
/// block verbatim.
fn go_runtime_file(package: &str) -> String {
    let mut out = generator::Header {
        note: Some(
            "The Cyclone runtime - Writer, Reader, DecodeError, Limits - carried\n\
             verbatim from RFC-0002. Identical in every project cyclonec generates\n\
             for: nothing in it is derived from your models.",
        ),
        ..generator::Header::default()
    }
    .render();
    out.push_str(&format!("package {package}\n"));
    out.push_str(generator::go_runtime::RUNTIME);
    out
}

// =================================================================== C# plan

/// The C# half of [`plan`].
///
/// C# compiles by project, not by directory or by package, so - unlike
/// [`plan_go`] - there is no external project file this backend needs to find
/// first: a namespace is self-declared by whatever source wrote it, and a
/// cross-namespace reference is always spelled out in full rather than
/// `import`ed, so nothing here has to compute an import path at all. Every
/// generated file in one run still shares one namespace, derived from
/// `--out`'s own directory name - the C# counterpart of the package name
/// [`plan_go`] derives the same way.
///
/// # Errors
///
/// A model with `Array<Array<T>>` (a deliberate gap, see
/// [`generator::csharp`]), or two codecs that would collide on one file name.
fn plan_csharp(
    options: &Options,
    schema: &Schema,
    csharp_namespaces: &std::collections::BTreeMap<String, Option<String>>,
) -> Result<BackendPlan, String> {
    for model in &schema.models {
        generator::csharp::check_no_nested_arrays(model)?;
    }

    let namespace = generator::csharp::namespace_from_out(&options.out);

    // Where each model's C# namespace is, so a generated codec can qualify a
    // reference to it - read from source rather than derived from the path,
    // because the two need not match. `--model-path` overrides every model at
    // once, the same as it does for Go's import path.
    let mut locations: std::collections::BTreeMap<String, Option<String>> =
        std::collections::BTreeMap::new();
    for model in &schema.models {
        let location = match &options.model_path {
            Some(prefix) => Some(prefix.clone()),
            None => csharp_namespaces
                .get(&model.source)
                .cloned()
                .unwrap_or(None),
        };
        locations.insert(model.name.clone(), location);
    }
    let imports = generator::csharp::Imports {
        locations: &locations,
        own_namespace: &namespace,
    };

    let mut seen_files: BTreeSet<String> = BTreeSet::new();
    for model in &schema.models {
        for message in &model.messages {
            let name = generator::csharp::file_name(&model.name, &message.codec);
            if !seen_files.insert(name.clone()) {
                return Err(format!(
                    "two codecs would both be generated as `{name}` - rename one of the models \
                     or codecs involved"
                ));
            }
        }
    }

    let mut files = Vec::new();
    let mut artifacts = Vec::new();

    files.push(PlannedFile {
        path: options.out.join("runtime.cs"),
        contents: csharp_runtime_file(&namespace),
        timestamped: true,
    });

    let handshake = generator::csharp_handshake::handshake_file(
        schema,
        &namespace,
        options.validate_message_fingerprint,
    )?;
    files.push(PlannedFile {
        path: options.out.join(generator::csharp_handshake::FILE_NAME),
        contents: handshake,
        timestamped: true,
    });

    for model in &schema.models {
        for message in &model.messages {
            let file = options
                .out
                .join(generator::csharp::file_name(&model.name, &message.codec));
            let contents = generator::csharp::codec_file(model, message, &namespace, &imports);

            artifacts.push(Artifact {
                path: display(&file),
                source: model.source.clone(),
                model: model.name.clone(),
                codec: message.codec.clone(),
                fingerprint: message.fingerprint,
                sha256: buildgraph::digest(&contents),
            });
            files.push(PlannedFile {
                path: file,
                contents,
                timestamped: true,
            });
        }
    }

    let shared: Vec<Shared> = files
        .iter()
        .filter(|file| {
            matches!(
                file.path.file_name().and_then(|name| name.to_str()),
                Some("runtime.cs") | Some(generator::csharp_handshake::FILE_NAME)
            )
        })
        .map(|file| Shared {
            path: display(&file.path),
            sha256: buildgraph::digest(&file.contents),
            kind: match file.path.file_name().and_then(|name| name.to_str()) {
                Some("runtime.cs") => "runtime",
                _ => "handshake",
            },
        })
        .collect();

    Ok((files, artifacts, shared))
}

/// The C# runtime file: the header, then the namespace and the RFC-0002
/// block verbatim inside it.
fn csharp_runtime_file(namespace: &str) -> String {
    let mut out = generator::Header {
        note: Some(
            "The Cyclone runtime - Writer, Reader, DecodeException, Limits - carried\n\
             verbatim from RFC-0002. Identical in every project cyclonec generates\n\
             for: nothing in it is derived from your models.",
        ),
        ..generator::Header::default()
    }
    .render();
    out.push_str(&format!("namespace {namespace}\n{{\n"));
    out.push_str(generator::csharp_runtime::RUNTIME);
    out.push_str("}\n");
    out
}

// ============================================================== GDScript plan

/// The GDScript half of [`plan`].
///
/// GDScript needs no external project file, no package, and no namespace:
/// every model, and every generated codec, already declares its own
/// `class_name`, and Godot makes that name reachable project-wide with
/// nothing to `preload` or `import`. So unlike [`plan_go`] and
/// [`plan_csharp`], there is no `Imports` type here and no per-source
/// location to track - the discovery loop above never bothers building one
/// for GDScript, the way it does `go_packages` and `csharp_namespaces`.
///
/// # Errors
///
/// A model with `Array<Array<T>>` (a deliberate gap, see
/// [`generator::gdscript`]), two constants that would collide (see
/// [`generator::gdscript_handshake`]), or two codecs that would collide on
/// one file name.
fn plan_gdscript(options: &Options, schema: &Schema) -> Result<BackendPlan, String> {
    for model in &schema.models {
        generator::gdscript::check_no_nested_arrays(model)?;
    }

    let mut seen_files: BTreeSet<String> = BTreeSet::new();
    for model in &schema.models {
        for message in &model.messages {
            let name = generator::gdscript::file_name(&model.name, &message.codec);
            if !seen_files.insert(name.clone()) {
                return Err(format!(
                    "two codecs would both be generated as `{name}` - rename one of the models \
                     or codecs involved"
                ));
            }
        }
    }

    let mut files = Vec::new();
    let mut artifacts = Vec::new();

    files.push(PlannedFile {
        path: options.out.join("runtime.gd"),
        contents: gdscript_runtime_file(),
        timestamped: true,
    });

    let handshake = generator::gdscript_handshake::handshake_file(
        schema,
        options.validate_message_fingerprint,
    )?;
    files.push(PlannedFile {
        path: options.out.join(generator::gdscript_handshake::FILE_NAME),
        contents: handshake,
        timestamped: true,
    });

    for model in &schema.models {
        for message in &model.messages {
            let file = options
                .out
                .join(generator::gdscript::file_name(&model.name, &message.codec));
            let contents = generator::gdscript::codec_file(model, message);

            artifacts.push(Artifact {
                path: display(&file),
                source: model.source.clone(),
                model: model.name.clone(),
                codec: message.codec.clone(),
                fingerprint: message.fingerprint,
                sha256: buildgraph::digest(&contents),
            });
            files.push(PlannedFile {
                path: file,
                contents,
                timestamped: true,
            });
        }
    }

    let shared: Vec<Shared> = files
        .iter()
        .filter(|file| {
            matches!(
                file.path.file_name().and_then(|name| name.to_str()),
                Some("runtime.gd") | Some(generator::gdscript_handshake::FILE_NAME)
            )
        })
        .map(|file| Shared {
            path: display(&file.path),
            sha256: buildgraph::digest(&file.contents),
            kind: match file.path.file_name().and_then(|name| name.to_str()) {
                Some("runtime.gd") => "runtime",
                _ => "handshake",
            },
        })
        .collect();

    Ok((files, artifacts, shared))
}

/// The GDScript runtime file: the header, the file's own `class_name
/// CycloneRuntime`, then the RFC-0002 block verbatim.
fn gdscript_runtime_file() -> String {
    let mut out = generator::gdscript::Header {
        note: Some(
            "The Cyclone runtime - Writer, Reader, DecodeError, Limits - carried\n\
             verbatim from RFC-0002. Identical in every project cyclonec generates\n\
             for: nothing in it is derived from your models.",
        ),
        ..generator::gdscript::Header::default()
    }
    .render();
    out.push_str("class_name CycloneRuntime\n");
    out.push_str(generator::gdscript_runtime::RUNTIME);
    out
}

// ==================================================================== C++ plan

/// The C++ half of [`plan`].
///
/// C++ needs no external project file, no package and no namespace of its
/// own the way Go needs `go.mod` - a namespace is self-declared by whatever
/// source wrote it, the same as C#'s, and this generator derives one more
/// for the generated tree itself from `--out`'s own directory name, the C++
/// counterpart of [`plan_go`]'s package name and [`plan_csharp`]'s namespace.
/// What C++ needs that neither of those does is a physical `#include` for
/// every model a generated header touches - see
/// [`generator::cpp::ModelLocation`].
///
/// # Errors
///
/// A model with `Array<Array<T>>` (a deliberate gap, see [`generator::cpp`]),
/// two constants that would collide (see [`generator::cpp_handshake`]), or
/// two codecs that would collide on one file name.
fn plan_cpp(
    options: &Options,
    schema: &Schema,
    cpp_namespaces: &std::collections::BTreeMap<String, Option<String>>,
) -> Result<BackendPlan, String> {
    for model in &schema.models {
        generator::cpp::check_no_nested_arrays(model)?;
    }

    let namespace = generator::cpp::namespace_from_out(&options.out);

    // Where each model's C++ type is declared: the `#include` is always the
    // model's own source path (never overridden - see
    // `Options::model_path`'s doc comment), and the namespace is read from
    // source unless `--model-path` overrides it uniformly, the same as C#.
    let mut locations: std::collections::BTreeMap<String, generator::cpp::ModelLocation> =
        std::collections::BTreeMap::new();
    for model in &schema.models {
        let namespace_override = match &options.model_path {
            Some(prefix) => Some(prefix.clone()),
            None => cpp_namespaces.get(&model.source).cloned().unwrap_or(None),
        };
        locations.insert(
            model.name.clone(),
            generator::cpp::ModelLocation {
                include: model.source.clone(),
                namespace: namespace_override,
            },
        );
    }
    let imports = generator::cpp::Imports {
        locations: &locations,
    };

    let mut seen_files: BTreeSet<String> = BTreeSet::new();
    for model in &schema.models {
        for message in &model.messages {
            let name = generator::cpp::file_name(&model.name, &message.codec);
            if !seen_files.insert(name.clone()) {
                return Err(format!(
                    "two codecs would both be generated as `{name}` - rename one of the models \
                     or codecs involved"
                ));
            }
        }
    }

    let mut files = Vec::new();
    let mut artifacts = Vec::new();

    files.push(PlannedFile {
        path: options.out.join("runtime.hpp"),
        contents: cpp_runtime_file(&namespace),
        timestamped: true,
    });

    let handshake = generator::cpp_handshake::handshake_file(
        schema,
        &namespace,
        options.validate_message_fingerprint,
    )?;
    files.push(PlannedFile {
        path: options.out.join(generator::cpp_handshake::FILE_NAME),
        contents: handshake,
        timestamped: true,
    });

    for model in &schema.models {
        for message in &model.messages {
            let file = options
                .out
                .join(generator::cpp::file_name(&model.name, &message.codec));
            let contents = generator::cpp::codec_file(model, message, &namespace, &imports);

            artifacts.push(Artifact {
                path: display(&file),
                source: model.source.clone(),
                model: model.name.clone(),
                codec: message.codec.clone(),
                fingerprint: message.fingerprint,
                sha256: buildgraph::digest(&contents),
            });
            files.push(PlannedFile {
                path: file,
                contents,
                timestamped: true,
            });
        }
    }

    let shared: Vec<Shared> = files
        .iter()
        .filter(|file| {
            matches!(
                file.path.file_name().and_then(|name| name.to_str()),
                Some("runtime.hpp") | Some(generator::cpp_handshake::FILE_NAME)
            )
        })
        .map(|file| Shared {
            path: display(&file.path),
            sha256: buildgraph::digest(&file.contents),
            kind: match file.path.file_name().and_then(|name| name.to_str()) {
                Some("runtime.hpp") => "runtime",
                _ => "handshake",
            },
        })
        .collect();

    Ok((files, artifacts, shared))
}

/// The C++ runtime file: the header, `#pragma once`, the standard includes it
/// needs, then the namespace and the RFC-0002 block verbatim inside it.
fn cpp_runtime_file(namespace: &str) -> String {
    let mut out = generator::Header {
        note: Some(
            "The Cyclone runtime - Writer, Reader, DecodeError, Limits - carried\n\
             verbatim from RFC-0002. Identical in every project cyclonec generates\n\
             for: nothing in it is derived from your models.",
        ),
        ..generator::Header::default()
    }
    .render();
    out.push_str("#pragma once\n\n");
    out.push_str(
        "#include <cstddef>\n#include <cstdint>\n#include <cstdio>\n#include <cstring>\n\
         #include <string>\n#include <vector>\n\n",
    );
    out.push_str(&format!("namespace {namespace} {{\n"));
    out.push_str(generator::cpp_runtime::RUNTIME);
    out.push_str(&format!("\n}}  // namespace {namespace}\n"));
    out
}

// ====================================================================== C plan

/// The C half of [`plan`].
///
/// Plain C needs no external project file and - unlike [`plan_cpp`] - no
/// namespace of its own to derive from `--out`, because C has no namespace at
/// all: every generated identifier is already global, prefixed by model and
/// codec name the same way [`generator::c`]'s whole naming scheme is built
/// around. What it needs that [`plan_cpp`] does not is `arrays.h` - one owned
/// array type per distinct `Array<T>` element type the schema uses, written
/// once and shared, since C has no `std::vector<T>` of its own - and one more
/// file per model, `<model>_cyclone.h`, carrying the `<Model>_free` that
/// releases whatever any of that model's codecs allocated (see
/// [`generator::c::free_file`]).
///
/// # Errors
///
/// A model with `Array<Array<T>>` (a deliberate gap, see [`generator::c`]),
/// two constants that would collide (see [`generator::c_handshake`]), or two
/// generated files that would collide on one name.
fn plan_c(options: &Options, schema: &Schema) -> Result<BackendPlan, String> {
    for model in &schema.models {
        generator::c::check_no_nested_arrays(model)?;
    }

    // Where each model's C `struct` is declared: always the model's own
    // source path, exactly as C++'s is - `--model-path` has no effect on this
    // backend at all, since C has no namespace for it to override (see
    // `Options::model_path`'s doc comment).
    let mut locations: std::collections::BTreeMap<String, generator::c::ModelLocation> =
        std::collections::BTreeMap::new();
    for model in &schema.models {
        locations.insert(
            model.name.clone(),
            generator::c::ModelLocation {
                include: model.source.clone(),
            },
        );
    }
    let imports = generator::c::Imports {
        locations: &locations,
    };

    let mut seen_files: BTreeSet<String> = BTreeSet::new();
    for model in &schema.models {
        let name = generator::c::free_file_name(&model.name);
        if !seen_files.insert(name.clone()) {
            return Err(format!(
                "two models would both be generated as `{name}` - rename one of them"
            ));
        }
        for message in &model.messages {
            let name = generator::c::file_name(&model.name, &message.codec);
            if !seen_files.insert(name.clone()) {
                return Err(format!(
                    "two codecs would both be generated as `{name}` - rename one of the models \
                     or codecs involved"
                ));
            }
        }
    }

    let mut files = Vec::new();
    let mut artifacts = Vec::new();

    files.push(PlannedFile {
        path: options.out.join("runtime.h"),
        contents: c_runtime_file(),
        timestamped: true,
    });

    files.push(PlannedFile {
        path: options.out.join(generator::c::ARRAYS_FILE_NAME),
        contents: generator::c::arrays_file(schema, &imports),
        timestamped: true,
    });

    let handshake =
        generator::c_handshake::handshake_file(schema, options.validate_message_fingerprint)?;
    files.push(PlannedFile {
        path: options.out.join(generator::c_handshake::FILE_NAME),
        contents: handshake,
        timestamped: true,
    });

    for model in &schema.models {
        files.push(PlannedFile {
            path: options.out.join(generator::c::free_file_name(&model.name)),
            contents: generator::c::free_file(model, &imports),
            timestamped: true,
        });

        for message in &model.messages {
            let file = options
                .out
                .join(generator::c::file_name(&model.name, &message.codec));
            let contents = generator::c::codec_file(model, message, &imports);

            artifacts.push(Artifact {
                path: display(&file),
                source: model.source.clone(),
                model: model.name.clone(),
                codec: message.codec.clone(),
                fingerprint: message.fingerprint,
                sha256: buildgraph::digest(&contents),
            });
            files.push(PlannedFile {
                path: file,
                contents,
                timestamped: true,
            });
        }
    }

    let shared: Vec<Shared> = files
        .iter()
        .filter(|file| {
            matches!(
                file.path.file_name().and_then(|name| name.to_str()),
                Some("runtime.h")
                    | Some(generator::c::ARRAYS_FILE_NAME)
                    | Some(generator::c_handshake::FILE_NAME)
            )
        })
        .map(|file| Shared {
            path: display(&file.path),
            sha256: buildgraph::digest(&file.contents),
            kind: match file.path.file_name().and_then(|name| name.to_str()) {
                Some("runtime.h") => "runtime",
                Some(generator::c::ARRAYS_FILE_NAME) => "arrays",
                _ => "handshake",
            },
        })
        .collect();

    Ok((files, artifacts, shared))
}

/// The C runtime file: the header, `#pragma once`, the standard includes it
/// needs, then the RFC-0002 block verbatim. No namespace to open: unlike
/// [`cpp_runtime_file`], every name in it is already global, exactly the way
/// [`generator::c`]'s own generated names are.
fn c_runtime_file() -> String {
    let mut out = generator::Header {
        note: Some(
            "The Cyclone runtime - CycloneWriter, CycloneReader, CycloneDecodeError,\n\
             CycloneLimits, CycloneBytes - carried verbatim from RFC-0002. Identical in\n\
             every project cyclonec generates for: nothing in it is derived from your\n\
             models.",
        ),
        ..generator::Header::default()
    }
    .render();
    out.push_str("#pragma once\n\n");
    out.push_str(
        "#include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n#include <stdio.h>\n\
         #include <stdlib.h>\n#include <string.h>\n",
    );
    out.push_str(generator::c_runtime::RUNTIME);
    out
}

/// Writes (or, with `check`, only inspects) everything the plan holds.
///
/// Returns whether the tree on disk is - or now is - what the plan says.
///
/// # Errors
///
/// A file that cannot be written.
pub fn apply(plan: &Plan, check: bool, quiet: bool) -> Result<bool, String> {
    let mut current = true;

    for file in &plan.files {
        let existing = std::fs::read_to_string(&file.path).ok();
        let unchanged = existing.as_deref().is_some_and(|existing| {
            if file.timestamped {
                generator::same_but_for_timestamp(existing, &file.contents)
            } else {
                existing == file.contents
            }
        });

        if unchanged {
            continue;
        }

        if check {
            let problem = if existing.is_none() {
                "missing"
            } else {
                "does not match its sources"
            };
            eprintln!("stale: {} {problem}", display(&file.path));
            current = false;
            continue;
        }

        if let Some(directory) = file.path.parent() {
            if !directory.as_os_str().is_empty() {
                std::fs::create_dir_all(directory)
                    .map_err(|error| format!("cannot create {}: {error}", display(directory)))?;
            }
        }
        std::fs::write(&file.path, &file.contents)
            .map_err(|error| format!("cannot write {}: {error}", display(&file.path)))?;
        if !quiet {
            eprintln!("cyclonec: {}", display(&file.path));
        }
    }

    for path in &plan.obsolete {
        if check {
            eprintln!(
                "stale: {} is generated from a model that no longer exists",
                display(path)
            );
            current = false;
            continue;
        }
        std::fs::remove_file(path)
            .map_err(|error| format!("cannot remove {}: {error}", display(path)))?;
        if !quiet {
            eprintln!("cyclonec: removed {}", display(path));
        }
    }

    Ok(current)
}

/// Files the previous build graph claims this generator wrote, which this run
/// does not write again.
///
/// A codec whose model was deleted leaves a file behind that nothing includes
/// and nothing compiles - inert, but confusing, and the sort of thing that gets
/// hand-edited months later by somebody who does not know it is dead. Only
/// files that still carry the generated marker are removed; anything a human
/// has replaced is left alone.
fn obsolete(options: &Options, files: &[PlannedFile]) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(options.build_graph_path()) else {
        return Vec::new();
    };
    let Ok(document) = crate::json::parse(&text) else {
        return Vec::new();
    };

    let planned: BTreeSet<String> = files.iter().map(|file| display(&file.path)).collect();
    let mut obsolete = Vec::new();

    let previous = document
        .get("sources")
        .and_then(Json::as_object)
        .unwrap_or_default();
    let outputs = previous
        .iter()
        .filter_map(|(_, entry)| entry.get("outputs").and_then(Json::as_array))
        .flatten();
    // The shared files too: a run that changes the tree's shape - as the move
    // from one `include!`d file to a module tree did - leaves the old shape's
    // root behind, and a stray `cyclone.rs` full of `include!` is exactly the
    // kind of thing somebody finds and tries to use.
    let shared = document
        .get("shared")
        .and_then(Json::as_array)
        .unwrap_or(&[]);

    for output in outputs.chain(shared.iter()) {
        let Some(path) = output.get("path").and_then(Json::as_str) else {
            continue;
        };
        if planned.contains(path) {
            continue;
        }
        let path = PathBuf::from(path);
        // Only ever inside the directory this run is writing. A run with a
        // different `--out` has moved the tree, not orphaned it, and deleting
        // the old location - possibly the project's real one, from a one-off
        // override - is not this command's call to make.
        if !path.starts_with(&options.out) {
            continue;
        }
        if std::fs::read_to_string(&path).is_ok_and(|text| starts_with_a_marker(&text)) {
            obsolete.push(path);
        }
    }

    obsolete.sort();
    obsolete
}

// =============================================================== model paths

/// Where each model's type can be reached from inside the generated tree.
///
/// A generated codec is a module now, so it has to `use` the model it encodes,
/// and only the project knows where that model lives. The default reads it off
/// the source layout, which is the same thing Rust itself does:
///
/// ```text
/// src/models/player.rs   →  crate::models::player::Player
/// src/lib.rs             →  crate::Player
/// src/models/mod.rs      →  crate::models::Player
/// ```
///
/// A project whose modules do not mirror its directories - or one that
/// re-exports every model from one place - overrides it wholesale with
/// `model_path` in `cyclone.toml`, or `--model-path`.
pub fn model_paths(
    options: &Options,
    parsed: &[(PathBuf, Vec<Model>)],
) -> std::collections::BTreeMap<String, String> {
    let mut paths = std::collections::BTreeMap::new();

    for (relative_source, models) in parsed {
        let module = match &options.model_path {
            Some(prefix) => prefix.clone(),
            None => module_path_of(relative_source),
        };
        for model in models {
            let path = if module.is_empty() {
                model.name.clone()
            } else {
                format!("{module}::{}", model.name)
            };
            paths.insert(model.name.clone(), path);
        }
    }

    paths
}

/// `models/player.rs` → `crate::models::player`.
fn module_path_of(relative_source: &Path) -> String {
    let mut parts = vec!["crate".to_owned()];

    for component in relative_source.components() {
        let text = component.as_os_str().to_string_lossy().into_owned();
        let text = text.strip_suffix(".rs").unwrap_or(&text).to_owned();
        // `mod.rs`, `lib.rs` and `main.rs` are their directory, not a module
        // under it.
        if matches!(text.as_str(), "mod" | "lib" | "main") {
            continue;
        }
        parts.push(text);
    }

    parts.join("::")
}

/// Two codecs may not want the same module.
///
/// `DeviceState` + `unity` and `DeviceStateUnity` + no codec would both like to
/// be `device_state_unity`. Vanishingly rare, entirely mechanical, and far
/// better said here than as a duplicate-definition error in the user's build.
fn check_module_names(modules: &[String]) -> Result<(), String> {
    for (index, module) in modules.iter().enumerate() {
        if modules[..index].contains(module) {
            return Err(format!(
                "two codecs would both be generated as `{module}.rs` - rename one of the models \
                 or codecs involved"
            ));
        }
    }
    Ok(())
}

// ================================================================== discovery

/// Every source file to read, as `(root, path)` - the root being the `--src`
/// entry it was found under, which is what output paths are made relative to.
fn discover(options: &Options) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut sources = Vec::new();

    for entry in &options.src {
        let metadata =
            std::fs::metadata(entry).map_err(|error| format!("{}: {error}", display(entry)))?;

        if metadata.is_file() {
            // A file named explicitly is read even if discovery would have
            // skipped it; the caller said so.
            let root = entry.parent().unwrap_or(Path::new(".")).to_path_buf();
            sources.push((root, entry.clone()));
            continue;
        }

        walk(entry, entry, &options.out, &mut sources)?;
    }

    sources.sort();
    sources.dedup();
    Ok(sources)
}

fn walk(
    root: &Path,
    directory: &Path,
    out: &Path,
    sources: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), String> {
    // `target/` is a build directory, and the output tree is this generator's
    // own writing - reading either back in would be a loop at best.
    if directory.file_name().is_some_and(|name| name == "target") || same_path(directory, out) {
        return Ok(());
    }

    let entries =
        std::fs::read_dir(directory).map_err(|error| format!("{}: {error}", display(directory)))?;

    for entry in entries {
        let path = entry
            .map_err(|error| format!("{}: {error}", display(directory)))?
            .path();

        if path.is_dir() {
            walk(root, &path, out, sources)?;
            continue;
        }
        let recognised = path.extension().is_some_and(|extension| {
            extension == "rs"
                || extension == "go"
                || extension == "cs"
                || extension == "gd"
                || extension == "hpp"
                || extension == "cpp"
                || extension == "cc"
                || extension == "cxx"
                || extension == "c"
                || extension == "h"
        });
        if recognised && !is_generated(&path) {
            sources.push((root.to_path_buf(), path));
        }
    }

    Ok(())
}

/// Whether a file was written by this generator.
///
/// The marker in the header, not the file name: a project may put generated
/// code anywhere, and `--out` is not the only place it can end up once
/// somebody moves a directory.
fn is_generated(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| starts_with_a_marker(&text))
}

/// Whether `text` opens with this generator's own marker - either spelling:
/// [`generator::MARKER`] (Rust, Go, C#) or [`generator::GDSCRIPT_MARKER`]
/// (GDScript, whose `#` comment syntax the `//`-based marker would not
/// compile as).
fn starts_with_a_marker(text: &str) -> bool {
    text.starts_with(generator::MARKER) || text.starts_with(generator::GDSCRIPT_MARKER)
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

// ======================================================================= paths

/// A path as this crate prints and stores it: `/` separators everywhere, and
/// no leading `./`.
pub fn display(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    text.strip_prefix("./").unwrap_or(&text).to_owned()
}

/// `path` with `root` removed from the front, if it is there.
fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{check_module_names, display, module_path_of};

    /// The default follows Rust's own rule: a directory is a module, a file is
    /// a module, and `mod.rs` / `lib.rs` / `main.rs` are their directory.
    #[test]
    fn a_model_path_is_read_off_the_source_layout() {
        assert_eq!(
            module_path_of(Path::new("models/player.rs")),
            "crate::models::player"
        );
        assert_eq!(module_path_of(Path::new("player.rs")), "crate::player");
        assert_eq!(module_path_of(Path::new("lib.rs")), "crate");
        assert_eq!(module_path_of(Path::new("models/mod.rs")), "crate::models");
    }

    #[test]
    fn two_codecs_may_not_want_the_same_module() {
        assert!(check_module_names(&["player_edge".to_owned(), "team_edge".to_owned()]).is_ok());

        let error = check_module_names(&["player_edge".to_owned(), "player_edge".to_owned()])
            .expect_err("collision");
        assert!(error.contains("player_edge.rs"), "{error}");
    }

    #[test]
    fn a_displayed_path_has_no_leading_dot() {
        assert_eq!(display(Path::new("./generated/mod.rs")), "generated/mod.rs");
    }
}
