//! `cargo xtask core-tests`: runs `loom test` on every test file in `std/core/tests/`.

use std::path::Path;
use std::process::Command;

use crate::conformance::build_loom;
use crate::workspace_root;

/// Runs `loom test` on every `.loom` file in `std/core/tests/` and prints a summary.
pub fn run() -> Result<(), String> {
    let root = workspace_root();
    let binary = build_loom(&root)?;
    let directory = root.join("std").join("core").join("tests");
    let mut entries: Vec<_> = std::fs::read_dir(&directory)
        .map_err(|error| format!("could not read `{}`: {error}", directory.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "loom")
        })
        .collect();
    entries.sort();

    let mut total_passed = 0;
    let mut failures = Vec::new();

    for path in &entries {
        let relative = path.strip_prefix(&root).unwrap_or(path);
        match run_test_file(&binary, &root, relative) {
            Ok(passed_count) => total_passed += passed_count,
            Err(message) => failures.push(format!("{}: {message}", relative.display())),
        }
    }

    println!(
        "core-tests: {total_passed} passed, {} failed across {} files",
        failures.len(),
        entries.len()
    );

    if failures.is_empty() {
        Ok(())
    } else {
        for failure in &failures {
            eprintln!("  {failure}");
        }
        Err("core-tests failed".to_owned())
    }
}

fn run_test_file(binary: &Path, root: &Path, relative: &Path) -> Result<usize, String> {
    let output = Command::new(binary)
        .arg("test")
        .arg(relative)
        .current_dir(root)
        .output()
        .map_err(|error| format!("could not run `{}`: {error}", binary.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "exit status {:?}\nstderr:\n{}\nstdout:\n{}",
            output.status.code(),
            stderr.trim(),
            stdout.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_passed_count(&stdout)
        .ok_or_else(|| format!("could not parse test summary from stdout:\n{stdout}"))
}

/// Parses the number of passed tests from `test result: ok. X passed; Y failed`.
fn parse_passed_count(stdout: &str) -> Option<usize> {
    stdout.lines().find_map(|line| {
        let rest = line.strip_prefix("test result: ok. ")?;
        let passed_str = rest.split_whitespace().next()?;
        passed_str.parse::<usize>().ok()
    })
}
