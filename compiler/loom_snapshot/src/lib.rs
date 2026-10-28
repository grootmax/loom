//! In-house snapshot testing helper for Loom.
//!
//! Provides file-based snapshot assertions with automatic header formatting,
//! line-by-line diff generation, and support for blessing new or updated snapshots.
//!
//! # Examples
//!
//! ```
//! use loom_snapshot::assert_snapshot_at;
//!
//! let temp_dir = std::env::temp_dir().join("loom_snapshot_doc_example");
//! let _ = std::fs::remove_dir_all(&temp_dir);
//! std::fs::create_dir_all(&temp_dir).unwrap();
//! std::fs::write(
//!     temp_dir.join("example.snap"),
//!     "# loom-snapshot: example\n\nhello world\n",
//! )
//! .unwrap();
//!
//! assert_snapshot_at(&temp_dir, "example", "hello world");
//! let _ = std::fs::remove_dir_all(temp_dir);
//! ```

use std::path::{Path, PathBuf};

/// Asserts that `actual` matches the stored snapshot named `name` under `directory`.
///
/// If the snapshot file exists and matches `actual`, this function returns normally.
///
/// Environment variable behavior:
/// - If `LOOM_BLESS` is set to `1`, the snapshot file is written or updated.
/// - If the file does not exist and `CI` is not set, the snapshot file is created,
///   a message is printed to stderr, and the function returns.
/// - Otherwise, if the snapshot file is missing or does not match `actual`, the function panics
///   with a colored diff and a hint to set `LOOM_BLESS=1`.
///
/// # Panics
///
/// Panics if:
/// - `name` does not match `[a-z0-9_\-/]+`.
/// - The snapshot file is missing and `CI` is set.
/// - The snapshot file contents do not match `actual`.
/// - File system operations (e.g., reading/writing snapshot files) fail.
pub fn assert_snapshot_at(directory: &Path, name: &str, actual: &str) {
    let bless = std::env::var("LOOM_BLESS").is_ok_and(|v| v == "1");
    let ci = std::env::var("CI").is_ok();
    assert_snapshot_impl(directory, name, actual, bless, ci);
}

/// Internal implementation of `assert_snapshot_at` accepting `bless` and `ci` flags.
///
/// This permits deterministic testing without modifying process environment variables.
///
/// # Panics
///
/// Panics if `name` is invalid, if snapshot comparison fails, or on I/O error.
pub fn assert_snapshot_impl(directory: &Path, name: &str, actual: &str, bless: bool, ci: bool) {
    assert!(
        is_valid_name(name),
        "invalid snapshot name: {name:?}, must match [a-z0-9_\\-/]+"
    );

    let path = snapshot_path(directory, name);
    let actual_norm = normalize_crlf(actual);
    let actual_trimmed = actual_norm.strip_suffix('\n').unwrap_or(&actual_norm);

    let exists = path.exists();

    if bless {
        write_snapshot(&path, name, actual);
        return;
    }

    if exists {
        let file_content = match std::fs::read_to_string(&path) {
            Ok(content) => normalize_crlf(&content),
            Err(err) => panic!("failed to read snapshot at {}: {err}", path.display()),
        };

        let expected_header = format!("# loom-snapshot: {name}\n\n");
        let expected_trimmed = if let Some(body) = file_content.strip_prefix(&expected_header) {
            body.strip_suffix('\n').unwrap_or(body)
        } else {
            file_content.strip_suffix('\n').unwrap_or(&file_content)
        };

        if expected_trimmed == actual_trimmed {
            return;
        }

        let diff = format_diff(expected_trimmed, actual_trimmed, &path);
        panic_mismatch(&path, &diff);
    }

    if !ci {
        write_snapshot(&path, name, actual);
        eprintln!("loom_snapshot: created {}", path.display());
        return;
    }

    let diff = format_diff("", actual_trimmed, &path);
    panic_mismatch(&path, &diff);
}

/// Asserts that `actual` matches the stored snapshot named `name`.
///
/// Snapshots are stored under `<CARGO_MANIFEST_DIR>/tests/snapshots/<name>.snap`.
///
/// # Examples
///
/// ```ignore
/// use loom_snapshot::assert_snapshot;
///
/// assert_snapshot!("my_test", "hello world");
/// ```
#[macro_export]
macro_rules! assert_snapshot {
    ($name:expr, $actual:expr) => {
        $crate::assert_snapshot_at(
            &::std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("snapshots"),
            ::std::convert::AsRef::<str>::as_ref(&($name)),
            ::std::convert::AsRef::<str>::as_ref(&($actual)),
        )
    };
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| matches!(ch, 'a'..='z' | '0'..='9' | '_' | '-' | '/'))
}

fn normalize_crlf(input: &str) -> String {
    input.replace("\r\n", "\n")
}

fn snapshot_path(directory: &Path, name: &str) -> PathBuf {
    let relative = format!("{name}.snap");
    let mut path = directory.to_path_buf();
    for component in relative.split('/') {
        path.push(component);
    }
    path
}

fn format_snapshot(name: &str, actual: &str) -> String {
    let actual_norm = normalize_crlf(actual);
    let mut out = format!("# loom-snapshot: {name}\n\n{actual_norm}");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn write_snapshot(path: &Path, name: &str, actual: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|err| panic!("failed to create directory {}: {err}", parent.display()));
    }
    let formatted = format_snapshot(name, actual);
    std::fs::write(path, formatted)
        .unwrap_or_else(|err| panic!("failed to write snapshot to {}: {err}", path.display()));
}

fn panic_mismatch(path: &Path, diff: &str) -> ! {
    panic!(
        "snapshot mismatch at {}\n\n{}\n\nrun with LOOM_BLESS=1 to update",
        path.display(),
        diff
    );
}

enum DiffOp<'a> {
    Equal(&'a str),
    Delete(&'a str),
    Insert(&'a str),
}

fn compute_diff<'a>(expected_lines: &'a [&'a str], actual_lines: &'a [&'a str]) -> Vec<DiffOp<'a>> {
    let m = expected_lines.len();
    let n = actual_lines.len();
    let mut dp = vec![vec![0u32; n + 1]; m + 1];

    for i in (0..m).rev() {
        for j in (0..n).rev() {
            if expected_lines[i] == actual_lines[j] {
                dp[i][j] = dp[i + 1][j + 1] + 1;
            } else if dp[i + 1][j] >= dp[i][j + 1] {
                dp[i][j] = dp[i + 1][j];
            } else {
                dp[i][j] = dp[i][j + 1];
            }
        }
    }

    let mut ops = Vec::new();
    let mut i = 0;
    let mut j = 0;
    while i < m && j < n {
        if expected_lines[i] == actual_lines[j] {
            ops.push(DiffOp::Equal(expected_lines[i]));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push(DiffOp::Delete(expected_lines[i]));
            i += 1;
        } else {
            ops.push(DiffOp::Insert(actual_lines[j]));
            j += 1;
        }
    }
    while i < m {
        ops.push(DiffOp::Delete(expected_lines[i]));
        i += 1;
    }
    while j < n {
        ops.push(DiffOp::Insert(actual_lines[j]));
        j += 1;
    }
    ops
}

fn format_diff(expected: &str, actual: &str, path: &Path) -> String {
    let expected_lines: Vec<&str> = if expected.is_empty() {
        Vec::new()
    } else {
        expected.lines().collect()
    };
    let actual_lines: Vec<&str> = if actual.is_empty() {
        Vec::new()
    } else {
        actual.lines().collect()
    };

    if expected_lines.len() > 2000 || actual_lines.len() > 2000 {
        return format!(
            "--- expected: {}\n{}\n+++ actual:\n{}",
            path.display(),
            expected,
            actual
        );
    }

    let ops = compute_diff(&expected_lines, &actual_lines);
    let total = ops.len();
    let is_change: Vec<bool> = ops
        .iter()
        .map(|op| !matches!(op, DiffOp::Equal(_)))
        .collect();

    let mut show = vec![false; total];
    for (idx, &change) in is_change.iter().enumerate() {
        if change {
            let start = idx.saturating_sub(2);
            let end = (idx + 3).min(total);
            for item in &mut show[start..end] {
                *item = true;
            }
        }
    }

    let mut out = String::new();
    let mut in_hunk = false;

    for (idx, op) in ops.iter().enumerate() {
        if show[idx] {
            if !in_hunk {
                if !out.is_empty() {
                    out.push_str("...\n");
                }
                in_hunk = true;
            }
            match op {
                DiffOp::Equal(line) => {
                    out.push(' ');
                    out.push_str(line);
                    out.push('\n');
                }
                DiffOp::Delete(line) => {
                    out.push('-');
                    out.push_str(line);
                    out.push('\n');
                }
                DiffOp::Insert(line) => {
                    out.push('+');
                    out.push_str(line);
                    out.push('\n');
                }
            }
        } else {
            in_hunk = false;
        }
    }

    if out.ends_with('\n') {
        out.pop();
    }
    out
}
