//! Generator for Unicode character tables used by `loom_unicode`.
//!
//! Downloads/sources required:
//! - `https://www.unicode.org/Public/17.0.0/ucd/DerivedCoreProperties.txt`
//! - `https://www.unicode.org/Public/17.0.0/ucd/PropList.txt`
//!
//! Run with: `cargo xtask gen-unicode <UCD_DIR>`

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// Runs the Unicode table generator reading UCD files from `ucd_dir`.
///
/// # Errors
/// Returns an error message string if reading or parsing fails, or if the
/// UCD version is not 17.0.0.
pub fn run(ucd_dir: &Path) -> Result<(), String> {
    let derived_path = ucd_dir.join("DerivedCoreProperties.txt");
    let prop_path = ucd_dir.join("PropList.txt");

    let derived_content = fs::read_to_string(&derived_path)
        .map_err(|err| format!("failed to read `{}`: {err}", derived_path.display()))?;
    let prop_content = fs::read_to_string(&prop_path)
        .map_err(|err| format!("failed to read `{}`: {err}", prop_path.display()))?;

    verify_version_17_0_0(&derived_content, "DerivedCoreProperties.txt")?;
    verify_version_17_0_0(&prop_content, "PropList.txt")?;

    let xid_start = parse_and_merge(&derived_content, "XID_Start")?;
    let xid_continue = parse_and_merge(&derived_content, "XID_Continue")?;
    let white_space = parse_and_merge(&prop_content, "White_Space")?;

    let output = generate_tables_rs(&xid_start, &xid_continue, &white_space);

    let tables_path = crate::workspace_root()
        .join("compiler")
        .join("loom_unicode")
        .join("src")
        .join("tables.rs");

    if let Some(parent) = tables_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create dir `{}`: {err}", parent.display()))?;
    }

    fs::write(&tables_path, output)
        .map_err(|err| format!("failed to write `{}`: {err}", tables_path.display()))?;

    eprintln!("xtask: generated {}", tables_path.display());
    Ok(())
}

fn verify_version_17_0_0(content: &str, file_name: &str) -> Result<(), String> {
    let has_version = content.lines().take(20).any(|line| line.contains("17.0.0"));
    if has_version {
        Ok(())
    } else {
        Err(format!(
            "`{file_name}` does not appear to be Unicode version 17.0.0"
        ))
    }
}

/// Parses property ranges from a UCD file content and merges adjacent ranges.
fn parse_and_merge(content: &str, target_prop: &str) -> Result<Vec<(u32, u32)>, String> {
    let raw_ranges = parse_ucd_property(content, target_prop)?;
    Ok(merge_ranges(raw_ranges))
}

/// Parses raw property ranges from a UCD data file string for `target_prop`.
///
/// # Errors
/// Returns an error message string if any line has an invalid hex code point or range.
pub fn parse_ucd_property(content: &str, target_prop: &str) -> Result<Vec<(u32, u32)>, String> {
    let mut ranges = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let code_part = match line.find('#') {
            Some(idx) => &line[..idx],
            None => line,
        };

        let parts: Vec<&str> = code_part.split(';').map(str::trim).collect();
        if parts.len() < 2 {
            continue;
        }

        if parts[1] != target_prop {
            continue;
        }

        let range_str = parts[0];
        if range_str.contains("..") {
            let range_parts: Vec<&str> = range_str.split("..").map(str::trim).collect();
            if range_parts.len() != 2 {
                return Err(format!("invalid range `{range_str}` in line: {line}"));
            }
            let start = u32::from_str_radix(range_parts[0], 16).map_err(|err| {
                format!("invalid hex `{}` in `{range_str}`: {err}", range_parts[0])
            })?;
            let end = u32::from_str_radix(range_parts[1], 16).map_err(|err| {
                format!("invalid hex `{}` in `{range_str}`: {err}", range_parts[1])
            })?;
            ranges.push((start, end));
        } else {
            let val = u32::from_str_radix(range_str, 16)
                .map_err(|err| format!("invalid hex `{range_str}`: {err}"))?;
            ranges.push((val, val));
        }
    }

    Ok(ranges)
}

/// Sorts and merges overlapping or adjacent ranges.
#[must_use]
pub fn merge_ranges(mut ranges: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    ranges.sort_unstable_by_key(|&(start, _)| start);

    let mut merged = Vec::new();
    for (start, end) in ranges {
        if let Some((_, last_end)) = merged.last_mut()
            && start <= *last_end + 1
        {
            if end > *last_end {
                *last_end = end;
            }
            continue;
        }
        merged.push((start, end));
    }
    merged
}

fn generate_tables_rs(
    xid_start: &[(u32, u32)],
    xid_continue: &[(u32, u32)],
    white_space: &[(u32, u32)],
) -> String {
    let mut out = String::new();
    out.push_str("// @generated by `cargo xtask gen-unicode` from UCD 17.0.0. Do not edit.\n\n");

    out.push_str("#[rustfmt::skip]\n");
    out.push_str("pub(crate) static XID_START: &[(u32, u32)] = &[\n");
    for &(s, e) in xid_start {
        let _ = writeln!(out, "    (0x{s:04X}, 0x{e:04X}),");
    }
    out.push_str("];\n\n");

    out.push_str("#[rustfmt::skip]\n");
    out.push_str("pub(crate) static XID_CONTINUE: &[(u32, u32)] = &[\n");
    for &(s, e) in xid_continue {
        let _ = writeln!(out, "    (0x{s:04X}, 0x{e:04X}),");
    }
    out.push_str("];\n\n");

    out.push_str("#[rustfmt::skip]\n");
    out.push_str("pub(crate) static WHITE_SPACE: &[(u32, u32)] = &[\n");
    for &(s, e) in white_space {
        let _ = writeln!(out, "    (0x{s:04X}, 0x{e:04X}),");
    }
    out.push_str("];\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ucd_property_single_and_range() {
        let sample = "\
# DerivedCoreProperties-17.0.0.txt
0041..005A    ; XID_Start # L&  [26] LATIN CAPITAL LETTER A..LATIN CAPITAL LETTER Z
00AA          ; XID_Start # Lo       FEMININE ORDINAL INDICATOR
0030..0039    ; XID_Continue # Nd
";
        let xid_start = parse_ucd_property(sample, "XID_Start").unwrap();
        assert_eq!(xid_start, vec![(0x0041, 0x005A), (0x00AA, 0x00AA)]);

        let xid_continue = parse_ucd_property(sample, "XID_Continue").unwrap();
        assert_eq!(xid_continue, vec![(0x0030, 0x0039)]);
    }

    #[test]
    fn test_merge_ranges() {
        let input = vec![
            (0x0041, 0x005A),
            (0x005B, 0x0060),
            (0x00AA, 0x00AA),
            (0x0055, 0x005D),
        ];
        let merged = merge_ranges(input);
        assert_eq!(merged, vec![(0x0041, 0x0060), (0x00AA, 0x00AA)]);
    }

    #[test]
    fn test_verify_version() {
        let ok_content = "# DerivedCoreProperties-17.0.0.txt\n";
        let bad_content = "# DerivedCoreProperties-16.0.0.txt\n";
        assert!(verify_version_17_0_0(ok_content, "file.txt").is_ok());
        assert!(verify_version_17_0_0(bad_content, "file.txt").is_err());
    }
}
