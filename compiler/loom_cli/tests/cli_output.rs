//! Integration tests verifying loom CLI diagnostic output formats, rendering, colors, and summaries.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(content: &[u8], extension: &str) -> Self {
        let mut path = std::env::temp_dir();
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!(
            "loom_test_{}_{}.{}",
            std::process::id(),
            id,
            extension
        ));
        std::fs::write(&path, content).expect("failed to write temp file");
        Self { path }
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn loom_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_loom"))
}

#[test]
fn test_short_format_lexical_error() {
    let file = TempFile::new(b"# \xff\ndef main\nend\n", "loom");
    let output = loom_cmd()
        .arg("check")
        .arg("--message-format=short")
        .arg(&file.path)
        .output()
        .expect("failed to run loom command");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected_short = format!(
        "{}:1:3: error[E0001]: source file is not valid UTF-8",
        file.path.display()
    );
    assert!(
        stderr.contains(&expected_short),
        "expected short output `{expected_short}`, got stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("due to"),
        "short format should not contain summary line"
    );
}

#[test]
fn test_short_format_type_error() {
    let code = b"def main\n  x = if true then 10 else \"hello\" end\nend\n";
    let file = TempFile::new(code, "loom");
    let output = loom_cmd()
        .arg("check")
        .arg("--message-format=short")
        .arg(&file.path)
        .output()
        .expect("failed to run loom command");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected_short = format!(
        "{}:2:28: error[E0300]: mismatched types: expected `{{integer}}`, found `String`",
        file.path.display()
    );
    assert!(
        stderr.contains(&expected_short),
        "expected short output `{expected_short}`, got stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("due to"),
        "short format should not contain summary line"
    );
}

#[test]
fn test_short_format_ownership_error() {
    let code = b"def main\n  x = \"hello\"\n  a = x\n  b = x\nend\n";
    let file = TempFile::new(code, "loom");
    let output = loom_cmd()
        .arg("check")
        .arg("--message-format=short")
        .arg(&file.path)
        .output()
        .expect("failed to run loom command");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected_short = format!(
        "{}:4:7: error[E0500]: use of a moved value",
        file.path.display()
    );
    assert!(
        stderr.contains(&expected_short),
        "expected short output `{expected_short}`, got stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("due to"),
        "short format should not contain summary line"
    );
}

#[test]
fn test_default_format_snippet_and_caret() {
    let code = b"def main\n  x = \"hello\"\n  a = x\n  b = x\nend\n";
    let file = TempFile::new(code, "loom");
    let output = loom_cmd()
        .arg("check")
        .arg("--color=never")
        .arg(&file.path)
        .output()
        .expect("failed to run loom command");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Verify snippet line and caret
    assert!(
        stderr.contains("4 |   b = x"),
        "expected snippet line in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("^ used here after the move"),
        "expected caret line in stderr:\n{stderr}"
    );

    // Verify summary line
    let expected_summary = format!(
        "error: could not check `{}` due to 1 previous error",
        file.path.display()
    );
    assert!(
        stderr.contains(&expected_summary),
        "expected summary line `{expected_summary}` in stderr:\n{stderr}"
    );
}

#[test]
fn test_no_color_disables_escape_codes() {
    let code = b"def main\n  x = \"hello\"\n  a = x\n  b = x\nend\n";
    let file = TempFile::new(code, "loom");

    // Pass NO_COLOR env var and --color=auto
    let output = loom_cmd()
        .env("NO_COLOR", "1")
        .arg("check")
        .arg("--color=auto")
        .arg(&file.path)
        .output()
        .expect("failed to run loom command");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stderr.contains('\x1b'),
        "NO_COLOR should disable ANSI escape codes in stderr:\n{stderr}"
    );

    // Also test --color=never without NO_COLOR
    let output_never = loom_cmd()
        .env_remove("NO_COLOR")
        .arg("check")
        .arg("--color=never")
        .arg(&file.path)
        .output()
        .expect("failed to run loom command");

    let stderr_never = String::from_utf8_lossy(&output_never.stderr);
    assert!(
        !stderr_never.contains('\x1b'),
        "--color=never should disable ANSI escape codes in stderr:\n{stderr_never}"
    );
}

#[test]
fn test_summary_line_counts_errors_and_warnings() {
    let code = b"def main\n  x = if true then 10 else \"hello\" end\n  ## dangling doc\nend\n";
    let file = TempFile::new(code, "loom");
    let output = loom_cmd()
        .arg("check")
        .arg("--color=never")
        .arg(&file.path)
        .output()
        .expect("failed to run loom command");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);

    let expected_summary = format!(
        "error: could not check `{}` due to 1 previous error; 1 warning emitted",
        file.path.display()
    );
    assert!(
        stderr.contains(&expected_summary),
        "expected summary line `{expected_summary}` in stderr:\n{stderr}"
    );
}
