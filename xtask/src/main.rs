//! Repository automation for Loom, run as `cargo xtask <command>`.
//!
//! This crate uses only the Rust standard library, so it builds anywhere the
//! pinned toolchain does. See `CONTRIBUTING.md` for when to run each command.

mod conformance;
mod core_tests;
mod differential;
mod examples;
mod gen_unicode;
mod spec_check;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const HELP: &str = "\
cargo xtask <command>

COMMANDS:
  ci          Run every check CI runs: fmt, clippy, test, doc, conformance, examples, core-tests, deny
  conformance Run spec/conformance against the implementation
  core-tests  Run loom test on every file in std/core/tests
  differential Compare the native backend with the interpreter [--aot] [--valgrind]
  examples    Run the examples whose phase has been reached
  gen-unicode Generate Unicode tables from UCD 17.0.0
  spec-check  Check that grammar rules cite existing spec sections
  help        Print this help
";

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    let result = match command.as_deref() {
        Some("ci") => ci(),
        Some("conformance") => conformance::run(),
        Some("core-tests") => core_tests::run(),
        Some("differential") => differential::run(&std::env::args().skip(2).collect::<Vec<_>>()),
        Some("examples") => examples::run(),
        Some("gen-unicode") => match std::env::args().nth(2) {
            Some(ucd_dir) => gen_unicode::run(Path::new(&ucd_dir)),
            None => Err("usage: cargo xtask gen-unicode <UCD_DIR>".to_string()),
        },
        Some("spec-check") => spec_check::run(),
        None | Some("help" | "-h" | "--help") => {
            print!("{HELP}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{HELP}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Runs every check that CI runs, stopping at the first failure.
fn ci() -> Result<(), String> {
    spec_check::run()?;
    let root = workspace_root();
    run(&root, "cargo", &["fmt", "--all", "--check"], &[])?;
    run(
        &root,
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
        &[],
    )?;
    run(&root, "cargo", &["test", "--workspace", "--locked"], &[])?;
    run(
        &root,
        "cargo",
        &["doc", "--workspace", "--no-deps", "--locked"],
        &[("RUSTDOCFLAGS", "-D warnings")],
    )?;
    conformance::run()?;
    examples::run()?;
    core_tests::run()?;
    differential::run(&[])?;
    if has_cargo_subcommand("deny") {
        run(&root, "cargo", &["deny", "--locked", "check"], &[])?;
    } else if std::env::var_os("CI").is_some() {
        return Err("cargo-deny is required in CI but is not installed".to_owned());
    } else {
        eprintln!();
        eprintln!("xtask: WARNING: cargo-deny is not installed; skipping `cargo deny check`.");
        eprintln!("xtask: CI will still run it. Install with `cargo install cargo-deny --locked`.");
    }
    eprintln!("xtask: ci passed");
    Ok(())
}

/// Runs a command in `dir`, echoing it first, and fails if it exits unsuccessfully.
fn run(dir: &Path, program: &str, args: &[&str], envs: &[(&str, &str)]) -> Result<(), String> {
    eprintln!("$ {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .envs(envs.iter().copied())
        .current_dir(dir)
        .status()
        .map_err(|error| format!("could not start `{program}`: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "`{program} {}` failed with {status}",
            args.join(" ")
        ))
    }
}

fn has_cargo_subcommand(name: &str) -> bool {
    Command::new("cargo")
        .args([name, "--version"])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The repository root: the parent of this crate's manifest directory.
pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one directory below the workspace root")
        .to_path_buf()
}
