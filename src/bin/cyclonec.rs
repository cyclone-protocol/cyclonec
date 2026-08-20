use std::process::{Command, ExitCode};

use cyclonec::cli::{self, CiArgs, CompatArgs, GenerateArgs};
use cyclonec::compat::{self, Verdict};
use cyclonec::generate::{self, Options};
use cyclonec::ir::Schema;
use cyclonec::schema;
use cyclonec::watch;

fn main() -> ExitCode {
    let command = match cli::parse(std::env::args_os().skip(1)) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("cyclonec: {message}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };

    let result = match command {
        cli::Command::Help => {
            print!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        cli::Command::Version => {
            println!("cyclonec {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        cli::Command::Generate(arguments) if arguments.watch => run_watch(&arguments),
        cli::Command::Generate(arguments) => run_generate(&arguments),
        cli::Command::Compat(arguments) => run_compat(&arguments),
        cli::Command::Ci(arguments) => run_ci(&arguments),
    };

    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("cyclonec: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run_generate(arguments: &GenerateArgs) -> Result<ExitCode, String> {
    let options = Options::resolve(&arguments.paths)?;
    let plan = generate::plan(&options)?;

    if plan.schema.models.is_empty() && !arguments.quiet {
        eprintln!(
            "cyclonec: no #[network] model found under {}",
            options
                .src
                .iter()
                .map(|path| generate::display(path))
                .collect::<Vec<String>>()
                .join(", ")
        );
    }

    if let Ok(text) = std::fs::read_to_string(options.schema_path()) {
        match schema::from_json(&text) {
            Ok(previous) => {
                let report = compat::compare(&previous, &plan.schema);
                let rendered = report.render(!arguments.quiet);
                if !rendered.is_empty() {
                    print!("{rendered}");
                }
                if report.verdict() == Verdict::Breaking {
                    println!(
                        "BREAKING CHANGE. Generated anyway - a schema is yours to break. \
                         `cyclonec ci --base-ref <branch>` is what stops it reaching a peer."
                    );
                }
            }
            Err(problem) => eprintln!(
                "cyclonec: {} could not be read ({problem}); generating without a comparison",
                generate::display(&options.schema_path())
            ),
        }
    }

    let current = generate::apply(&plan, arguments.check, arguments.quiet)?;

    if arguments.check {
        if !current {
            eprintln!("cyclonec: run `cyclonec generate` to bring it up to date");
            return Ok(ExitCode::FAILURE);
        }
        if !arguments.quiet {
            eprintln!("cyclonec: up to date");
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn run_watch(arguments: &GenerateArgs) -> Result<ExitCode, String> {
    let options = Options::resolve(&arguments.paths)?;
    watch::run(
        &options,
        arguments.quiet,
        watch::DEFAULT_POLL_INTERVAL,
        watch::DEFAULT_SETTLE_INTERVAL,
        || false,
    )?;
    Ok(ExitCode::SUCCESS)
}

fn run_compat(arguments: &CompatArgs) -> Result<ExitCode, String> {
    let base = read_schema(&arguments.base)?;
    let head = match &arguments.head {
        Some(path) => read_schema(path)?,
        None => {
            let options = Options::resolve(&arguments.paths)?;
            generate::plan(&options)?.schema
        }
    };

    let report = compat::compare(&base, &head);
    print!("{}", report.render(!arguments.quiet));

    let verdict = report.verdict();
    println!("{}", verdict.label());

    Ok(match verdict {
        Verdict::Breaking => ExitCode::FAILURE,
        _ => ExitCode::SUCCESS,
    })
}

fn run_ci(arguments: &CiArgs) -> Result<ExitCode, String> {
    let options = Options::resolve(&arguments.paths)?;
    let plan = generate::plan(&options)?;

    let path = options.schema_path();
    let committed = std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "{}: {error}\nRun `cyclonec generate` and commit it: CI has nothing to compare \
             against without it.",
            generate::display(&path)
        )
    })?;

    if committed != schema::to_json(&plan.schema) {
        eprintln!(
            "{} does not match the source in this branch.",
            generate::display(&path)
        );
        eprintln!("Run `cyclonec generate` and commit the result.");
        return Ok(ExitCode::FAILURE);
    }
    if !arguments.quiet {
        println!("✓ {} matches the source", generate::display(&path));
    }

    let Some(text) = git_show(&arguments.base_ref, schema::PATH)? else {
        println!(
            "{}:{} does not exist - the target branch has no schema yet, so there is nothing \
             to compare against.",
            arguments.base_ref,
            schema::PATH
        );
        println!("{}", Verdict::Compatible.label());
        return Ok(ExitCode::SUCCESS);
    };

    let base = schema::from_json(&text)
        .map_err(|problem| format!("{}:{}: {problem}", arguments.base_ref, schema::PATH))?;

    let report = compat::compare(&base, &plan.schema);
    if !arguments.quiet {
        println!("\n{} → this branch\n", arguments.base_ref);
    }
    print!("{}", report.render(!arguments.quiet));

    let verdict = report.verdict();
    println!("\n{}", verdict.label());

    Ok(match verdict {
        Verdict::Breaking => {
            println!(
                "A peer running the target branch would misread this branch's messages. \
                 Append fields at the end, or version the message."
            );
            ExitCode::FAILURE
        }
        _ => ExitCode::SUCCESS,
    })
}

fn read_schema(path: &std::path::Path) -> Result<Schema, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("{}: {error}", generate::display(path)))?;
    schema::from_json(&text).map_err(|problem| format!("{}: {problem}", generate::display(path)))
}

fn git_show(reference: &str, path: &str) -> Result<Option<String>, String> {
    let output = Command::new("git")
        .args(["show", &format!("{reference}:{path}")])
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;

    if output.status.success() {
        return String::from_utf8(output.stdout)
            .map(Some)
            .map_err(|_| format!("{reference}:{path} is not UTF-8"));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("does not exist") || stderr.contains("exists on disk, but not in") {
        return Ok(None);
    }
    Err(format!(
        "git show {reference}:{path} failed: {}",
        stderr.trim()
    ))
}
