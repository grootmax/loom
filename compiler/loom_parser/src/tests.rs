//! Parser tests: every example parses without errors and losslessly.

use loom_span::FileId;

use crate::parse;

#[test]
fn every_example_parses_cleanly_and_losslessly() {
    let examples = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(examples).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "loom") {
            let source = std::fs::read_to_string(&path).unwrap();
            let parse = parse(&source, FileId::DETACHED);
            assert_eq!(parse.syntax().text(), source, "{}", path.display());
            for diagnostic in parse.diagnostics() {
                let line = source[..diagnostic.primary.span.range.start().to_usize()]
                    .lines()
                    .count();
                failures.push(format!("{}:{line}: {}", path.display(), diagnostic.message));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
