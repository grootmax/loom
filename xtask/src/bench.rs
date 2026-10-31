//! `cargo xtask bench`: runs the benchmark suite in `benches/`.
//!
//! Builds `loom` in release mode, compiles each `.loom` file in `benches/` to a native
//! executable using `loom build`, runs it multiple times, and prints a Markdown table
//! of wall-clock timing statistics. Optionally times `loom run --interp` with a timeout.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::workspace_root;

/// Options for `cargo xtask bench`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Options {
    runs: usize,
    interp: bool,
    filter: Option<String>,
}

enum InterpResult {
    Completed { duration: Duration, stdout: String },
    Timeout,
}

struct BenchResult {
    program_name: String,
    native_median: Duration,
    native_min: Duration,
    interp: Option<InterpResult>,
}

/// Runs the benchmark suite.
///
/// # Errors
/// Returns an error if building `loom`, compiling a benchmark, executing a benchmark,
/// or verifying checksums fails.
pub fn run(args: &[String]) -> Result<(), String> {
    let options = parse_args(args)?;
    let root = workspace_root();
    let binary = build_loom_release(&root)?;

    let bench_dir = root.join("benches");
    let programs = collect_benchmarks(&bench_dir, options.filter.as_deref())?;

    if programs.is_empty() {
        println!("bench: no matching benchmark programs found");
        return Ok(());
    }

    let scratch = root.join("target").join("bench");
    std::fs::create_dir_all(&scratch).map_err(|e| {
        format!(
            "cannot create scratch directory `{}`: {e}",
            scratch.display()
        )
    })?;

    let mut results = Vec::new();

    for (index, program) in programs.iter().enumerate() {
        let result = bench_single_program(program, &binary, &root, &scratch, index, &options)?;
        results.push(result);
    }

    print_markdown_table(&results);

    Ok(())
}

fn bench_single_program(
    program: &Path,
    binary: &Path,
    root: &Path,
    scratch: &Path,
    index: usize,
    options: &Options,
) -> Result<BenchResult, String> {
    let name = program.file_name().map_or_else(
        || "unknown".to_string(),
        |n| n.to_string_lossy().to_string(),
    );

    let exe_path = scratch.join(format!("bench_{index}{}", std::env::consts::EXE_SUFFIX));

    let build_output = Command::new(binary)
        .arg("build")
        .arg("-o")
        .arg(&exe_path)
        .arg(program)
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run `loom build` on `{name}`: {e}"))?;

    if !build_output.status.success() {
        return Err(format!(
            "loom build failed for `{name}`: {}",
            String::from_utf8_lossy(&build_output.stderr).trim()
        ));
    }

    let mut runs_durations = Vec::with_capacity(options.runs);
    let mut native_stdout: Option<String> = None;

    for r in 0..options.runs {
        let start = Instant::now();
        let output = Command::new(&exe_path)
            .current_dir(root)
            .output()
            .map_err(|e| format!("cannot run native executable `{name}` (run {r}): {e}"))?;
        let elapsed = start.elapsed();

        if !output.status.success() {
            let _ = std::fs::remove_file(&exe_path);
            return Err(format!(
                "native executable `{name}` failed with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
        if let Some(ref first_stdout) = native_stdout {
            if first_stdout != &stdout_str {
                let _ = std::fs::remove_file(&exe_path);
                return Err(format!(
                    "native executable `{name}` produced inconsistent stdout across runs"
                ));
            }
        } else {
            native_stdout = Some(stdout_str);
        }

        runs_durations.push(elapsed);
    }

    let _ = std::fs::remove_file(&exe_path);

    runs_durations.sort();
    let native_min = runs_durations[0];
    let native_median = if runs_durations.len() % 2 == 1 {
        runs_durations[runs_durations.len() / 2]
    } else {
        let mid = runs_durations.len() / 2;
        (runs_durations[mid - 1] + runs_durations[mid]) / 2
    };

    let first_native_stdout = native_stdout.unwrap_or_default();

    let interp_res = if options.interp {
        let interp_outcome = run_interp(binary, root, program, Duration::from_secs(60))?;
        if let InterpResult::Completed { ref stdout, .. } = interp_outcome
            && stdout.trim() != first_native_stdout.trim()
        {
            return Err(format!(
                "checksum mismatch for `{name}`:\n  native: {:?}\n  interp: {:?}",
                first_native_stdout.trim(),
                stdout.trim()
            ));
        }
        Some(interp_outcome)
    } else {
        None
    };

    Ok(BenchResult {
        program_name: name,
        native_median,
        native_min,
        interp: interp_res,
    })
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut runs = 5;
    let mut interp = false;
    let mut filter = None;

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--interp" => interp = true,
            "--runs" => {
                let val = iter
                    .next()
                    .ok_or_else(|| "--runs requires a number argument".to_string())?;
                runs = val
                    .parse::<usize>()
                    .map_err(|_| format!("invalid value for --runs: `{val}`"))?;
                if runs == 0 {
                    return Err("--runs must be at least 1".to_string());
                }
            }
            s if s.starts_with("--runs=") => {
                let val = &s["--runs=".len()..];
                runs = val
                    .parse::<usize>()
                    .map_err(|_| format!("invalid value for --runs: `{val}`"))?;
                if runs == 0 {
                    return Err("--runs must be at least 1".to_string());
                }
            }
            "--filter" => {
                let val = iter
                    .next()
                    .ok_or_else(|| "--filter requires a string argument".to_string())?;
                filter = Some(val.clone());
            }
            s if s.starts_with("--filter=") => {
                let val = &s["--filter=".len()..];
                filter = Some(val.to_owned());
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }

    Ok(Options {
        runs,
        interp,
        filter,
    })
}

fn build_loom_release(root: &Path) -> Result<PathBuf, String> {
    crate::run(
        root,
        "cargo",
        &[
            "build",
            "--release",
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
    Ok(target.join("release").join(name))
}

fn collect_benchmarks(directory: &Path, filter: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|e| format!("cannot read directory `{}`: {e}", directory.display()))?;

    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|ext| ext == "loom") {
            if let Some(f) = filter {
                let filename = path.file_name().map_or("", |n| n.to_str().unwrap_or(""));
                if !filename.contains(f) {
                    continue;
                }
            }
            files.push(path);
        }
    }

    files.sort();
    Ok(files)
}

fn run_interp(
    binary: &Path,
    root: &Path,
    program: &Path,
    timeout: Duration,
) -> Result<InterpResult, String> {
    let mut child = Command::new(binary)
        .arg("run")
        .arg("--interp")
        .arg(program)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot spawn interpreter for `{}`: {e}", program.display()))?;

    let start = Instant::now();

    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();

            if let Some(mut out) = child.stdout.take() {
                let _ = std::io::Read::read_to_end(&mut out, &mut stdout);
            }
            if let Some(mut err) = child.stderr.take() {
                let _ = std::io::Read::read_to_end(&mut err, &mut stderr);
            }

            let duration = start.elapsed();

            if !status.success() {
                return Err(format!(
                    "interpreter failed on `{}` with status {status}: {}",
                    program.display(),
                    String::from_utf8_lossy(&stderr).trim()
                ));
            }

            return Ok(InterpResult::Completed {
                duration,
                stdout: String::from_utf8_lossy(&stdout).to_string(),
            });
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(InterpResult::Timeout);
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

fn print_markdown_table(results: &[BenchResult]) {
    println!("| Program | Native Median | Native Min | Interpreter | Ratio |");
    println!("| :--- | :--- | :--- | :--- | :--- |");

    for r in results {
        let native_med_ms = r.native_median.as_secs_f64() * 1000.0;
        let native_min_ms = r.native_min.as_secs_f64() * 1000.0;

        let (interp_str, ratio_str) = match &r.interp {
            Some(InterpResult::Completed { duration, .. }) => {
                let interp_ms = duration.as_secs_f64() * 1000.0;
                let ratio = if native_med_ms > 0.0 {
                    interp_ms / native_med_ms
                } else {
                    0.0
                };
                (format!("{interp_ms:.1} ms"), format!("{ratio:.1}x"))
            }
            Some(InterpResult::Timeout) => ("timeout".to_string(), "-".to_string()),
            None => ("-".to_string(), "-".to_string()),
        };

        println!(
            "| {} | {:.1} ms | {:.1} ms | {} | {} |",
            r.program_name, native_med_ms, native_min_ms, interp_str, ratio_str
        );
    }
}
