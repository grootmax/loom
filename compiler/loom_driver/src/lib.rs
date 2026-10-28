//! Query database and compilation orchestration.
//!
//! In Phase 1 the driver loads files, parses them and collects diagnostics. Each file is
//! independent, so there is nothing to memoize yet. The salsa query database arrives with
//! name resolution in Phase 2 ([ADR 0018] §11).
//!
//! [ADR 0018]: https://github.com/grootmax/loom/blob/main/docs/adr/0018-front-end-design.md

use std::fmt;
use std::path::{Path, PathBuf};

use loom_diagnostics::Diagnostic;
use loom_parser::Parse;
use loom_span::{FileId, SourceMap};

/// One compilation session: the loaded files and their parses.
#[derive(Debug, Default)]
pub struct Session {
    sources: SourceMap,
    parses: Vec<(FileId, PathBuf, Parse)>,
}

/// A file could not be loaded.
#[derive(Debug)]
pub struct LoadError {
    /// The file.
    pub path: PathBuf,
    /// What went wrong.
    pub message: String,
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for LoadError {}

impl Session {
    /// An empty session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The source map of every loaded file.
    #[must_use]
    pub fn sources(&self) -> &SourceMap {
        &self.sources
    }

    /// Reads and parses the file at `path`. Invalid UTF-8 is reported as E0001 and the
    /// file is parsed from its lossy decoding.
    ///
    /// # Errors
    ///
    /// Returns a [`LoadError`] if the file cannot be read or is larger than 4 GiB.
    pub fn load(&mut self, path: &Path) -> Result<FileId, LoadError> {
        let bytes = std::fs::read(path).map_err(|error| LoadError {
            path: path.to_owned(),
            message: error.to_string(),
        })?;
        self.add_bytes(path, &bytes)
    }

    /// Parses `bytes` as the contents of `path` (which need not exist).
    ///
    /// # Errors
    ///
    /// Returns a [`LoadError`] if the input is larger than 4 GiB.
    pub fn add_bytes(&mut self, path: &Path, bytes: &[u8]) -> Result<FileId, LoadError> {
        let too_large = |message: String| LoadError {
            path: path.to_owned(),
            message,
        };
        if u32::try_from(bytes.len()).is_err() {
            return Err(too_large("source files are limited to 4 GiB".to_owned()));
        }
        // The lossy decoding is deterministic, so the source map holds exactly the text
        // the parser sees.
        let text = String::from_utf8_lossy(bytes).into_owned();
        let file = self
            .sources
            .add(path.display().to_string(), text)
            .map_err(|error| too_large(error.to_string()))?;
        let (_, parse) = loom_parser::parse_bytes(bytes, file);
        self.parses.push((file, path.to_owned(), parse));
        Ok(file)
    }

    /// The parse of a loaded file.
    #[must_use]
    pub fn parse(&self, file: FileId) -> Option<&Parse> {
        self.parses
            .iter()
            .find(|(id, _, _)| *id == file)
            .map(|(_, _, parse)| parse)
    }

    /// Every loaded file with its path and parse, in load order.
    pub fn files(&self) -> impl Iterator<Item = (FileId, &Path, &Parse)> {
        self.parses
            .iter()
            .map(|(id, path, parse)| (*id, path.as_path(), parse))
    }

    /// Every diagnostic of every file, in load order and then source order.
    pub fn diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.parses
            .iter()
            .flat_map(|(_, _, parse)| parse.diagnostics())
    }

    /// Whether any loaded file has an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.parses.iter().any(|(_, _, parse)| parse.has_errors())
    }
}

/// Collects `.loom` files: each path that is a file is taken as is; directories are
/// searched recursively, in sorted order.
///
/// # Errors
///
/// Returns a [`LoadError`] for a path that does not exist or a directory that cannot be read.
pub fn collect_sources(paths: &[PathBuf]) -> Result<Vec<PathBuf>, LoadError> {
    let mut files = Vec::new();
    for path in paths {
        collect_into(path, &mut files, true)?;
    }
    Ok(files)
}

fn collect_into(path: &Path, files: &mut Vec<PathBuf>, explicit: bool) -> Result<(), LoadError> {
    let error = |message: String| LoadError {
        path: path.to_owned(),
        message,
    };
    let metadata = std::fs::metadata(path).map_err(|e| error(e.to_string()))?;
    if metadata.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|e| error(e.to_string()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        entries.sort();
        for entry in entries {
            let hidden = entry
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'));
            if !hidden && entry.file_name().is_some_and(|name| name != "target") {
                collect_into(&entry, files, false)?;
            }
        }
    } else if explicit || path.extension().is_some_and(|ext| ext == "loom") {
        files.push(path.to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Session;

    #[test]
    fn parses_and_reports_with_file_positions() {
        let mut session = Session::new();
        let good = session
            .add_bytes(Path::new("good.loom"), b"def main\n  puts 1\nend\n")
            .unwrap();
        let bad = session
            .add_bytes(Path::new("bad.loom"), b"def main\n  x = 1; y = 2\nend\n")
            .unwrap();
        assert!(!session.parse(good).unwrap().has_errors());
        assert!(session.has_errors());
        let short: Vec<String> = session
            .diagnostics()
            .map(|d| d.short(session.sources()))
            .collect();
        assert_eq!(
            short,
            ["bad.loom:2:8: error[E0006]: `;` is not used in Loom"]
        );
        assert_eq!(
            session.sources().file(bad).line_text(1),
            Some("  x = 1; y = 2")
        );
    }

    #[test]
    fn invalid_utf8_is_reported_once() {
        let mut session = Session::new();
        session
            .add_bytes(Path::new("x.loom"), b"# \xff\ndef main\nend\n")
            .unwrap();
        let codes: Vec<String> = session.diagnostics().map(|d| d.id.to_string()).collect();
        assert_eq!(codes, ["E0001"]);
    }
}
