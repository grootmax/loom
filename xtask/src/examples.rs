//! `cargo xtask examples`: runs every example whose phase has been reached and compares
//! its output with `<name>.stdout` (see `examples/README.md`).

use std::path::Path;
use std::process::Command;

use crate::conformance::{CURRENT_PHASE, build_loom};
use crate::workspace_root;

/// Runs the examples and fails if any output differs.
pub fn run() -> Result<(), String> {
    let root = workspace_root();
    let binary = build_loom(&root)?;
    let directory = root.join("examples");
    let mut entries: Vec<_> = std::fs::read_dir(&directory)
        .map_err(|error| format!("could not read `{}`: {error}", directory.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "loom")
        })
        .collect();
    entries.sort();
    let (mut passed, mut skipped) = (0, 0);
    let mut failures = Vec::new();
    for path in &entries {
        let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        let header = |key: &str| {
            text.lines().find_map(|line| {
                line.strip_prefix(&format!("# {key}:"))
                    .map(|v| v.trim().to_owned())
            })
        };
        let phase: u32 = header("phase")
            .and_then(|p| p.parse().ok())
            .unwrap_or(u32::MAX);
        if phase > CURRENT_PHASE {
            skipped += 1;
            continue;
        }
        let mode = header("run").unwrap_or_else(|| "run".to_owned());
        let relative = path.strip_prefix(&root).unwrap_or(path);
        match check(&binary, &root, relative, &mode) {
            Ok(()) => passed += 1,
            Err(message) => failures.push(format!("{}: {message}", relative.display())),
        }
    }
    println!(
        "examples: {passed} passed, {} failed, {skipped} skipped (later phases)",
        failures.len()
    );
    if failures.is_empty() {
        Ok(())
    } else {
        for failure in &failures {
            eprintln!("  {failure}");
        }
        Err("examples failed".to_owned())
    }
}

fn check(binary: &Path, root: &Path, example: &Path, mode: &str) -> Result<(), String> {
    let command = match mode {
        "run" => "run",
        "test" => "test",
        "build-only" => "check",
        other => return Err(format!("unknown run mode `{other}`")),
    };
    let output = Command::new(binary)
        .arg(command)
        .arg(example)
        .current_dir(root)
        .output()
        .map_err(|error| format!("could not run `{}`: {error}", binary.display()))?;
    if !output.status.success() {
        return Err(format!(
            "exit status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if command == "check" {
        return Ok(());
    }
    let expected_path = root.join(example).with_extension("stdout");
    let expected = std::fs::read_to_string(&expected_path)
        .map_err(|error| format!("could not read `{}`: {error}", expected_path.display()))?
        .replace("\r\n", "\n");
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    if stdout == expected {
        Ok(())
    } else {
        Err(format!(
            "output differs:\n--- expected\n{expected}--- got\n{stdout}"
        ))
    }
}
