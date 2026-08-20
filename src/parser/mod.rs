pub mod c;
pub mod cpp;
pub mod csharp;
pub mod gdscript;
pub mod go;
pub mod rust;
pub mod typescript;

use std::path::{Path, PathBuf};

use crate::model::Model;

#[derive(Debug)]
pub struct Error {
    pub path: PathBuf,
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.path.display(), self.line, self.message)
    }
}

pub fn parse(path: &Path, text: &str) -> Result<Vec<Model>, Error> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("go") => go::parse(path, text),
        Some("cs") => csharp::parse(path, text),
        Some("gd") => gdscript::parse(path, text),
        Some("hpp") | Some("cpp") | Some("cc") | Some("cxx") => cpp::parse(path, text),
        Some("c") | Some("h") => c::parse(path, text),
        Some("ts") | Some("js") => typescript::parse(path, text),
        _ => rust::parse(path, text),
    }
}
