//! Smoke tests and regression replay for `loom_fuzz`.

use std::fs;
use std::path::PathBuf;

use loom_fuzz::cli::{FuzzTarget, Options};
use loom_fuzz::{run_fuzzer, run_replay};

#[test]
fn smoke_test_lex() {
    let options = Options {
        target: FuzzTarget::Lex,
        seconds: None,
        iterations: Some(500),
        seed: 42,
        corpus_dirs: vec![
            PathBuf::from("../../examples"),
            PathBuf::from("../../spec/conformance"),
        ],
        out_dir: PathBuf::from("../../target/fuzz_test_lex"),
        replay_file: None,
    };

    let failures = run_fuzzer(&options).expect("fuzzer execution failed");
    assert_eq!(failures, 0, "smoke test lex reported failures");
}

#[test]
fn smoke_test_parse() {
    let options = Options {
        target: FuzzTarget::Parse,
        seconds: None,
        iterations: Some(500),
        seed: 42,
        corpus_dirs: vec![
            PathBuf::from("../../examples"),
            PathBuf::from("../../spec/conformance"),
        ],
        out_dir: PathBuf::from("../../target/fuzz_test_parse"),
        replay_file: None,
    };

    let failures = run_fuzzer(&options).expect("fuzzer execution failed");
    assert_eq!(failures, 0, "smoke test parse reported failures");
}

#[test]
fn smoke_test_fmt() {
    let options = Options {
        target: FuzzTarget::Fmt,
        seconds: None,
        iterations: Some(500),
        seed: 42,
        corpus_dirs: vec![
            PathBuf::from("../../examples"),
            PathBuf::from("../../spec/conformance"),
        ],
        out_dir: PathBuf::from("../../target/fuzz_test_fmt"),
        replay_file: None,
    };

    let failures = run_fuzzer(&options).expect("fuzzer execution failed");
    assert_eq!(failures, 0, "smoke test fmt reported failures");
}

#[test]
fn replay_regressions() {
    let reg_dir = PathBuf::from("corpus/regressions");
    if !reg_dir.exists() {
        return;
    }

    let entries = fs::read_dir(&reg_dir).expect("failed to read regressions dir");
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("loom") {
            run_replay(&path, FuzzTarget::All)
                .unwrap_or_else(|e| panic!("regression replay failed on {}: {e}", path.display()));
        }
    }
}
