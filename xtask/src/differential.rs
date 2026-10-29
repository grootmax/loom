//! `cargo xtask differential` (ADR 0021 §7): runs every example and conformance accept
//! case with the interpreter and with the native backend, and requires the same standard
//! output and exit status, no runtime objects leaked, and no construct the backend does
//! not support. With `--aot`, each program is also built with `loom build` and the
//! executable is compared too (under Valgrind with `--valgrind`).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::conformance::{CURRENT_PHASE, build_loom, declared_phase};
use crate::workspace_root;

/// Options of a differential run.
#[derive(Clone, Copy, Default)]
struct Options {
    aot: bool,
    valgrind: bool,
}

/// Runs the differential test.
pub fn run(args: &[String]) -> Result<(), String> {
    let mut options = Options::default();
    for arg in args {
        match arg.as_str() {
            "--aot" => options.aot = true,
            "--valgrind" => {
                options.aot = true;
                options.valgrind = true;
            }
            other => return Err(format!("differential: unknown option `{other}`")),
        }
    }
    let root = workspace_root();
    let binary = build_loom(&root)?;
    let programs = programs(&root)?;
    let scratch = root.join("target").join("differential");
    std::fs::create_dir_all(&scratch)
        .map_err(|e| format!("cannot create {}: {e}", scratch.display()))?;
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let chunks: Vec<&[PathBuf]> = programs
        .chunks(programs.len().div_ceil(threads).max(1))
        .collect();
    let failures: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .iter()
            .enumerate()
            .map(|(worker, chunk)| {
                let (binary, root, scratch) = (&binary, &root, &scratch);
                scope.spawn(move || {
                    chunk
                        .iter()
                        .enumerate()
                        .filter_map(|(i, program)| {
                            let exe = scratch
                                .join(format!("p{worker}_{i}{}", std::env::consts::EXE_SUFFIX));
                            compare(binary, root, program, &exe, options)
                                .err()
                                .map(|e| {
                                    format!(
                                        "{}: {e}",
                                        program.strip_prefix(root).unwrap_or(program).display()
                                    )
                                })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });
    println!(
        "differential: {} programs, {} matched, {} failed{}",
        programs.len(),
        programs.len() - failures.len(),
        failures.len(),
        if options.aot {
            " (JIT and AOT)"
        } else {
            " (JIT)"
        }
    );
    if failures.is_empty() {
        Ok(())
    } else {
        for failure in &failures {
            eprintln!("  {failure}");
        }
        Err("differential test failed".to_owned())
    }
}

/// The examples whose phase has been reached and the conformance accept cases.
fn programs(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for entry in read_dir(&root.join("examples"))? {
        if entry.extension().is_some_and(|e| e == "loom") {
            let text = std::fs::read_to_string(&entry).map_err(|e| e.to_string())?;
            let header = |key: &str| {
                text.lines().find_map(|l| {
                    l.strip_prefix(&format!("# {key}:"))
                        .map(|v| v.trim().to_owned())
                })
            };
            let phase: u32 = header("phase")
                .and_then(|p| p.parse().ok())
                .unwrap_or(u32::MAX);
            if phase <= CURRENT_PHASE && header("run").as_deref().unwrap_or("run") == "run" {
                out.push(entry);
            }
        }
    }
    for section in read_dir(&root.join("spec").join("conformance"))? {
        if !section.is_dir() {
            continue;
        }
        for entry in read_dir(&section)? {
            let program = entry.with_extension("loom");
            if entry.extension().is_some_and(|e| e == "stdout")
                && declared_phase(&program).is_none_or(|phase| phase <= CURRENT_PHASE)
            {
                out.push(program);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn read_dir(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    Ok(entries)
}

fn loom(binary: &Path, root: &Path, args: &[&str], program: &Path) -> Result<Output, String> {
    Command::new(binary)
        .args(args)
        .arg(program)
        .current_dir(root)
        .env("LOOM_LEAK_CHECK", "1")
        .output()
        .map_err(|e| format!("cannot run loom: {e}"))
}

fn compare(
    binary: &Path,
    root: &Path,
    program: &Path,
    exe: &Path,
    options: Options,
) -> Result<(), String> {
    let expected = loom(binary, root, &["run", "--interp"], program)?;
    let native = loom(binary, root, &["run", "--native"], program)?;
    check_same("JIT", &expected, &native)?;
    if options.aot {
        let built = Command::new(binary)
            .arg("build")
            .arg("-o")
            .arg(exe)
            .arg(program)
            .current_dir(root)
            .output()
            .map_err(|e| format!("cannot run loom build: {e}"))?;
        if !built.status.success() {
            return Err(format!(
                "loom build failed: {}",
                String::from_utf8_lossy(&built.stderr).trim()
            ));
        }
        let mut command = if options.valgrind {
            let mut c = Command::new("valgrind");
            c.args([
                "--quiet",
                "--error-exitcode=99",
                "--leak-check=full",
                "--errors-for-leak-kinds=definite",
            ])
            .arg(exe);
            c
        } else {
            Command::new(exe)
        };
        let output = command
            .current_dir(root)
            .env("LOOM_LEAK_CHECK", "1")
            .output()
            .map_err(|e| format!("cannot run the executable: {e}"))?;
        let _ = std::fs::remove_file(exe);
        check_same("AOT", &expected, &output)?;
    }
    Ok(())
}

fn check_same(engine: &str, expected: &Output, native: &Output) -> Result<(), String> {
    let stderr = String::from_utf8_lossy(&native.stderr);
    if stderr.contains("not supported by the native backend") || stderr.contains("internal error") {
        return Err(format!("{engine}: {}", stderr.trim()));
    }
    if stderr.contains("runtime objects leaked") || stderr.contains("ERROR SUMMARY") {
        return Err(format!("{engine}: {}", stderr.trim()));
    }
    if expected.status.code() != native.status.code() {
        return Err(format!(
            "{engine}: exit status {:?}, interpreter {:?}: {}",
            native.status.code(),
            expected.status.code(),
            stderr.trim()
        ));
    }
    let expected_stdout = String::from_utf8_lossy(&expected.stdout).replace("\r\n", "\n");
    let native_stdout = String::from_utf8_lossy(&native.stdout).replace("\r\n", "\n");
    if expected_stdout != native_stdout {
        return Err(format!(
            "{engine}: output differs\n--- interpreter\n{expected_stdout}--- native\n{native_stdout}"
        ));
    }
    Ok(())
}
