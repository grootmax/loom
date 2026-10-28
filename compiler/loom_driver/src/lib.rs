//! Query database and compilation orchestration.
//!
//! The driver loads files, parses them, and analyzes programs: lowering to HIR with the
//! embedded core library, name resolution and type checking ([`Session::analyze`]). In
//! Phase 2 each root file is its own program and nothing is memoized across programs;
//! a query engine arrives with the language server ([ADR 0019] §1).
//!
//! [ADR 0019]: https://github.com/grootmax/loom/blob/main/docs/adr/0019-semantic-analysis-and-interpreter.md

use std::fmt;
use std::path::{Path, PathBuf};

use loom_ast::AstNode;
use loom_check::Checked;
use loom_diagnostics::Diagnostic;
use loom_hir::{DefId, DefKind, Hir, Package};
use loom_parser::Parse;
use loom_resolve::Resolution;
use loom_span::{FileId, SourceMap};

/// The core library's source files (ADR 0019 §4): `lib.loom` is the root module and each
/// other file is a child module named after it.
pub const CORE_FILES: &[(&str, &str)] = &[
    ("lib", include_str!("../../../std/core/lib.loom")),
    ("traits", include_str!("../../../std/core/traits.loom")),
    (
        "primitives",
        include_str!("../../../std/core/primitives.loom"),
    ),
    ("option", include_str!("../../../std/core/option.loom")),
    ("string", include_str!("../../../std/core/string.loom")),
    ("array", include_str!("../../../std/core/array.loom")),
    ("map", include_str!("../../../std/core/map.loom")),
    ("iter", include_str!("../../../std/core/iter.loom")),
    ("io", include_str!("../../../std/core/io.loom")),
    ("prelude", include_str!("../../../std/core/prelude.loom")),
];

/// A program after semantic analysis.
#[derive(Debug)]
pub struct Analysis {
    /// The program's HIR (core library included).
    pub hir: Hir,
    /// Name resolution.
    pub resolution: Resolution,
    /// Types.
    pub checked: Checked,
    /// The root module of the program.
    pub root: DefId,
    /// Parse, resolution and type errors, in source order.
    pub diagnostics: Vec<Diagnostic>,
}

impl Analysis {
    /// Whether any error was found.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }

    /// The root module's `main` function, if it has one.
    #[must_use]
    pub fn main(&self) -> Option<DefId> {
        let DefKind::Module(module) = &self.hir.def(self.root).kind else {
            return None;
        };
        module.items.iter().copied().find(|&item| {
            self.hir.def(item).name.as_str() == "main"
                && matches!(self.hir.def(item).kind, DefKind::Fn(_))
        })
    }
}

/// One compilation session: the loaded files and their parses.
#[derive(Debug, Default)]
pub struct Session {
    sources: SourceMap,
    parses: Vec<(FileId, PathBuf, Parse)>,
    core: Vec<(FileId, &'static str, Parse)>,
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

    /// Lowers, resolves and checks the program whose root is `file`, together with the
    /// core library.
    ///
    /// # Panics
    ///
    /// If `file` was not loaded in this session.
    pub fn analyze(&mut self, file: FileId) -> Analysis {
        self.load_core();
        let mut hir = Hir::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let mut core_root = None;
        for (id, name, parse) in &self.core {
            diagnostics.extend(parse.diagnostics().iter().cloned());
            let tree = loom_ast::SourceFile::cast(parse.syntax())
                .expect("a parse is rooted at SOURCE_FILE");
            if *name == "lib" {
                core_root = Some(loom_hir::lower_file(
                    &mut hir,
                    *id,
                    &tree,
                    Package::Core,
                    None,
                    "",
                ));
            } else {
                let module =
                    loom_hir::lower_file(&mut hir, *id, &tree, Package::Core, core_root, name);
                if let Some(root) = core_root
                    && let DefKind::Module(data) = &mut hir.defs[root.index()].kind
                {
                    data.items.push(module);
                }
            }
        }
        let parse = self.parse(file).expect("the file was loaded");
        diagnostics.extend(parse.diagnostics().iter().cloned());
        let tree =
            loom_ast::SourceFile::cast(parse.syntax()).expect("a parse is rooted at SOURCE_FILE");
        let root = loom_hir::lower_file(&mut hir, file, &tree, Package::Main, None, "");
        let resolution = loom_resolve::resolve(&hir);
        let checked = loom_check::check(&hir, &resolution);
        // A program with lexical or syntax errors reports only those (spec Appendix C):
        // later errors in such a program are mostly consequences of the first ones.
        if !diagnostics.iter().any(Diagnostic::is_error) {
            diagnostics.extend(resolution.diagnostics.iter().cloned());
            diagnostics.extend(checked.diagnostics.iter().cloned());
        }
        diagnostics.sort_by_key(|d| (d.primary.span.file, d.primary.span.range.start()));
        diagnostics.dedup();
        Analysis {
            hir,
            resolution,
            checked,
            root,
            diagnostics,
        }
    }

    fn load_core(&mut self) {
        if !self.core.is_empty() {
            return;
        }
        for (name, text) in CORE_FILES {
            let path = format!("<core>/{name}.loom");
            let Ok(id) = self.sources.add(path, (*text).to_owned()) else {
                continue;
            };
            let parse = loom_parser::parse(text, id);
            self.core.push((id, name, parse));
        }
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
