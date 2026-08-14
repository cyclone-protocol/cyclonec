//! `--watch`: reread source on change, regenerate, keep going.
//!
//! No filesystem-event API is worth a dependency for (see `Cargo.toml`: none
//! are wanted, in the generator or in what it generates), so this polls.
//! Every tick it rereads exactly the file list [`crate::generate::plan`]
//! would - never the output directory, never a file this generator wrote
//! itself, since [`crate::generate::discover`] already excludes both - and
//! hashes each file's contents with the crate's own [`crate::sha256`]. A
//! save that fires several filesystem events (a temp file, then a rename)
//! never causes two regenerations: polling only ever sees the *result* of
//! however many events happened between two ticks, and a change is acted on
//! only once it has stopped changing for a whole `settle_interval`, not on
//! the first tick that notices it.
//!
//! Regeneration itself is not incremental at the parse level, and cannot be
//! without redesigning the compiler: [`crate::ir::Schema::build`] takes every
//! model in the project at once, because one model's fields can name
//! another's regardless of which file declared it, and a fingerprint is
//! computed over the whole schema's canonical form, not file by file. What
//! *is* incremental, and needs no change here to be so, is the write: `apply`
//! already compares each generated file's new contents against what is on
//! disk and skips the ones that did not change, so editing one model still
//! only ever rewrites that model's own codecs (and, for a shape-changing
//! evolution, the shared `schema.json` / `build-graph.json` alongside them).

use std::collections::BTreeMap;
use std::time::Duration;

use crate::generate::{self, Options};
use crate::sha256;

/// How often to reread the source tree.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(150);

/// How long a changed tree must sit still before it is regenerated - long
/// enough that an editor's temp-file-then-rename save is very unlikely to be
/// seen as two changes instead of one.
pub const DEFAULT_SETTLE_INTERVAL: Duration = Duration::from_millis(75);

/// One tick's worth of "what does source look like right now": every
/// relevant file's project-relative path, paired with a digest of its
/// contents. Two snapshots are equal exactly when nothing relevant was
/// added, removed, or edited between them.
type Snapshot = BTreeMap<String, [u8; 32]>;

/// Watches `options.src` and regenerates `options.out` on every settled
/// change, until `should_stop` returns `true`.
///
/// Performs the initial scan and generation before doing anything else -
/// `should_stop` is not consulted until after it - so a caller that stops
/// watching immediately still gets one full generation out of this call.
///
/// # Errors
///
/// Only ever the same errors `--src` itself would produce outside watch mode
/// (a path that does not exist, e.g.) - never an error in a *model*: an
/// invalid annotation or an unparsable file is reported to stderr and
/// watched past, not returned, because a mistake in a source file is not a
/// reason to end the process that would otherwise let someone fix it.
pub fn run(
    options: &Options,
    quiet: bool,
    poll_interval: Duration,
    settle_interval: Duration,
    mut should_stop: impl FnMut() -> bool,
) -> Result<(), String> {
    regenerate(options, quiet);

    let mut last = snapshot(options)?;

    while !should_stop() {
        std::thread::sleep(poll_interval);
        if should_stop() {
            return Ok(());
        }

        let current = snapshot(options)?;
        if current == last {
            continue;
        }

        // Settle: keep sampling until two consecutive snapshots agree, so a
        // save that touches the filesystem more than once - a temp file and
        // a rename, e.g. - is regenerated from its final state, once.
        let mut settled = current;
        loop {
            std::thread::sleep(settle_interval);
            if should_stop() {
                return Ok(());
            }
            let next = snapshot(options)?;
            if next == settled {
                break;
            }
            settled = next;
        }

        last = settled;
        regenerate(options, quiet);
    }

    Ok(())
}

/// One generation attempt: plan, apply, and say what happened - an error
/// included, since watch mode's whole point is to survive one and keep
/// going.
fn regenerate(options: &Options, quiet: bool) {
    let outcome = generate::plan(options).and_then(|plan| generate::apply(&plan, false, quiet));
    if let Err(error) = outcome {
        eprintln!("[cyclonec] error: {error}");
    }
    if !quiet {
        eprintln!("[cyclonec] watching for changes...");
    }
}

/// The current content-hash of every file [`generate::discover`] would read
/// for `options` right now.
///
/// A file that cannot be read at the moment of hashing - mid-write, or
/// mid-rename - is left out of the snapshot rather than treated as an error:
/// the next tick reads it again, and until then a snapshot simply missing
/// that one key is still a snapshot that compares unequal to one that has
/// it, which is exactly the "something changed" signal a save in progress
/// ought to produce.
fn snapshot(options: &Options) -> Result<Snapshot, String> {
    let sources = generate::discover(options)?;
    let mut snapshot = Snapshot::new();
    for (_, path) in sources {
        if let Ok(text) = std::fs::read_to_string(&path) {
            snapshot.insert(generate::display(&path), sha256::hash(text.as_bytes()));
        }
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::run;
    use crate::cli::Paths;
    use crate::generate::Options;

    /// A scratch project of its own under `target/tests/watch-<name>`, with
    /// one annotated Rust model - the same fixture shape `tests/cli.rs` uses,
    /// built by hand here since this module cannot reach `tests/`.
    fn project(name: &str) -> PathBuf {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/tests")
            .join(format!("watch-{name}"));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(directory.join("src/models")).expect("create project");
        std::fs::write(
            directory.join("src/models/player.rs"),
            "#[network]\n#[codec(edge)]\npub struct Player {\n    \
             #[network(u32)]\n    #[codec(edge)]\n    pub id: u32,\n}\n",
        )
        .expect("write player.rs");
        directory
    }

    fn options(directory: &Path) -> Options {
        Options {
            src: vec![directory.join("src")],
            out: directory.join("generated"),
            root: directory.to_path_buf(),
            model_path: None,
            validate_message_fingerprint: false,
        }
    }

    #[test]
    fn the_initial_generation_happens_even_if_told_to_stop_immediately() {
        let directory = project("initial");

        run(
            &options(&directory),
            true,
            Duration::from_millis(5),
            Duration::from_millis(5),
            || true,
        )
        .expect("watch");

        assert!(
            directory.join("generated/player_edge.rs").exists(),
            "the initial scan and generation runs before `should_stop` is ever consulted"
        );
    }

    #[test]
    fn it_returns_promptly_once_should_stop_says_so() {
        let directory = project("shutdown");
        let calls = Cell::new(0);

        let started = std::time::Instant::now();
        run(
            &options(&directory),
            true,
            Duration::from_millis(5),
            Duration::from_millis(5),
            || {
                calls.set(calls.get() + 1);
                calls.get() > 3
            },
        )
        .expect("watch");

        assert!(
            started.elapsed() < Duration::from_secs(2),
            "a synchronous poll loop with a stop condition that goes true returns promptly, \
             leaving nothing running in the background"
        );
    }

    #[test]
    fn an_unresolvable_src_is_reported_rather_than_watched_forever() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/tests/watch-missing-src-does-not-exist");
        let _ = std::fs::remove_dir_all(&directory);

        let paths = Paths {
            src: vec![directory.join("src")],
            out: Some(directory.join("generated")),
            model_path: None,
        };
        // `Options::resolve` reads `cyclone.toml` from the process's own
        // current directory, which this test does not want to depend on -
        // built by hand instead, the same as `options()` above.
        let options = Options {
            src: paths.src,
            out: paths.out.expect("out"),
            root: directory.clone(),
            model_path: None,
            validate_message_fingerprint: false,
        };

        let error = run(
            &options,
            true,
            Duration::from_millis(5),
            Duration::from_millis(5),
            || true,
        )
        .expect_err("no such directory");
        assert!(error.contains("src"), "{error}");
    }
}
