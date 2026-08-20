use std::path::{Path, PathBuf};

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

pub fn import_path(module: &str, relative_source: &Path) -> String {
    match relative_source.parent() {
        Some(directory) => import_path_of_dir(module, directory),
        None => import_path_of_dir(module, Path::new("")),
    }
}

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
