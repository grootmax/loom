//! Integration tests verifying that error explanation examples and fixes parse as expected.

use loom_diagnostics::codes;
use loom_span::FileId;
use std::fs;
use std::path::Path;

fn extract_loom_block(markdown: &str, header: &str) -> Option<String> {
    let mut lines = markdown
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l));
    for line in lines.by_ref() {
        if line.trim() == header {
            break;
        }
    }
    let mut in_code = false;
    let mut code_lines = Vec::new();
    for line in lines {
        if line.starts_with("## ") {
            return None;
        }
        if in_code {
            if line.trim() == "```" {
                let mut block = code_lines.join("\n");
                block.push('\n');
                return Some(block);
            }
            code_lines.push(line);
        } else if line.trim().starts_with("```loom") {
            in_code = true;
        }
    }
    None
}

#[test]
fn test_explanation_examples_and_fixes() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let explanations_dir = manifest_dir.join("../loom_diagnostics/explanations");

    let mut failures = Vec::new();

    for code in codes::ALL {
        let id = code.id;

        // Skip E0001 (an invalid UTF-8 byte cannot appear in Markdown)
        if id == "E0001" {
            continue;
        }
        // Skip E0112 (nesting too deep is too large to show in an explanation example)
        if id == "E0112" {
            continue;
        }

        let file_path = explanations_dir.join(format!("{id}.md"));
        if !file_path.exists() {
            continue;
        }
        let markdown = fs::read_to_string(&file_path)
            .unwrap_or_else(|err| panic!("Failed to read {file_path:?}: {err}"));

        let example_code = extract_loom_block(&markdown, "## Example")
            .unwrap_or_else(|| panic!("{id}: missing ## Example ```loom block"));
        let fix_code = extract_loom_block(&markdown, "## How to fix it")
            .unwrap_or_else(|| panic!("{id}: missing ## How to fix it ```loom block"));

        // Check example produces the target error code
        let parse_example = loom_parser::parse(&example_code, FileId::DETACHED);
        let reported_codes: Vec<_> = parse_example
            .diagnostics()
            .iter()
            .filter_map(|d| d.code().map(|c| c.id))
            .collect();
        if !reported_codes.contains(&id) {
            failures.push(format!(
                "{id} EXAMPLE: expected error {id}, got {reported_codes:?}\nSnippet:\n{example_code}"
            ));
        }

        // Check fix produces no errors
        let parse_fix = loom_parser::parse(&fix_code, FileId::DETACHED);
        let fix_errors: Vec<_> = parse_fix
            .diagnostics()
            .iter()
            .filter(|d| d.is_error())
            .map(|d| format!("{:?}: {}", d.code().map(|c| c.id), d.message))
            .collect();
        if !fix_errors.is_empty() {
            failures.push(format!(
                "{id} FIX: expected no errors, got {fix_errors:?}\nSnippet:\n{fix_code}"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "Found {} failures:\n\n{}",
        failures.len(),
        failures.join("\n---\n")
    );
}
