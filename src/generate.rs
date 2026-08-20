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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Rust,
    Go,
    CSharp,
    GDScript,
    Cpp,
    C,
    TypeScript,
    JavaScript,
}

impl Language {
    fn of(path: &Path) -> Language {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("go") => Language::Go,
            Some("cs") => Language::CSharp,
            Some("gd") => Language::GDScript,
            Some("c") | Some("h") => Language::C,
            Some("hpp") | Some("cpp") | Some("cc") | Some("cxx") => Language::Cpp,
            Some("ts") => Language::TypeScript,
            Some("js") => Language::JavaScript,
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
            Language::TypeScript => "TypeScript",
            Language::JavaScript => "JavaScript",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    pub src: Vec<PathBuf>,
    pub out: PathBuf,
    pub root: PathBuf,
    pub model_path: Option<String>,
    pub validate_message_fingerprint: bool,
}

impl Options {
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

    pub fn schema_path(&self) -> PathBuf {
        self.root.join(schema::PATH)
    }

    pub fn build_graph_path(&self) -> PathBuf {
        self.root.join(buildgraph::PATH)
    }
}

#[derive(Debug, Clone)]
pub struct PlannedFile {
    pub path: PathBuf,
    pub contents: String,
    pub timestamped: bool,
}

pub struct Plan {
    pub schema: Schema,
    pub files: Vec<PlannedFile>,
    pub obsolete: Vec<PathBuf>,
}

pub fn plan(options: &Options) -> Result<Plan, String> {
    let sources = discover(options)?;

    let mut parsed: Vec<(PathBuf, Vec<Model>)> = Vec::new();
    let mut go_packages: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut csharp_namespaces: std::collections::BTreeMap<String, Option<String>> =
        std::collections::BTreeMap::new();
    let mut cpp_namespaces: std::collections::BTreeMap<String, Option<String>> =
        std::collections::BTreeMap::new();
    let mut has_rust = false;
    let mut has_go = false;
    let mut has_csharp = false;
    let mut has_gdscript = false;
    let mut has_cpp = false;
    let mut has_c = false;
    let mut has_typescript = false;
    let mut has_javascript = false;
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
                        Language::TypeScript => has_typescript = true,
                        Language::JavaScript => has_javascript = true,
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
        (has_typescript, Language::TypeScript),
        (has_javascript, Language::JavaScript),
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
    } else if has_typescript {
        Language::TypeScript
    } else if has_javascript {
        Language::JavaScript
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
        Language::TypeScript => plan_typescript(options, &schema)?,
        Language::JavaScript => plan_javascript(options, &schema)?,
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

type BackendPlan = (Vec<PlannedFile>, Vec<Artifact>, Vec<Shared>);

fn plan_rust(
    options: &Options,
    schema: &Schema,
    parsed: &[(PathBuf, Vec<Model>)],
) -> Result<BackendPlan, String> {
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

fn plan_csharp(
    options: &Options,
    schema: &Schema,
    csharp_namespaces: &std::collections::BTreeMap<String, Option<String>>,
) -> Result<BackendPlan, String> {
    for model in &schema.models {
        generator::csharp::check_no_nested_arrays(model)?;
    }

    let namespace = generator::csharp::namespace_from_out(&options.out);

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
        path: options.out.join(generator::csharp::RUNTIME_FILE_NAME),
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
                Some(generator::csharp::RUNTIME_FILE_NAME)
                    | Some(generator::csharp_handshake::FILE_NAME)
            )
        })
        .map(|file| Shared {
            path: display(&file.path),
            sha256: buildgraph::digest(&file.contents),
            kind: match file.path.file_name().and_then(|name| name.to_str()) {
                Some(generator::csharp::RUNTIME_FILE_NAME) => "runtime",
                _ => "handshake",
            },
        })
        .collect();

    Ok((files, artifacts, shared))
}

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

fn plan_cpp(
    options: &Options,
    schema: &Schema,
    cpp_namespaces: &std::collections::BTreeMap<String, Option<String>>,
) -> Result<BackendPlan, String> {
    for model in &schema.models {
        generator::cpp::check_no_nested_arrays(model)?;
    }

    let namespace = generator::cpp::namespace_from_out(&options.out);

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

fn plan_c(options: &Options, schema: &Schema) -> Result<BackendPlan, String> {
    for model in &schema.models {
        generator::c::check_no_nested_arrays(model)?;
    }

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

fn plan_typescript(options: &Options, schema: &Schema) -> Result<BackendPlan, String> {
    for model in &schema.models {
        generator::typescript::check_no_nested_arrays(model)?;
    }

    let locations: std::collections::BTreeMap<String, generator::typescript::ModelLocation> =
        model_specifiers(options, schema)
            .into_iter()
            .map(|(name, specifier)| (name, generator::typescript::ModelLocation { specifier }))
            .collect();
    let imports = generator::typescript::Imports {
        locations: &locations,
    };

    let mut seen_files: BTreeSet<String> = BTreeSet::new();
    for model in &schema.models {
        for message in &model.messages {
            let name = generator::typescript::file_name(&model.name, &message.codec);
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
        path: options.out.join("runtime.ts"),
        contents: typescript_runtime_file(),
        timestamped: true,
    });

    let handshake = generator::typescript_handshake::handshake_file(
        schema,
        options.validate_message_fingerprint,
    )?;
    files.push(PlannedFile {
        path: options.out.join(generator::typescript_handshake::FILE_NAME),
        contents: handshake,
        timestamped: true,
    });

    for model in &schema.models {
        for message in &model.messages {
            let file = options.out.join(generator::typescript::file_name(
                &model.name,
                &message.codec,
            ));
            let contents = generator::typescript::codec_file(model, message, &imports);

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
                Some("runtime.ts") | Some(generator::typescript_handshake::FILE_NAME)
            )
        })
        .map(|file| Shared {
            path: display(&file.path),
            sha256: buildgraph::digest(&file.contents),
            kind: match file.path.file_name().and_then(|name| name.to_str()) {
                Some("runtime.ts") => "runtime",
                _ => "handshake",
            },
        })
        .collect();

    Ok((files, artifacts, shared))
}

fn typescript_runtime_file() -> String {
    let mut out = generator::Header {
        note: Some(
            "The Cyclone runtime - Writer, Reader, DecodeError, Limits - carried\n\
             verbatim from RFC-0002. Identical in every project cyclonec generates\n\
             for: nothing in it is derived from your models.",
        ),
        ..generator::Header::default()
    }
    .render();
    out.push_str(generator::typescript_runtime::RUNTIME);
    out
}

fn plan_javascript(options: &Options, schema: &Schema) -> Result<BackendPlan, String> {
    for model in &schema.models {
        generator::javascript::check_no_nested_arrays(model)?;
    }

    let locations: std::collections::BTreeMap<String, generator::javascript::ModelLocation> =
        model_specifiers(options, schema)
            .into_iter()
            .map(|(name, specifier)| (name, generator::javascript::ModelLocation { specifier }))
            .collect();
    let imports = generator::javascript::Imports {
        locations: &locations,
    };

    let mut seen_files: BTreeSet<String> = BTreeSet::new();
    for model in &schema.models {
        for message in &model.messages {
            let name = generator::javascript::file_name(&model.name, &message.codec);
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
        path: options.out.join("runtime.js"),
        contents: javascript_runtime_file(),
        timestamped: true,
    });

    let handshake = generator::javascript_handshake::handshake_file(
        schema,
        options.validate_message_fingerprint,
    )?;
    files.push(PlannedFile {
        path: options.out.join(generator::javascript_handshake::FILE_NAME),
        contents: handshake,
        timestamped: true,
    });

    for model in &schema.models {
        for message in &model.messages {
            let file = options.out.join(generator::javascript::file_name(
                &model.name,
                &message.codec,
            ));
            let contents = generator::javascript::codec_file(model, message, &imports);

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
                Some("runtime.js") | Some(generator::javascript_handshake::FILE_NAME)
            )
        })
        .map(|file| Shared {
            path: display(&file.path),
            sha256: buildgraph::digest(&file.contents),
            kind: match file.path.file_name().and_then(|name| name.to_str()) {
                Some("runtime.js") => "runtime",
                _ => "handshake",
            },
        })
        .collect();

    Ok((files, artifacts, shared))
}

fn javascript_runtime_file() -> String {
    let mut out = generator::Header {
        note: Some(
            "The Cyclone runtime - Writer, Reader, DecodeError, Limits - carried\n\
             verbatim from RFC-0002. Identical in every project cyclonec generates\n\
             for: nothing in it is derived from your models.",
        ),
        ..generator::Header::default()
    }
    .render();
    out.push_str(generator::javascript_runtime::RUNTIME);
    out
}

fn model_specifiers(
    options: &Options,
    schema: &Schema,
) -> std::collections::BTreeMap<String, String> {
    schema
        .models
        .iter()
        .map(|model| {
            let specifier = match &options.model_path {
                Some(prefix) => prefix.clone(),
                None => relative_module_specifier(&options.out, &model.source),
            };
            (model.name.clone(), specifier)
        })
        .collect()
}

fn relative_module_specifier(out: &Path, source: &str) -> String {
    let source_path = Path::new(source);
    let out_components: Vec<_> = out.components().collect();
    let source_dir_components: Vec<_> = source_path
        .parent()
        .map(|parent| parent.components().collect())
        .unwrap_or_default();

    let mut common = 0;
    while common < out_components.len()
        && common < source_dir_components.len()
        && out_components[common] == source_dir_components[common]
    {
        common += 1;
    }

    let mut parts: Vec<String> = Vec::new();
    for _ in common..out_components.len() {
        parts.push("..".to_owned());
    }
    for component in &source_dir_components[common..] {
        parts.push(component.as_os_str().to_string_lossy().into_owned());
    }

    let stem = source_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.to_owned());
    parts.push(stem);

    let joined = parts.join("/");
    if joined.starts_with("..") {
        joined
    } else {
        format!("./{joined}")
    }
}

pub fn apply(plan: &Plan, check: bool, quiet: bool) -> Result<bool, String> {
    let mut current = true;

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

    Ok(current)
}

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

fn module_path_of(relative_source: &Path) -> String {
    let mut parts = vec!["crate".to_owned()];

    for component in relative_source.components() {
        let text = component.as_os_str().to_string_lossy().into_owned();
        let text = text.strip_suffix(".rs").unwrap_or(&text).to_owned();
        if matches!(text.as_str(), "mod" | "lib" | "main") {
            continue;
        }
        parts.push(text);
    }

    parts.join("::")
}

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

pub(crate) fn discover(options: &Options) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut sources = Vec::new();

    for entry in &options.src {
        let metadata =
            std::fs::metadata(entry).map_err(|error| format!("{}: {error}", display(entry)))?;

        if metadata.is_file() {
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
                || extension == "ts"
                || extension == "js"
        });
        if recognised && !is_generated(&path) {
            sources.push((root.to_path_buf(), path));
        }
    }

    Ok(())
}

fn is_generated(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| starts_with_a_marker(&text))
}

fn starts_with_a_marker(text: &str) -> bool {
    text.starts_with(generator::MARKER) || text.starts_with(generator::GDSCRIPT_MARKER)
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

pub fn display(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    text.strip_prefix("./").unwrap_or(&text).to_owned()
}

fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{check_module_names, display, module_path_of};

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
