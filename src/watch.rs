use std::collections::BTreeMap;
use std::time::Duration;

use crate::generate::{self, Options};
use crate::sha256;

pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(150);

pub const DEFAULT_SETTLE_INTERVAL: Duration = Duration::from_millis(75);

type Snapshot = BTreeMap<String, [u8; 32]>;

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

fn regenerate(options: &Options, quiet: bool) {
    let outcome = generate::plan(options).and_then(|plan| generate::apply(&plan, false, quiet));
    if let Err(error) = outcome {
        eprintln!("[cyclonec] error: {error}");
    }
    if !quiet {
        eprintln!("[cyclonec] watching for changes...");
    }
}

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
