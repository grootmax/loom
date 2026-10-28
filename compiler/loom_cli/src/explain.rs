//! Embedded explanations for Loom error codes.

/// Embedded explanation Markdown text for every Loom error code.
pub(crate) static EXPLANATIONS: &[(&str, &str)] = &[
    (
        "E0001",
        include_str!("../../loom_diagnostics/explanations/E0001.md"),
    ),
    (
        "E0002",
        include_str!("../../loom_diagnostics/explanations/E0002.md"),
    ),
    (
        "E0003",
        include_str!("../../loom_diagnostics/explanations/E0003.md"),
    ),
    (
        "E0004",
        include_str!("../../loom_diagnostics/explanations/E0004.md"),
    ),
    (
        "E0005",
        include_str!("../../loom_diagnostics/explanations/E0005.md"),
    ),
    (
        "E0006",
        include_str!("../../loom_diagnostics/explanations/E0006.md"),
    ),
    (
        "E0007",
        include_str!("../../loom_diagnostics/explanations/E0007.md"),
    ),
    (
        "E0008",
        include_str!("../../loom_diagnostics/explanations/E0008.md"),
    ),
    (
        "E0009",
        include_str!("../../loom_diagnostics/explanations/E0009.md"),
    ),
    (
        "E0010",
        include_str!("../../loom_diagnostics/explanations/E0010.md"),
    ),
    (
        "E0011",
        include_str!("../../loom_diagnostics/explanations/E0011.md"),
    ),
    (
        "E0012",
        include_str!("../../loom_diagnostics/explanations/E0012.md"),
    ),
    (
        "E0013",
        include_str!("../../loom_diagnostics/explanations/E0013.md"),
    ),
    (
        "E0014",
        include_str!("../../loom_diagnostics/explanations/E0014.md"),
    ),
    (
        "E0015",
        include_str!("../../loom_diagnostics/explanations/E0015.md"),
    ),
    (
        "E0016",
        include_str!("../../loom_diagnostics/explanations/E0016.md"),
    ),
    (
        "E0017",
        include_str!("../../loom_diagnostics/explanations/E0017.md"),
    ),
    (
        "E0018",
        include_str!("../../loom_diagnostics/explanations/E0018.md"),
    ),
    (
        "E0019",
        include_str!("../../loom_diagnostics/explanations/E0019.md"),
    ),
    (
        "E0020",
        include_str!("../../loom_diagnostics/explanations/E0020.md"),
    ),
    (
        "E0021",
        include_str!("../../loom_diagnostics/explanations/E0021.md"),
    ),
    (
        "E0022",
        include_str!("../../loom_diagnostics/explanations/E0022.md"),
    ),
    (
        "E0023",
        include_str!("../../loom_diagnostics/explanations/E0023.md"),
    ),
    (
        "E0024",
        include_str!("../../loom_diagnostics/explanations/E0024.md"),
    ),
    (
        "E0025",
        include_str!("../../loom_diagnostics/explanations/E0025.md"),
    ),
    (
        "E0026",
        include_str!("../../loom_diagnostics/explanations/E0026.md"),
    ),
    (
        "E0027",
        include_str!("../../loom_diagnostics/explanations/E0027.md"),
    ),
    (
        "E0100",
        include_str!("../../loom_diagnostics/explanations/E0100.md"),
    ),
    (
        "E0101",
        include_str!("../../loom_diagnostics/explanations/E0101.md"),
    ),
    (
        "E0102",
        include_str!("../../loom_diagnostics/explanations/E0102.md"),
    ),
    (
        "E0103",
        include_str!("../../loom_diagnostics/explanations/E0103.md"),
    ),
    (
        "E0104",
        include_str!("../../loom_diagnostics/explanations/E0104.md"),
    ),
    (
        "E0105",
        include_str!("../../loom_diagnostics/explanations/E0105.md"),
    ),
    (
        "E0106",
        include_str!("../../loom_diagnostics/explanations/E0106.md"),
    ),
    (
        "E0107",
        include_str!("../../loom_diagnostics/explanations/E0107.md"),
    ),
    (
        "E0108",
        include_str!("../../loom_diagnostics/explanations/E0108.md"),
    ),
    (
        "E0109",
        include_str!("../../loom_diagnostics/explanations/E0109.md"),
    ),
    (
        "E0110",
        include_str!("../../loom_diagnostics/explanations/E0110.md"),
    ),
    (
        "E0111",
        include_str!("../../loom_diagnostics/explanations/E0111.md"),
    ),
    (
        "E0112",
        include_str!("../../loom_diagnostics/explanations/E0112.md"),
    ),
    (
        "E0113",
        include_str!("../../loom_diagnostics/explanations/E0113.md"),
    ),
];

/// Looks up an explanation by error code ID (case-insensitive).
#[must_use]
pub(crate) fn explanation(id: &str) -> Option<&'static str> {
    let normalized = id.to_ascii_uppercase();
    EXPLANATIONS
        .iter()
        .find(|(code, _)| *code == normalized)
        .map(|(_, text)| *text)
}

/// Extracts the first ` ```loom ` block after `header`.
#[cfg(test)]
#[must_use]
pub(crate) fn extract_loom_block(markdown: &str, header: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use loom_diagnostics::codes;
    use std::collections::HashSet;
    use std::fs;
    use std::path::Path;

    #[test]
    fn explanations_match_registry_codes() {
        let registry_ids: HashSet<_> = codes::ALL.iter().map(|code| code.id).collect();
        let table_ids: HashSet<_> = EXPLANATIONS.iter().map(|(id, _)| *id).collect();

        assert_eq!(EXPLANATIONS.len(), codes::ALL.len(), "length mismatch");
        assert_eq!(
            table_ids.len(),
            EXPLANATIONS.len(),
            "duplicate keys in EXPLANATIONS"
        );
        assert_eq!(
            table_ids, registry_ids,
            "EXPLANATIONS entries must match ALL exactly"
        );

        // Verify order matches codes::ALL
        for (i, code) in codes::ALL.iter().enumerate() {
            assert_eq!(
                EXPLANATIONS[i].0, code.id,
                "EXPLANATIONS entry at index {i} does not match code order"
            );
        }
    }

    #[test]
    fn explanations_match_directory_files() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let explanations_dir = manifest_dir.join("../loom_diagnostics/explanations");
        let entries =
            fs::read_dir(&explanations_dir).expect("failed to read explanations directory");

        for entry in entries {
            let entry = entry.expect("valid directory entry");
            let file_name = entry.file_name();
            let name_str = file_name.to_str().expect("UTF-8 filename");
            if name_str.starts_with('E')
                && Path::new(name_str)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
            {
                let code_id = &name_str[..name_str.len() - 3];
                assert!(
                    explanation(code_id).is_some(),
                    "Missing explanation entry for file {name_str}"
                );
            }
        }
    }

    #[test]
    fn explanation_format_and_sections() {
        for (id, text) in EXPLANATIONS {
            let code = codes::lookup(id).unwrap_or_else(|| panic!("code {id} not in registry"));
            let first_line = text.lines().next().unwrap_or_default();
            let expected_title = format!("# {id}: {}", code.title);
            assert_eq!(
                first_line, expected_title,
                "First line of {id} explanation must be '# {{id}}: {{title}}'"
            );

            let example = extract_loom_block(text, "## Example");
            assert!(
                example.is_some(),
                "Explanation for {id} missing '## Example' with ```loom code block"
            );

            let fix = extract_loom_block(text, "## How to fix it");
            assert!(
                fix.is_some(),
                "Explanation for {id} missing '## How to fix it' with ```loom code block"
            );
        }
    }
}
