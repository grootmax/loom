//! `cargo xtask conformance`: runs `spec/conformance/` against this implementation.
//!
//! The rules are in `spec/conformance/README.md`. In Phase 2:
//!
//! - a reject case (`.diag`) whose codes are all implemented (E0001–E0499) must produce
//!   exactly those codes at exactly those positions;
//! - an accept case (`.stdout`) is run with `loom run`; its standard output must equal
//!   the file, and its exit status must be 0 (or the number in `<name>.exit`);
//! - a case whose second line is `# phase: N` with `N` greater than the current phase,
//!   or whose codes belong to a later phase, is skipped.
//!
//! The runner stays std-only: it builds the `loom` binary, parses the one-line
//! diagnostics of `loom check --message-format=short`, and runs accept cases.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::workspace_root;

/// The highest error code the current phase implements.
const IMPLEMENTED_UP_TO: u32 = 599;

/// The current roadmap phase.
pub const CURRENT_PHASE: u32 = 3;

/// Runs the suite and fails if any case does not match.
pub fn run() -> Result<(), String> {
    let root = workspace_root();
    let cases = collect_cases(&root.join("spec").join("conformance"))?;
    if cases.is_empty() {
        println!("conformance: no cases");
        return Ok(());
    }
    let binary = build_loom(&root)?;
    let cases: Vec<PathBuf> = cases;
    let mut command = Command::new(&binary);
    command
        .arg("check")
        .arg("--message-format=short")
        .current_dir(&root);
    for case in &cases {
        command.arg(case);
    }
    let output = command
        .output()
        .map_err(|error| format!("could not run `{}`: {error}", binary.display()))?;
    let reported = parse_report(&String::from_utf8_lossy(&output.stderr));
    let runner = Runner {
        root: &root,
        binary: &binary,
    };

    let mut failures = Vec::new();
    let (mut passed, mut skipped) = (0, 0);
    for case in &cases {
        // Reported paths use `/`; so do the keys, on every platform.
        let key = case.display().to_string().replace('\\', "/");
        let got = reported.get(&key).cloned().unwrap_or_default();
        match runner.check_case(case, &got)? {
            Outcome::Passed => passed += 1,
            Outcome::Skipped => skipped += 1,
            Outcome::Failed(message) => failures.push(format!("{key}: {message}")),
        }
    }
    println!(
        "conformance: {passed} passed, {} failed, {skipped} skipped (later phases)",
        failures.len()
    );
    if failures.is_empty() {
        Ok(())
    } else {
        for failure in &failures {
            eprintln!("  {failure}");
        }
        Err("conformance failed".to_owned())
    }
}

enum Outcome {
    Passed,
    Skipped,
    Failed(String),
}

/// Every `.loom` file under `directory`, relative to the workspace root, sorted.
fn collect_cases(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut cases = Vec::new();
    collect_into(directory, &mut cases)?;
    cases.sort();
    let root = workspace_root();
    Ok(cases
        .into_iter()
        .map(|path| {
            path.strip_prefix(&root)
                .map(Path::to_path_buf)
                .unwrap_or(path)
        })
        .collect())
}

fn collect_into(directory: &Path, cases: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("could not read `{}`: {error}", directory.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            collect_into(&path, cases)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "loom")
        {
            cases.push(path);
        }
    }
    Ok(())
}

pub fn build_loom(root: &Path) -> Result<PathBuf, String> {
    crate::run(
        root,
        "cargo",
        // The runtime's static library is linked into `loom build` executables.
        &[
            "build",
            "--quiet",
            "--locked",
            "-p",
            "loom_cli",
            "-p",
            "loom_runtime",
        ],
        &[],
    )?;
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    let name = if cfg!(windows) { "loom.exe" } else { "loom" };
    Ok(target.join("debug").join(name))
}

/// Groups `path:line:col: error[CODE]: message` lines by path, keeping `LINE:COL CODE`.
/// Warnings (named lints) are ignored, as in `.diag` files.
fn parse_report(stderr: &str) -> BTreeMap<String, Vec<String>> {
    let mut reported: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in stderr.lines() {
        let Some((location, rest)) = line.split_once(": error[") else {
            continue;
        };
        let Some(code) = rest.split(']').next() else {
            continue;
        };
        // `location` is `path:line:col`; the path itself may contain `:` (Windows drives).
        let mut parts = location.rsplitn(3, ':');
        let (Some(column), Some(line_number), Some(path)) =
            (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        reported
            .entry(path.replace('\\', "/"))
            .or_default()
            .push(format!("{line_number}:{column} {code}"));
    }
    reported
}

struct Runner<'a> {
    root: &'a Path,
    binary: &'a Path,
}

/// The phase a case declares on its second line (`# phase: N`), if any.
pub fn declared_phase(case: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(case).ok()?;
    let line = text.lines().nth(1)?;
    line.strip_prefix("# phase:")?.trim().parse().ok()
}

impl Runner<'_> {
    fn check_case(&self, relative: &Path, got: &[String]) -> Result<Outcome, String> {
        let case = &self.root.join(relative);
        if declared_phase(case).is_some_and(|phase| phase > CURRENT_PHASE) {
            return Ok(Outcome::Skipped);
        }
        let diag = case.with_extension("diag");
        if diag.exists() {
            let text = std::fs::read_to_string(&diag)
                .map_err(|error| format!("could not read `{}`: {error}", diag.display()))?;
            let expected: Vec<String> = text.lines().map(str::to_owned).collect();
            if !expected
                .iter()
                .all(|line| code_number(line).is_some_and(|n| n <= IMPLEMENTED_UP_TO))
            {
                return Ok(Outcome::Skipped);
            }
            return Ok(if expected == got {
                Outcome::Passed
            } else {
                Outcome::Failed(format!("expected {expected:?}, got {got:?}"))
            });
        }
        let stdout_file = case.with_extension("stdout");
        if stdout_file.exists() {
            if !got.is_empty() {
                return Ok(Outcome::Failed(format!("accept case reported {got:?}")));
            }
            let expected = std::fs::read_to_string(&stdout_file)
                .map_err(|error| format!("could not read `{}`: {error}", stdout_file.display()))?;
            let expected_exit: i32 = std::fs::read_to_string(case.with_extension("exit"))
                .ok()
                .and_then(|t| t.trim().parse().ok())
                .unwrap_or(0);
            let output = Command::new(self.binary)
                .arg("run")
                .arg(relative)
                .current_dir(self.root)
                .output()
                .map_err(|error| format!("could not run `{}`: {error}", self.binary.display()))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let exit = output.status.code().unwrap_or(-1);
            return Ok(if stdout != expected {
                Outcome::Failed(format!("expected output {expected:?}, got {stdout:?}"))
            } else if exit != expected_exit {
                Outcome::Failed(format!(
                    "expected exit status {expected_exit}, got {exit}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            } else {
                Outcome::Passed
            });
        }
        Ok(Outcome::Failed(
            "has neither a `.diag` nor a `.stdout` file".to_owned(),
        ))
    }
}

/// The number of the code in a `LINE:COL EXXXX` line.
fn code_number(line: &str) -> Option<u32> {
    line.rsplit_once(" E")
        .and_then(|(_, digits)| digits.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::{code_number, parse_report};

    #[test]
    fn report_lines_are_grouped_by_path() {
        let stderr = "\
spec/a.loom:2:5: error[E0007]: `!` is not an operator
C:\\x\\b.loom:1:1: error[E0100]: expected x
spec/a.loom:3:1: warning[dangling_doc_comment]: doc comment
spec/a.loom:4:2: error[E0006]: `;` is not used in Loom
loom: some other message
";
        let report = parse_report(stderr);
        assert_eq!(report["spec/a.loom"], ["2:5 E0007", "4:2 E0006"]);
        assert_eq!(report["C:/x/b.loom"], ["1:1 E0100"]);
        assert_eq!(report.len(), 2);
    }

    #[test]
    fn code_numbers() {
        assert_eq!(code_number("3:5 E0012"), Some(12));
        assert_eq!(code_number("3:5 E0301"), Some(301));
        assert_eq!(code_number("garbage"), None);
    }
}
