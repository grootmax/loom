//! `cargo xtask race`: verifies concurrency under the deterministic scheduler.
//!
//! Checks that:
//! 1. Correct concurrent programs (phase <= 4 examples and concurrency-using conformance
//!    accept cases, plus fixed versions of race tests) produce expected output for every
//!    seed in both interpreter and native modes.
//! 2. Racy programs produce outputs within their expected `.racy` sets, expose at least
//!    two distinct outputs across seeds, and reproduce byte-identically when re-running
//!    the same seed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::conformance::{CURRENT_PHASE, build_loom, declared_phase};
use crate::workspace_root;

/// Options for the race test runner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Number of deterministic seeds to run (`0..seeds`), defaulting to 16.
    pub seeds: usize,
    /// Whether to also run checks through `loom run --native`.
    pub native: bool,
    /// Substring filter for matching program paths.
    pub filter: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            seeds: 16,
            native: false,
            filter: None,
        }
    }
}

/// Parses command-line arguments for `cargo xtask race`.
///
/// # Errors
/// Returns an error if unknown options or invalid values are encountered.
pub fn parse_args(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut iterator = arguments.iter();

    while let Some(argument) = iterator.next() {
        match argument.as_str() {
            "--seeds" => {
                let value = iterator
                    .next()
                    .ok_or_else(|| "--seeds requires an integer argument".to_owned())?;
                options.seeds = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid value for --seeds: `{value}`"))?;
                if options.seeds == 0 {
                    return Err("--seeds must be at least 1".to_owned());
                }
            }
            s if s.starts_with("--seeds=") => {
                let value = &s["--seeds=".len()..];
                options.seeds = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid value for --seeds: `{value}`"))?;
                if options.seeds == 0 {
                    return Err("--seeds must be at least 1".to_owned());
                }
            }
            "--native" => {
                options.native = true;
            }
            "--filter" => {
                let value = iterator
                    .next()
                    .ok_or_else(|| "--filter requires a string argument".to_owned())?;
                options.filter = Some(value.clone());
            }
            s if s.starts_with("--filter=") => {
                let value = &s["--filter=".len()..];
                options.filter = Some(value.to_owned());
            }
            other => return Err(format!("race: unknown option `{other}`")),
        }
    }

    Ok(options)
}

/// Parses the contents of a `.racy` expectation file into a set of allowed outputs.
///
/// Each non-empty line represents one possible expected output. Blank lines and lines
/// starting with `#` are ignored. A line equal to `*` is forbidden.
///
/// # Errors
/// Returns an error if the file contains `*` or has no valid entries.
pub fn parse_racy(content: &str) -> Result<BTreeSet<String>, String> {
    let mut outputs = BTreeSet::new();
    for (line_index, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "*" {
            return Err(format!(
                "line {}: wildcard '*' is not allowed in .racy files",
                line_index + 1
            ));
        }
        outputs.insert(line.to_owned());
    }
    if outputs.is_empty() {
        return Err(".racy file must contain at least one expected output".to_owned());
    }
    Ok(outputs)
}

/// A sweep program with deterministic output expectation.
#[derive(Clone, Debug)]
struct SweepProgram {
    path: PathBuf,
    mode: String,
    arguments: Vec<String>,
    expected_stdout: String,
    expected_exit: i32,
}

/// A racy program with set-based expectations.
#[derive(Clone, Debug)]
struct RacyProgram {
    path: PathBuf,
    allowed_outputs: BTreeSet<String>,
}

/// An item to be verified by the race test runner.
enum TestItem<'a> {
    Sweep(&'a SweepProgram),
    Racy(&'a RacyProgram),
}

/// Runs the deterministic scheduler race tests.
///
/// # Errors
/// Returns an error if building `loom` fails, test files cannot be read, or any test fails.
pub fn run(arguments: &[String]) -> Result<(), String> {
    let options = parse_args(arguments)?;
    let root = workspace_root();
    let binary = build_loom(&root)?;

    let sweep = collect_sweep_programs(&root, options.filter.as_deref())?;
    let racy = collect_racy_programs(&root, options.filter.as_deref())?;

    let worker_count = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);

    let mut items: Vec<TestItem> = sweep.iter().map(TestItem::Sweep).collect();
    items.extend(racy.iter().map(TestItem::Racy));

    let next_index = std::sync::atomic::AtomicUsize::new(0);

    let failures: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..worker_count)
            .map(|_| {
                let (binary_ref, root_ref, options_ref, items_ref, next_index_ref) =
                    (&binary, &root, &options, &items, &next_index);
                scope.spawn(move || {
                    let mut local_failures = Vec::new();
                    loop {
                        let index =
                            next_index_ref.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if index >= items_ref.len() {
                            break;
                        }
                        match items_ref[index] {
                            TestItem::Sweep(program) => {
                                if let Err(error) =
                                    check_sweep_program(binary_ref, root_ref, program, options_ref)
                                {
                                    local_failures.push(error);
                                }
                            }
                            TestItem::Racy(program) => {
                                if let Err(error) =
                                    check_racy_program(binary_ref, root_ref, program, options_ref)
                                {
                                    local_failures.push(error);
                                }
                            }
                        }
                    }
                    local_failures
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap_or_default())
            .collect()
    });

    let total_failures = failures.len();
    println!(
        "race: {} sweep programs, {} racy programs, {} seeds{}",
        sweep.len(),
        racy.len(),
        options.seeds,
        if options.native {
            " (interpreter and native)"
        } else {
            " (interpreter only)"
        }
    );

    if total_failures == 0 {
        Ok(())
    } else {
        for failure in &failures {
            eprintln!("  {failure}");
        }
        Err(format!("race tests failed ({total_failures} failure(s))"))
    }
}

/// Checks that a sweep program produces its expected stdout and exit code under every seed.
fn check_sweep_program(
    binary: &Path,
    root: &Path,
    program: &SweepProgram,
    options: &Options,
) -> Result<(), String> {
    let relative = program.path.strip_prefix(root).unwrap_or(&program.path);

    for seed in 0..options.seeds {
        // Interpreter run
        let (command, engine_flag) = match program.mode.as_str() {
            "test" => ("test", None),
            "check" => ("check", None),
            _ => ("run", Some("--interp")),
        };

        let output = execute_loom(
            binary,
            root,
            command,
            engine_flag,
            &program.path,
            &program.arguments,
            seed,
        )?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        if program.mode != "check" && stdout != program.expected_stdout {
            return Err(format!(
                "{}: output differs at seed {seed}\n--- expected\n{}--- got\n{stdout}\nreproduce with: {}",
                relative.display(),
                program.expected_stdout,
                reproduce_command(command, engine_flag, relative, &program.arguments, seed)
            ));
        }

        if exit_code != program.expected_exit {
            return Err(format!(
                "{}: exit status {exit_code} (expected {}) at seed {seed}: {}\nreproduce with: {}",
                relative.display(),
                program.expected_exit,
                stderr.trim(),
                reproduce_command(command, engine_flag, relative, &program.arguments, seed)
            ));
        }

        // Native run if requested and mode is "run"
        if options.native && program.mode == "run" {
            let native_output = execute_loom(
                binary,
                root,
                "run",
                Some("--native"),
                &program.path,
                &program.arguments,
                seed,
            )?;

            let native_stdout = String::from_utf8_lossy(&native_output.stdout);
            let native_stderr = String::from_utf8_lossy(&native_output.stderr);
            let native_exit = native_output.status.code().unwrap_or(-1);

            if native_stdout != program.expected_stdout {
                return Err(format!(
                    "{}: native output differs at seed {seed}\n--- expected\n{}--- got\n{native_stdout}\nreproduce with: {}",
                    relative.display(),
                    program.expected_stdout,
                    reproduce_command("run", Some("--native"), relative, &program.arguments, seed)
                ));
            }

            if native_exit != program.expected_exit {
                return Err(format!(
                    "{}: native exit status {native_exit} (expected {}) at seed {seed}: {}\nreproduce with: {}",
                    relative.display(),
                    program.expected_exit,
                    native_stderr.trim(),
                    reproduce_command("run", Some("--native"), relative, &program.arguments, seed)
                ));
            }
        }
    }

    Ok(())
}

/// Checks that a racy program satisfies all race requirements:
/// 1. Outputs are in the allowed set for every seed.
/// 2. Re-running the same seed yields byte-identical output.
/// 3. At least two distinct outputs occur across seeds.
/// 4. With `--native`, native code matches interpreter output for each seed.
fn check_racy_program(
    binary: &Path,
    root: &Path,
    program: &RacyProgram,
    options: &Options,
) -> Result<(), String> {
    let relative = program.path.strip_prefix(root).unwrap_or(&program.path);
    let mut distinct_outputs = BTreeSet::new();

    for seed in 0..options.seeds {
        // First interpreter run
        let first_run = execute_loom(
            binary,
            root,
            "run",
            Some("--interp"),
            &program.path,
            &[],
            seed,
        )?;

        let stdout_str = String::from_utf8_lossy(&first_run.stdout);
        let stderr_str = String::from_utf8_lossy(&first_run.stderr);
        let trimmed_output = stdout_str.trim().to_owned();

        if !first_run.status.success() {
            return Err(format!(
                "{}: failed with exit code {:?} at seed {seed}: {}\nreproduce with: {}",
                relative.display(),
                first_run.status.code(),
                stderr_str.trim(),
                reproduce_command("run", Some("--interp"), relative, &[], seed)
            ));
        }

        if !program.allowed_outputs.contains(&trimmed_output) {
            return Err(format!(
                "{}: output `{trimmed_output}` at seed {seed} not in allowed set {:?}\nreproduce with: {}",
                relative.display(),
                program.allowed_outputs,
                reproduce_command("run", Some("--interp"), relative, &[], seed)
            ));
        }

        // Second interpreter run to verify determinism/reproducibility
        let second_run = execute_loom(
            binary,
            root,
            "run",
            Some("--interp"),
            &program.path,
            &[],
            seed,
        )?;

        if first_run.stdout != second_run.stdout || first_run.status != second_run.status {
            return Err(format!(
                "{}: non-deterministic execution detected on repeated run with seed {seed}\nreproduce with: {}",
                relative.display(),
                reproduce_command("run", Some("--interp"), relative, &[], seed)
            ));
        }

        // Native run if requested
        if options.native {
            let native_run = execute_loom(
                binary,
                root,
                "run",
                Some("--native"),
                &program.path,
                &[],
                seed,
            )?;

            if native_run.stdout != first_run.stdout || native_run.status != first_run.status {
                let native_stdout = String::from_utf8_lossy(&native_run.stdout);
                return Err(format!(
                    "{}: native output differs from interpreter at seed {seed}\n--- interpreter\n{stdout_str}--- native\n{native_stdout}\nreproduce with: {}",
                    relative.display(),
                    reproduce_command("run", Some("--native"), relative, &[], seed)
                ));
            }
        }

        distinct_outputs.insert(trimmed_output);
    }

    if distinct_outputs.len() < 2 {
        return Err(format!(
            "{}: scheduler did not expose race across {} seeds (only saw output(s) {:?}); expected at least 2 distinct outputs",
            relative.display(),
            options.seeds,
            distinct_outputs
        ));
    }

    Ok(())
}

/// Executes `loom` with deterministic scheduler seed and leak checking.
fn execute_loom(
    binary: &Path,
    root: &Path,
    subcommand: &str,
    engine_flag: Option<&str>,
    program: &Path,
    arguments: &[String],
    seed: usize,
) -> Result<Output, String> {
    let mut command = Command::new(binary);
    command.arg(subcommand);
    if let Some(flag) = engine_flag {
        command.arg(flag);
    }
    command.arg(program);
    if !arguments.is_empty() {
        command.arg("--");
        command.args(arguments);
    }
    command.current_dir(root);
    command.env("LOOM_SCHED", format!("deterministic:{seed}"));
    command.env("LOOM_LEAK_CHECK", "1");
    command
        .output()
        .map_err(|error| format!("failed to execute `{}`: {error}", binary.display()))
}

/// Formats the command line that reproduces a failure.
fn reproduce_command(
    subcommand: &str,
    engine_flag: Option<&str>,
    program: &Path,
    arguments: &[String],
    seed: usize,
) -> String {
    let mut parts = vec![
        format!("LOOM_SCHED=deterministic:{seed}"),
        "loom".to_owned(),
        subcommand.to_owned(),
    ];
    if let Some(flag) = engine_flag {
        parts.push(flag.to_owned());
    }
    parts.push(program.display().to_string().replace('\\', "/"));
    if !arguments.is_empty() {
        parts.push("--".to_owned());
        parts.extend(arguments.iter().cloned());
    }
    parts.join(" ")
}

/// Collects all sweep programs (examples <= phase 4, concurrency conformance accept cases, and fixed race tests).
fn collect_sweep_programs(root: &Path, filter: Option<&str>) -> Result<Vec<SweepProgram>, String> {
    let mut programs = Vec::new();

    // 1. Examples with phase <= CURRENT_PHASE
    let examples_dir = root.join("examples");
    for entry in read_directory(&examples_dir)? {
        if entry
            .extension()
            .is_some_and(|extension| extension == "loom")
        {
            let text = std::fs::read_to_string(&entry).map_err(|e| e.to_string())?;
            let phase = find_header(&text, "phase")
                .and_then(|p| p.parse::<u32>().ok())
                .unwrap_or(u32::MAX);
            if phase <= CURRENT_PHASE {
                let mode = find_header(&text, "run").unwrap_or_else(|| "run".to_owned());
                let arguments = find_header(&text, "args")
                    .map(|args_str| args_str.split_whitespace().map(str::to_owned).collect())
                    .unwrap_or_default();

                let expected_stdout = if mode == "check" {
                    String::new()
                } else {
                    let stdout_path = entry.with_extension("stdout");
                    std::fs::read_to_string(&stdout_path)
                        .map_err(|e| format!("cannot read `{}`: {e}", stdout_path.display()))?
                };

                let exit_path = entry.with_extension("exit");
                let expected_exit: i32 = std::fs::read_to_string(&exit_path)
                    .ok()
                    .and_then(|t| t.trim().parse().ok())
                    .unwrap_or(0);

                programs.push(SweepProgram {
                    path: entry,
                    mode,
                    arguments,
                    expected_stdout,
                    expected_exit,
                });
            }
        }
    }

    // 2. Conformance accept cases that use scope, spawn, Chan, or select
    let conformance_dir = root.join("spec").join("conformance");
    let mut conformance_cases = Vec::new();
    collect_loom_files_recursive(&conformance_dir, &mut conformance_cases)?;

    for case in conformance_cases {
        let stdout_file = case.with_extension("stdout");
        if stdout_file.exists() && declared_phase(&case).is_none_or(|phase| phase <= CURRENT_PHASE)
        {
            let text = std::fs::read_to_string(&case).map_err(|e| e.to_string())?;
            if uses_concurrency_primitive(&text) {
                let expected_stdout = std::fs::read_to_string(&stdout_file)
                    .map_err(|e| format!("cannot read `{}`: {e}", stdout_file.display()))?;
                let exit_path = case.with_extension("exit");
                let expected_exit: i32 = std::fs::read_to_string(&exit_path)
                    .ok()
                    .and_then(|t| t.trim().parse().ok())
                    .unwrap_or(0);

                programs.push(SweepProgram {
                    path: case,
                    mode: "run".to_owned(),
                    arguments: Vec::new(),
                    expected_stdout,
                    expected_exit,
                });
            }
        }
    }

    // 3. Fixed race tests under tests/race/
    let race_dir = root.join("tests").join("race");
    if race_dir.exists() {
        for entry in read_directory(&race_dir)? {
            if entry
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with("_fixed.loom"))
            {
                let stdout_file = entry.with_extension("stdout");
                let expected_stdout = std::fs::read_to_string(&stdout_file)
                    .map_err(|e| format!("cannot read `{}`: {e}", stdout_file.display()))?;
                let exit_path = entry.with_extension("exit");
                let expected_exit: i32 = std::fs::read_to_string(&exit_path)
                    .ok()
                    .and_then(|t| t.trim().parse().ok())
                    .unwrap_or(0);

                programs.push(SweepProgram {
                    path: entry,
                    mode: "run".to_owned(),
                    arguments: Vec::new(),
                    expected_stdout,
                    expected_exit,
                });
            }
        }
    }

    programs.retain(|program| {
        filter.is_none_or(|needle| {
            program.path.display().to_string().contains(needle)
                || program
                    .path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains(needle))
        })
    });

    programs.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(programs)
}

/// Collects all racy programs under `tests/race/`.
fn collect_racy_programs(root: &Path, filter: Option<&str>) -> Result<Vec<RacyProgram>, String> {
    let mut programs = Vec::new();
    let race_dir = root.join("tests").join("race");
    if !race_dir.exists() {
        return Ok(programs);
    }

    for entry in read_directory(&race_dir)? {
        if entry
            .extension()
            .is_some_and(|extension| extension == "loom")
        {
            let filename = entry
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_default();
            if filename.ends_with("_fixed.loom") {
                continue;
            }

            let racy_file = entry.with_extension("racy");
            if racy_file.exists() {
                let content = std::fs::read_to_string(&racy_file)
                    .map_err(|e| format!("cannot read `{}`: {e}", racy_file.display()))?;
                let allowed_outputs =
                    parse_racy(&content).map_err(|e| format!("{}: {e}", racy_file.display()))?;

                programs.push(RacyProgram {
                    path: entry,
                    allowed_outputs,
                });
            }
        }
    }

    programs.retain(|program| {
        filter.is_none_or(|needle| {
            program.path.display().to_string().contains(needle)
                || program
                    .path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains(needle))
        })
    });

    programs.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(programs)
}

/// Checks if a program text uses any of `scope`, `spawn`, `Chan`, or `select`.
fn uses_concurrency_primitive(text: &str) -> bool {
    let keywords = ["scope", "spawn", "Chan", "select"];
    text.split(|character: char| !character.is_alphanumeric() && character != '_')
        .any(|token| keywords.contains(&token))
}

fn find_header(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.strip_prefix(&format!("# {key}:"))
            .map(|value| value.trim().to_owned())
    })
}

fn read_directory(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)
        .map_err(|error| format!("cannot read `{}`: {error}", directory.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    Ok(entries)
}

fn collect_loom_files_recursive(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("cannot read `{}`: {error}", directory.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            collect_loom_files_recursive(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "loom") {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_defaults() {
        let options = parse_args(&[]).expect("empty arguments should succeed");
        assert_eq!(
            options,
            Options {
                seeds: 16,
                native: false,
                filter: None,
            }
        );
    }

    #[test]
    fn parse_args_flags() {
        let options = parse_args(&[
            "--seeds".to_owned(),
            "32".to_owned(),
            "--native".to_owned(),
            "--filter".to_owned(),
            "channel".to_owned(),
        ])
        .expect("valid flags should succeed");

        assert_eq!(
            options,
            Options {
                seeds: 32,
                native: true,
                filter: Some("channel".to_owned()),
            }
        );

        let options_equals = parse_args(&["--seeds=8".to_owned(), "--filter=select".to_owned()])
            .expect("equals syntax should succeed");

        assert_eq!(
            options_equals,
            Options {
                seeds: 8,
                native: false,
                filter: Some("select".to_owned()),
            }
        );
    }

    #[test]
    fn parse_args_errors() {
        assert!(parse_args(&["--unknown".to_owned()]).is_err());
        assert!(parse_args(&["--seeds".to_owned()]).is_err());
        assert!(parse_args(&["--seeds".to_owned(), "abc".to_owned()]).is_err());
        assert!(parse_args(&["--seeds".to_owned(), "0".to_owned()]).is_err());
        assert!(parse_args(&["--filter".to_owned()]).is_err());
    }

    #[test]
    fn parse_racy_valid() {
        let content = "\
# Possible outputs for lost_update
2
3
4
";
        let outputs = parse_racy(content).expect("valid content should parse");
        let expected: BTreeSet<String> = ["2".to_owned(), "3".to_owned(), "4".to_owned()]
            .into_iter()
            .collect();
        assert_eq!(outputs, expected);
    }

    #[test]
    fn parse_racy_rejects_wildcard() {
        let content = "\
first
*
second
";
        let error = parse_racy(content).expect_err("wildcard should be rejected");
        assert!(error.contains("wildcard '*' is not allowed"));
    }

    #[test]
    fn parse_racy_rejects_empty() {
        assert!(parse_racy("").is_err());
        assert!(parse_racy("# only comments\n\n").is_err());
    }

    #[test]
    fn uses_concurrency_primitive_detection() {
        assert!(uses_concurrency_primitive("scope do |s| end"));
        assert!(uses_concurrency_primitive("s.spawn { }"));
        assert!(uses_concurrency_primitive("Chan[Int].new"));
        assert!(uses_concurrency_primitive(
            "select\nwhen Some(x) = ch.recv\nend"
        ));
        assert!(!uses_concurrency_primitive("def select_point(p1, p2) end"));
        assert!(!uses_concurrency_primitive("def telescope() end"));
    }
}
