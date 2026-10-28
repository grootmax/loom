//! Integration tests for `loom_snapshot`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use loom_snapshot::{assert_snapshot_at, assert_snapshot_impl};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(label: &str) -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "loom_snap_test_{label}_{}_{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn catch_panic_message<F: FnOnce() + std::panic::UnwindSafe>(f: F) -> String {
    let result = std::panic::catch_unwind(f);
    assert!(result.is_err(), "expected panic but function succeeded");
    let err = result.unwrap_err();
    if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        panic!("unexpected panic payload type");
    }
}

#[test]
fn test_snapshot_match() {
    let dir = temp_dir("match");
    let snap_path = dir.join("test_match.snap");
    std::fs::write(
        &snap_path,
        "# loom-snapshot: test_match\n\nline 1\nline 2\n",
    )
    .unwrap();

    assert_snapshot_impl(&dir, "test_match", "line 1\nline 2", false, false);
}

#[test]
fn test_snapshot_mismatch_panics() {
    let dir = temp_dir("mismatch");
    let snap_path = dir.join("test_mismatch.snap");
    std::fs::write(
        &snap_path,
        "# loom-snapshot: test_mismatch\n\nline 1\nline 2\n",
    )
    .unwrap();

    let msg = catch_panic_message(|| {
        assert_snapshot_impl(&dir, "test_mismatch", "line 1\nline changed", false, false);
    });

    assert!(
        msg.contains("test_mismatch.snap"),
        "panic message missing path: {msg}"
    );
    assert!(
        msg.contains("-line 2"),
        "panic message missing deleted line: {msg}"
    );
    assert!(
        msg.contains("+line changed"),
        "panic message missing inserted line: {msg}"
    );
    assert!(
        msg.contains("run with LOOM_BLESS=1 to update"),
        "panic message missing hint: {msg}"
    );
}

#[test]
fn test_bless_mode_overwrites() {
    let dir = temp_dir("bless");
    let snap_path = dir.join("test_bless.snap");
    std::fs::write(&snap_path, "# loom-snapshot: test_bless\n\nold content\n").unwrap();

    assert_snapshot_impl(&dir, "test_bless", "new content", true, false);

    let content = std::fs::read_to_string(&snap_path).unwrap();
    assert_eq!(content, "# loom-snapshot: test_bless\n\nnew content\n");
}

#[test]
fn test_missing_file_created_when_ci_unset() {
    let dir = temp_dir("missing_ci_unset");
    let snap_path = dir.join("test_created.snap");
    assert!(!snap_path.exists());

    assert_snapshot_impl(&dir, "test_created", "created content", false, false);

    assert!(snap_path.exists());
    let content = std::fs::read_to_string(&snap_path).unwrap();
    assert_eq!(
        content,
        "# loom-snapshot: test_created\n\ncreated content\n"
    );
}

#[test]
fn test_missing_file_panics_when_ci_set() {
    let dir = temp_dir("missing_ci_set");

    let msg = catch_panic_message(|| {
        assert_snapshot_impl(&dir, "test_missing", "content", false, true);
    });

    assert!(
        msg.contains("test_missing.snap"),
        "missing file panic missing path: {msg}"
    );
    assert!(
        msg.contains("+content"),
        "missing file panic missing diff: {msg}"
    );
    assert!(
        msg.contains("run with LOOM_BLESS=1 to update"),
        "missing file panic missing hint: {msg}"
    );
}

#[test]
fn test_crlf_normalization() {
    let dir = temp_dir("crlf");
    let snap_path = dir.join("test_crlf.snap");
    std::fs::write(
        &snap_path,
        "# loom-snapshot: test_crlf\r\n\r\nline 1\r\nline 2\r\n",
    )
    .unwrap();

    // Actual with LF
    assert_snapshot_impl(&dir, "test_crlf", "line 1\nline 2", false, false);

    // Actual with CRLF
    assert_snapshot_impl(&dir, "test_crlf", "line 1\r\nline 2\r\n", false, false);
}

#[test]
fn test_invalid_name_panics() {
    let dir = temp_dir("invalid_name");
    let invalid_names = ["", "Upper", "with space", "dot.name", "special!"];

    for name in invalid_names {
        let msg = catch_panic_message(|| {
            assert_snapshot_impl(&dir, name, "content", false, false);
        });
        assert!(
            msg.contains("invalid snapshot name"),
            "expected invalid name panic for {name:?}, got: {msg}"
        );
    }
}

#[test]
fn test_nested_directory_name() {
    let dir = temp_dir("nested");

    assert_snapshot_impl(&dir, "sub/dir/test_nested", "nested content", true, false);

    let snap_path = dir.join("sub").join("dir").join("test_nested.snap");
    assert!(snap_path.exists());
    let content = std::fs::read_to_string(&snap_path).unwrap();
    assert_eq!(
        content,
        "# loom-snapshot: sub/dir/test_nested\n\nnested content\n"
    );
}

#[test]
fn test_assert_snapshot_at_public_api() {
    let dir = temp_dir("public_api");
    let snap_path = dir.join("test_pub.snap");
    std::fs::write(&snap_path, "# loom-snapshot: test_pub\n\nhello\n").unwrap();

    assert_snapshot_at(&dir, "test_pub", "hello");
}
