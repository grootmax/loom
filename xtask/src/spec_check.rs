//! Specification check for Loom grammar section references.

use std::collections::HashSet;

use crate::workspace_root;

/// Runs the `spec-check` command.
///
/// Reads `spec/loom-spec.md` and `spec/grammar.ebnf` relative to the workspace root,
/// verifies section references in grammar rules, and prints the result.
///
/// # Errors
///
/// Returns an error if reading files fails or if any grammar rule lacks a valid
/// section reference pointing to an existing heading in the spec.
pub fn run() -> Result<(), String> {
    let root = workspace_root();
    let spec_path = root.join("spec").join("loom-spec.md");
    let grammar_path = root.join("spec").join("grammar.ebnf");

    let spec_content = std::fs::read_to_string(&spec_path)
        .map_err(|err| format!("could not read `{}`: {err}", spec_path.display()))?;
    let grammar_content = std::fs::read_to_string(&grammar_path)
        .map_err(|err| format!("could not read `{}`: {err}", grammar_path.display()))?;

    let valid_sections = parse_spec_sections(&spec_content);
    let result = check_grammar(&grammar_content, &valid_sections);

    match result {
        Ok(rule_count) => {
            println!("spec-check: {rule_count} grammar rules, all cite existing spec sections");
            Ok(())
        }
        Err(errors) => {
            for error in errors {
                eprintln!("{error}");
            }
            Err("spec-check failed".to_string())
        }
    }
}

/// Extracts valid section numbers from `loom-spec.md`.
///
/// A heading is a line starting with 2–4 `#` characters, a space, then a section number
/// (digits separated by `.`, with an optional trailing `.`), and a space.
#[must_use]
pub fn parse_spec_sections(content: &str) -> HashSet<String> {
    let mut sections = HashSet::new();

    for line in content.lines() {
        let trimmed = line.trim_start();
        let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
        if !(2..=4).contains(&hashes) {
            continue;
        }

        let after_hashes = &trimmed[hashes..];
        if !after_hashes.starts_with(' ') {
            continue;
        }

        let title = after_hashes.trim_start_matches(' ');
        let token = match title.split(' ').next() {
            Some(t) if !t.is_empty() => t,
            _ => continue,
        };

        let raw_sec = token.strip_suffix('.').unwrap_or(token);
        if is_valid_section_number(raw_sec) {
            sections.insert(raw_sec.to_string());
        }
    }

    sections
}

fn is_valid_section_number(sec: &str) -> bool {
    if sec.is_empty() {
        return false;
    }
    sec.split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

#[derive(Debug)]
struct LineAnalysis {
    rule_name: Option<String>,
    is_comment_line: bool,
    section_refs: Vec<String>,
    end_depth: usize,
}

fn analyze_line(line: &str, initial_depth: usize) -> LineAnalysis {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut depth = initial_depth;

    let mut rule_name = None;
    let mut has_non_whitespace_outside_comment = false;
    let mut section_refs = Vec::new();

    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'(' && bytes[i + 1] == b'*' {
            depth += 1;
            i += 2;
            continue;
        }

        if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b')' {
            depth = depth.saturating_sub(1);
            i += 2;
            continue;
        }

        if depth > 0 {
            if line[i..].starts_with('§') {
                let sec_start = i + '§'.len_utf8();
                let mut sec_end = sec_start;
                while sec_end < bytes.len()
                    && (bytes[sec_end].is_ascii_digit() || bytes[sec_end] == b'.')
                {
                    sec_end += 1;
                }
                let raw_sec = &line[sec_start..sec_end];
                let sec = raw_sec.strip_suffix('.').unwrap_or(raw_sec);
                if is_valid_section_number(sec) {
                    section_refs.push(sec.to_string());
                }
                i = sec_end;
                continue;
            }
            let char_len = line[i..].chars().next().map_or(1, char::len_utf8);
            i += char_len;
        } else {
            let b = bytes[i];
            if b.is_ascii_whitespace() {
                i += 1;
                continue;
            }

            if !has_non_whitespace_outside_comment {
                has_non_whitespace_outside_comment = true;
                if is_ident_start(b) {
                    let mut j = i + 1;
                    while j < bytes.len() && is_ident_continue(bytes[j]) {
                        j += 1;
                    }
                    let candidate_ident = &line[i..j];
                    let mut k = j;
                    while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    if k < bytes.len() && bytes[k] == b'=' {
                        rule_name = Some(candidate_ident.to_string());
                    }
                }
            }

            let char_len = line[i..].chars().next().map_or(1, char::len_utf8);
            i += char_len;
        }
    }

    let is_blank = line.trim().is_empty();
    let is_comment_line = !is_blank && !has_non_whitespace_outside_comment;

    LineAnalysis {
        rule_name,
        is_comment_line,
        section_refs,
        end_depth: depth,
    }
}

const fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

const fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Checks section references in grammar content against valid spec sections.
///
/// # Errors
///
/// Returns a list of error strings for each rule that lacks a valid section reference comment
/// or references a missing spec section.
pub fn check_grammar(
    grammar_content: &str,
    valid_sections: &HashSet<String>,
) -> Result<usize, Vec<String>> {
    let lines: Vec<&str> = grammar_content.lines().collect();
    let mut analyses = Vec::with_capacity(lines.len());
    let mut current_depth = 0;

    for line in &lines {
        let line_analysis = analyze_line(line, current_depth);
        current_depth = line_analysis.end_depth;
        analyses.push(line_analysis);
    }

    let mut rule_count = 0;
    let mut errors = Vec::new();

    for (idx, item) in analyses.iter().enumerate() {
        if let Some(ref rule_name) = item.rule_name {
            rule_count += 1;
            let line_num = idx + 1;

            let mut preceding_comment_analysis = None;
            for prev_idx in (0..idx).rev() {
                if !lines[prev_idx].trim().is_empty() {
                    preceding_comment_analysis = Some(&analyses[prev_idx]);
                    break;
                }
            }

            let has_valid_ref = match preceding_comment_analysis {
                Some(prev) if prev.is_comment_line && prev.section_refs.len() == 1 => {
                    Some(&prev.section_refs[0])
                }
                _ => None,
            };

            match has_valid_ref {
                None => {
                    errors.push(format!(
                        "grammar.ebnf:{line_num}: rule `{rule_name}` has no (* §N.M *) section reference"
                    ));
                }
                Some(sec_ref) => {
                    if !valid_sections.contains(sec_ref) {
                        errors.push(format!(
                            "grammar.ebnf:{line_num}: rule `{rule_name}` cites §{sec_ref}, which is not a heading in spec/loom-spec.md"
                        ));
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(rule_count)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_rule() {
        let spec = "## 3. Items\n### 3.1 Functions";
        let grammar = "(* §3.1 *)\nfn_def = \"fn\" ;";
        let valid_sections = parse_spec_sections(spec);
        let result = check_grammar(grammar, &valid_sections);
        assert_eq!(result, Ok(1));
    }

    #[test]
    fn test_missing_reference() {
        let spec = "## 3. Items";
        let grammar = "fn_def = \"fn\" ;";
        let valid_sections = parse_spec_sections(spec);
        let result = check_grammar(grammar, &valid_sections);
        assert_eq!(
            result,
            Err(vec![
                "grammar.ebnf:1: rule `fn_def` has no (* §N.M *) section reference".to_string()
            ])
        );
    }

    #[test]
    fn test_reference_to_missing_section() {
        let spec = "## 3. Items";
        let grammar = "(* §99.9 *)\nfn_def = \"fn\" ;";
        let valid_sections = parse_spec_sections(spec);
        let result = check_grammar(grammar, &valid_sections);
        assert_eq!(
            result,
            Err(vec![
                "grammar.ebnf:2: rule `fn_def` cites §99.9, which is not a heading in spec/loom-spec.md".to_string()
            ])
        );
    }

    #[test]
    fn test_rule_inside_multiline_comment_ignored() {
        let spec = "## 3. Items";
        let grammar = "(*\n  fn_def = \"fn\" ;\n*)";
        let valid_sections = parse_spec_sections(spec);
        let result = check_grammar(grammar, &valid_sections);
        assert_eq!(result, Ok(0));
    }

    #[test]
    fn test_trailing_dot_heading() {
        let spec = "## 3. Items\n### 3.1. Functions";
        let valid_sections = parse_spec_sections(spec);
        assert!(valid_sections.contains("3"));
        assert!(valid_sections.contains("3.1"));
    }

    #[test]
    fn test_nested_section_numbers() {
        let spec = "#### 2.7.1 Integer literals";
        let grammar = "(* §2.7.1 *)\nINT_LIT = dec_lit ;";
        let valid_sections = parse_spec_sections(spec);
        assert!(valid_sections.contains("2.7.1"));
        let result = check_grammar(grammar, &valid_sections);
        assert_eq!(result, Ok(1));
    }
}
