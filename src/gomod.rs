//! `go.mod` - just enough to compute an import path.
//!
//! Go resolves an import path from two things: the `module` line of the
//! nearest enclosing `go.mod`, and the directory a package sits in relative to
//! it. That is the entire reason this file exists - to read the one line and
//! do that join - not to understand `require`, `replace`, `go`, or anything
//! else `go.mod` may hold.
//!
//! ```text
//! go.mod:  module github.com/acme/game
//! package: internal/models/player.go
//! import path:  github.com/acme/game/internal/models
//! ```

use std::path::{Path, PathBuf};

/// The nearest `go.mod` at or above `start`, and the module path its `module`
/// line declares.
///
/// # Errors
///
/// A `go.mod` was found but has no `module` line, or cannot be read.
pub fn find(start: &Path) -> Result<Option<(PathBuf, String)>, String> {
    let mut directory = Some(start);

    while let Some(current) = directory {
        let candidate = current.join("go.mod");
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate)
                .map_err(|error| format!("{}: {error}", candidate.display()))?;
            let module = module_line(&text)
                .ok_or_else(|| format!("{}: no `module` line", candidate.display()))?;
            return Ok(Some((current.to_path_buf(), module)));
        }
        directory = current.parent();
    }

    Ok(None)
}

/// The path after a top-level `module` line, if there is one.
fn module_line(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("module ") {
            let path = rest.trim();
            if !path.is_empty() {
                return Some(path.to_owned());
            }
        }
    }
    None
}

/// The import path of the package `relative_source` (a `.go` file, relative to
/// the module root) lives in.
///
/// Go imports a **directory**, never a file, so the file name itself is
/// dropped - `player.go` and `team.go` in the same directory share one import
/// path.
pub fn import_path(module: &str, relative_source: &Path) -> String {
    match relative_source.parent() {
        Some(directory) => import_path_of_dir(module, directory),
        None => import_path_of_dir(module, Path::new("")),
    }
}

/// The import path of the package that lives in `relative_dir`, a directory
/// relative to the module root - the same join [`import_path`] does for a
/// file, applied directly to a directory (what a generated package's own
/// `--out` needs, since there is no file to drop).
pub fn import_path_of_dir(module: &str, relative_dir: &Path) -> String {
    let mut parts = vec![module.trim_end_matches('/').to_owned()];

    for component in relative_dir.components() {
        let text = component.as_os_str().to_string_lossy().into_owned();
        if !text.is_empty() && text != "." {
            parts.push(text);
        }
    }

    parts.join("/")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{import_path, module_line};

    #[test]
    fn reads_the_module_line_and_ignores_the_rest() {
        assert_eq!(
            module_line("module github.com/acme/game\n\ngo 1.21\n"),
            Some("github.com/acme/game".to_owned())
        );
        assert_eq!(module_line("go 1.21\n"), None);
    }

    #[test]
    fn an_import_path_drops_the_file_name_and_keeps_the_directory() {
        assert_eq!(
            import_path(
                "github.com/acme/game",
                Path::new("internal/models/player.go")
            ),
            "github.com/acme/game/internal/models"
        );
        assert_eq!(
            import_path("github.com/acme/game", Path::new("player.go")),
            "github.com/acme/game"
        );
    }
}
