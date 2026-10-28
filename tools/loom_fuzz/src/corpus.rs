//! Corpus file discovery and loading for `loom_fuzz`.

use std::fs;
use std::path::{Path, PathBuf};

/// Corpus of initial `.loom` source inputs.
#[derive(Clone, Debug, Default)]
pub struct Corpus {
    inputs: Vec<String>,
}

impl Corpus {
    /// Loads all `.loom` files recursively from `dirs`.
    #[must_use]
    pub fn load<P: AsRef<Path>>(dirs: &[P]) -> Self {
        let mut inputs = Vec::new();
        for dir in dirs {
            collect_loom_files(dir.as_ref(), &mut inputs);
        }
        if inputs.is_empty() {
            // Default seed fallback if no files found
            inputs.push("def main()\n  puts \"hello\"\nend\n".to_string());
        }
        Self { inputs }
    }

    /// Accesses the underlying corpus inputs.
    #[must_use]
    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }

    /// Returns `true` if the corpus has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }

    /// Number of files in the corpus.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inputs.len()
    }
}

fn collect_loom_files(dir: &Path, acc: &mut Vec<String>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        paths.push(entry.path());
    }
    paths.sort(); // Sorting for deterministic traversal across filesystems

    for path in paths {
        if path.is_dir() {
            collect_loom_files(&path, acc);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("loom")
            && let Ok(content) = fs::read_to_string(&path)
            && !content.is_empty()
        {
            acc.push(content);
        }
    }
}
