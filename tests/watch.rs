//! `--watch`, driven at the library level rather than through the real
//! binary.
//!
//! `watch::run` polls in a loop with real (short) sleeps, so exercising it
//! properly means letting time actually pass - a real subprocess per
//! scenario (as `tests/cli.rs` prefers) would work too, but multiplies that
//! wait by however many scenarios there are. A background thread inside this
//! process, talking to the exact same `cyclonec::watch::run` the CLI calls,
//! gets the same coverage in a fraction of the time; `tests/cli.rs` still
//! covers `--watch` once, end to end, through the compiled binary.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cyclonec::generate::Options;
use cyclonec::watch;

// ===================================================================== harness

/// A scratch project of its own under `target/tests/`, starting with no
/// models at all - each test writes whatever source it wants from there.
fn project(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/tests")
        .join(format!("watch-{name}"));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("src/models")).expect("create project");
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

fn write(directory: &Path, relative: &str, contents: &str) {
    let path = directory.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(path, contents).expect("write source");
}

fn player(field_type: &str, codecs: &str) -> String {
    format!(
        "#[network]\n#[codec({codecs})]\npub struct Player {{\n    \
         #[network({field_type})]\n    #[codec({codecs})]\n    pub id: u32,\n}}\n"
    )
}

fn generated(directory: &Path, relative: &str) -> Option<String> {
    std::fs::read_to_string(directory.join("generated").join(relative)).ok()
}

/// Runs `watch::run` on a background thread against `directory`, with poll
/// and settle intervals short enough that a test does not have to wait long
/// for a change to be noticed. Returns a handle to join and a flag that
/// makes the loop return the next time it is checked.
fn spawn(directory: &Path) -> (std::thread::JoinHandle<Result<(), String>>, Arc<AtomicBool>) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_reader = Arc::clone(&stop);
    let options = options(directory);
    let handle = std::thread::spawn(move || {
        watch::run(
            &options,
            true,
            Duration::from_millis(15),
            Duration::from_millis(10),
            move || stop_reader.load(Ordering::Relaxed),
        )
    });
    (handle, stop)
}

fn stop_and_join(handle: std::thread::JoinHandle<Result<(), String>>, stop: &Arc<AtomicBool>) {
    stop.store(true, Ordering::Relaxed);
    handle
        .join()
        .expect("watch thread did not panic")
        .expect("watch returned an error");
}

/// Polls `condition` until it is true, or `timeout` passes - generous next
/// to the 15ms/10ms poll and settle intervals `spawn` uses, so a healthy
/// watch loop is never close to it.
fn wait_until(mut condition: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        if condition() {
            return true;
        }
        if start.elapsed() >= timeout {
            return condition();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

const TIMEOUT: Duration = Duration::from_secs(5);

// ======================================================================= tests

#[test]
fn source_file_modification_regenerates_the_affected_codec() {
    let directory = project("modification");
    write(&directory, "src/models/player.rs", &player("u32", "edge"));
    let (handle, stop) = spawn(&directory);

    assert!(
        wait_until(
            || generated(&directory, "player_edge.rs").is_some(),
            TIMEOUT
        ),
        "initial generation never produced player_edge.rs"
    );
    assert!(generated(&directory, "player_edge.rs")
        .unwrap()
        .contains("write_u32"));

    write(&directory, "src/models/player.rs", &player("f64", "edge"));
    assert!(
        wait_until(
            || generated(&directory, "player_edge.rs")
                .is_some_and(|text| text.contains("write_f64")),
            TIMEOUT
        ),
        "editing player.rs's field type was never reflected in its generated codec"
    );

    stop_and_join(handle, &stop);
}

#[test]
fn source_file_creation_generates_its_new_codec() {
    let directory = project("creation");
    let (handle, stop) = spawn(&directory);

    assert!(
        wait_until(|| generated(&directory, "runtime.rs").is_some(), TIMEOUT),
        "the initial scan and generation never ran"
    );
    assert!(
        generated(&directory, "player_edge.rs").is_none(),
        "nothing declared Player yet"
    );

    write(&directory, "src/models/player.rs", &player("u32", "edge"));
    assert!(
        wait_until(
            || generated(&directory, "player_edge.rs").is_some(),
            TIMEOUT
        ),
        "a newly created source file was never picked up"
    );

    stop_and_join(handle, &stop);
}

#[test]
fn source_file_deletion_removes_its_generated_codec() {
    let directory = project("deletion");
    write(&directory, "src/models/player.rs", &player("u32", "edge"));
    let (handle, stop) = spawn(&directory);

    assert!(
        wait_until(
            || generated(&directory, "player_edge.rs").is_some(),
            TIMEOUT
        ),
        "initial generation never produced player_edge.rs"
    );

    std::fs::remove_file(directory.join("src/models/player.rs")).expect("delete player.rs");
    assert!(
        wait_until(
            || generated(&directory, "player_edge.rs").is_none(),
            TIMEOUT
        ),
        "deleting the only source of Player never removed its generated codec"
    );

    stop_and_join(handle, &stop);
}

#[test]
fn a_parse_error_is_reported_and_watched_past_then_fixed() {
    let directory = project("parse-error");
    // `Vec<u32>` is not a Cyclone wire type - only `Array<u32>` is - so this
    // is invalid from the first tick onward.
    write(
        &directory,
        "src/models/player.rs",
        &player("Vec<u32>", "edge"),
    );
    let (handle, stop) = spawn(&directory);

    // Give the watch loop several ticks to have attempted (and failed) the
    // initial generation - it must still be alive and looping, not exited.
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        !handle.is_finished(),
        "an invalid annotation must be reported and watched past, not end the watch process"
    );
    assert!(
        generated(&directory, "player_edge.rs").is_none(),
        "an invalid model must not produce a codec"
    );

    write(&directory, "src/models/player.rs", &player("u32", "edge"));
    assert!(
        wait_until(
            || generated(&directory, "player_edge.rs").is_some(),
            TIMEOUT
        ),
        "fixing the source file was never regenerated"
    );

    stop_and_join(handle, &stop);
}

#[test]
fn generated_files_are_never_watched_as_a_source_of_their_own_changes() {
    let directory = project("no-feedback-loop");
    write(&directory, "src/models/player.rs", &player("u32", "edge"));
    let (handle, stop) = spawn(&directory);

    assert!(
        wait_until(
            || generated(&directory, "player_edge.rs").is_some(),
            TIMEOUT
        ),
        "initial generation never produced player_edge.rs"
    );

    // Give the loop a few settled ticks with nothing touched. If the output
    // directory were itself being watched, regenerating would keep rewriting
    // `generated-at`, `apply` would keep seeing that as "unchanged" (it
    // compares modulo timestamp) - so the file's mtime is the one honest
    // witness a feedback loop would leave behind that content-equality
    // would not catch.
    let before = std::fs::metadata(directory.join("generated/player_edge.rs"))
        .expect("metadata")
        .modified()
        .expect("mtime");
    std::thread::sleep(Duration::from_millis(400));
    let after = std::fs::metadata(directory.join("generated/player_edge.rs"))
        .expect("metadata")
        .modified()
        .expect("mtime");
    assert_eq!(
        before, after,
        "player_edge.rs was rewritten with nothing in src/ touched - the output directory is \
         being watched"
    );

    stop_and_join(handle, &stop);
}

#[test]
fn one_logical_save_that_fires_twice_is_regenerated_once() {
    let directory = project("duplicate-save");
    write(&directory, "src/models/player.rs", &player("u32", "edge"));
    let (handle, stop) = spawn(&directory);

    assert!(
        wait_until(
            || generated(&directory, "player_edge.rs").is_some(),
            TIMEOUT
        ),
        "initial generation never produced player_edge.rs"
    );

    // The way an editor's temp-file-then-rename save looks from here: two
    // writes, back to back, faster than the watch loop's settle interval
    // (10ms) - first an intermediate value, then the real one.
    write(&directory, "src/models/player.rs", &player("f32", "edge"));
    write(&directory, "src/models/player.rs", &player("f64", "edge"));

    assert!(
        wait_until(
            || generated(&directory, "player_edge.rs")
                .is_some_and(|text| text.contains("write_f64")),
            TIMEOUT
        ),
        "the final write of a rapid double-save was never reached"
    );
    // The intermediate value must never have been the one thing generated:
    // settling waits for the filesystem to stop changing before acting, so
    // the only content this codec should ever have shown after the double
    // write is the final one - never `write_f32` on its way there.
    assert!(
        !generated(&directory, "player_edge.rs")
            .unwrap()
            .contains("write_f32"),
        "the intermediate write of a rapid double-save was regenerated on its own - that is the \
         duplicate regeneration settling exists to avoid"
    );

    stop_and_join(handle, &stop);
}

#[test]
fn stopping_the_watch_leaves_no_further_regeneration_behind() {
    let directory = project("shutdown");
    write(&directory, "src/models/player.rs", &player("u32", "edge"));
    let (handle, stop) = spawn(&directory);

    assert!(
        wait_until(
            || generated(&directory, "player_edge.rs").is_some(),
            TIMEOUT
        ),
        "initial generation never produced player_edge.rs"
    );

    stop.store(true, Ordering::Relaxed);
    let stopped_at = Instant::now();
    handle
        .join()
        .expect("watch thread did not panic")
        .expect("watch returned an error");
    assert!(
        stopped_at.elapsed() < Duration::from_secs(1),
        "watch::run kept running well past being told to stop"
    );

    // Nothing left running: a change made after shutdown must sit there
    // unregenerated.
    write(&directory, "src/models/player.rs", &player("f64", "edge"));
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        generated(&directory, "player_edge.rs")
            .is_some_and(|text| text.contains("write_u32") && !text.contains("write_f64")),
        "a change after shutdown was regenerated - something is still watching in the background"
    );
}
