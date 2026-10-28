//! Diagnostic renderer that produces rustc-style snippets with optional ANSI color.

use std::fmt::Write as _;

use loom_span::{SourceFile, SourceMap, TextRange, TextSize};

use crate::{Diagnostic, Label, Severity};

/// Options controlling diagnostic rendering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderOptions {
    /// Whether to include ANSI color escape codes in output.
    pub color: bool,
}

const RESET: &str = "\x1b[0m";
const BOLD_RED: &str = "\x1b[1;31m";
const BOLD_YELLOW: &str = "\x1b[1;33m";
const BOLD: &str = "\x1b[1m";
const BOLD_BLUE: &str = "\x1b[1;34m";

/// Renders a diagnostic as a rustc-style source snippet.
#[must_use]
pub fn render(diagnostic: &Diagnostic, sources: &SourceMap, options: &RenderOptions) -> String {
    let mut out = String::new();

    let severity_color = match diagnostic.severity {
        Severity::Error => BOLD_RED,
        Severity::Warning => BOLD_YELLOW,
    };

    // Line 1: {severity}[{id}]: {message}
    if options.color {
        let _ = writeln!(
            out,
            "{severity_color}{}[{}]{RESET}: {BOLD}{}{RESET}",
            diagnostic.severity, diagnostic.id, diagnostic.message
        );
    } else {
        let _ = writeln!(
            out,
            "{}[{}]: {}",
            diagnostic.severity, diagnostic.id, diagnostic.message
        );
    }

    let source_file = sources.get(diagnostic.primary.span.file);

    let Some(file) = source_file else {
        render_extras(&mut out, diagnostic, " ", *options);
        return out;
    };

    let primary_start = diagnostic.primary.span.range.start();
    let primary_pos = file.line_column(primary_start);

    // Collect all labels in the same file as the primary label.
    let primary_file = diagnostic.primary.span.file;
    let mut same_file_labels = Vec::new();
    same_file_labels.push((true, &diagnostic.primary));
    for sec in &diagnostic.secondary {
        if sec.span.file == primary_file {
            same_file_labels.push((false, sec));
        }
    }

    // Determine shown line numbers.
    let mut shown_lines: Vec<u32> = same_file_labels
        .iter()
        .map(|(_, label)| file.line_column(label.span.range.start()).line)
        .collect();
    shown_lines.sort_unstable();
    shown_lines.dedup();

    let max_line = shown_lines.iter().max().copied().unwrap_or(1);
    let width = max_line.to_string().len();
    let pad = " ".repeat(width);

    // Line 2: {pad}--> {file}:{line}:{column}
    if options.color {
        let _ = writeln!(
            out,
            "{pad}{BOLD_BLUE}-->{RESET} {}:{}:{}",
            file.name(),
            primary_pos.line,
            primary_pos.column
        );
        let _ = writeln!(out, "{pad} {BOLD_BLUE}|{RESET}");
    } else {
        let _ = writeln!(
            out,
            "{pad}--> {}:{}:{}",
            file.name(),
            primary_pos.line,
            primary_pos.column
        );
        let _ = writeln!(out, "{pad} |");
    }

    for (i, &line_num) in shown_lines.iter().enumerate() {
        if i > 0 && line_num - shown_lines[i - 1] > 1 {
            out.push_str("...\n");
        }

        render_source_and_underlines(
            &mut out,
            file,
            line_num,
            width,
            &pad,
            &same_file_labels,
            severity_color,
            *options,
        );
    }

    render_extras(&mut out, diagnostic, &pad, *options);

    out
}

#[allow(clippy::too_many_arguments)]
fn render_source_and_underlines(
    out: &mut String,
    file: &SourceFile,
    line_num: u32,
    width: usize,
    pad: &str,
    same_file_labels: &[(bool, &Label)],
    severity_color: &'static str,
    options: RenderOptions,
) {
    let line_idx = (line_num - 1) as usize;
    let raw_line = file.line_text(line_idx).unwrap_or("");
    let trimmed_line = raw_line.trim_end_matches([' ', '\t', '\r', '\n']);
    let expanded_line = trimmed_line.replace('\t', "    ");

    if options.color {
        let _ = writeln!(
            out,
            "{BOLD_BLUE}{line_num:>width$}{RESET} {BOLD_BLUE}|{RESET} {expanded_line}"
        );
    } else {
        let _ = writeln!(out, "{line_num:>width$} | {expanded_line}");
    }

    let mut labels_on_line: Vec<(bool, &Label)> = same_file_labels
        .iter()
        .copied()
        .filter(|(_, label)| file.line_column(label.span.range.start()).line == line_num)
        .collect();

    labels_on_line.sort_by(|a, b| {
        let col_a = file.line_column(a.1.span.range.start()).column;
        let col_b = file.line_column(b.1.span.range.start()).column;
        col_a.cmp(&col_b).then_with(|| b.0.cmp(&a.0))
    });

    let line_range = file.lines().line_range(line_idx, file.text());
    let line_start_byte = line_range.map_or(TextSize::new(0), TextRange::start);
    let line_end_byte = line_range.map_or(TextSize::new(0), TextRange::end);

    for (is_primary, label) in labels_on_line {
        render_underline_row(
            out,
            raw_line,
            line_start_byte,
            line_end_byte,
            is_primary,
            label,
            pad,
            severity_color,
            options,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_underline_row(
    out: &mut String,
    raw_line: &str,
    line_start_byte: TextSize,
    line_end_byte: TextSize,
    is_primary: bool,
    label: &Label,
    pad: &str,
    severity_color: &'static str,
    options: RenderOptions,
) {
    let label_start_byte = label.span.range.start();
    let label_end_byte = label.span.range.end();

    let is_multi_line = label_end_byte > line_end_byte;
    let effective_end_byte = if is_multi_line {
        line_end_byte
    } else {
        label_end_byte
    };

    let mut col_before = 0usize;
    let mut col_marker = 0usize;

    if label_start_byte >= line_end_byte {
        for (_idx, c) in raw_line.char_indices() {
            col_before += if c == '\t' { 4 } else { 1 };
        }
    } else {
        for (byte_offset, c) in raw_line.char_indices() {
            let offset_u32 = u32::try_from(byte_offset).unwrap_or(u32::MAX);
            let abs_byte = line_start_byte + TextSize::from(offset_u32);
            let char_width = if c == '\t' { 4 } else { 1 };
            if abs_byte < label_start_byte {
                col_before += char_width;
            } else if abs_byte < effective_end_byte {
                col_marker += char_width;
            }
        }
    }

    let num_markers = col_marker.max(1);
    let marker_char = if is_primary { '^' } else { '-' };

    let spaces_str = " ".repeat(col_before);
    let markers_str = marker_char.to_string().repeat(num_markers);

    let msg = label.message.as_str();
    let label_text = if is_multi_line {
        if msg.is_empty() {
            "...".to_string()
        } else {
            format!("... {msg}")
        }
    } else {
        msg.to_string()
    };

    if options.color {
        let marker_color = if is_primary {
            severity_color
        } else {
            BOLD_BLUE
        };
        if label_text.is_empty() {
            let _ = writeln!(
                out,
                "{pad} {BOLD_BLUE}|{RESET} {spaces_str}{marker_color}{markers_str}{RESET}"
            );
        } else {
            let _ = writeln!(
                out,
                "{pad} {BOLD_BLUE}|{RESET} {spaces_str}{marker_color}{markers_str} {label_text}{RESET}"
            );
        }
    } else if label_text.is_empty() {
        let _ = writeln!(out, "{pad} | {spaces_str}{markers_str}");
    } else {
        let _ = writeln!(out, "{pad} | {spaces_str}{markers_str} {label_text}");
    }
}

fn render_extras(out: &mut String, diagnostic: &Diagnostic, pad: &str, options: RenderOptions) {
    let has_extras = !diagnostic.notes.is_empty()
        || !diagnostic.help.is_empty()
        || !diagnostic.suggestions.is_empty();

    if !has_extras {
        return;
    }

    if options.color {
        let _ = writeln!(out, "{pad} {BOLD_BLUE}|{RESET}");
        for note in &diagnostic.notes {
            let _ = writeln!(out, "{pad} {BOLD_BLUE}={RESET} note: {note}");
        }
        for help in &diagnostic.help {
            let _ = writeln!(out, "{pad} {BOLD_BLUE}={RESET} help: {help}");
        }
        for suggestion in &diagnostic.suggestions {
            let _ = writeln!(
                out,
                "{pad} {BOLD_BLUE}={RESET} help: {}",
                suggestion.message
            );
        }
    } else {
        let _ = writeln!(out, "{pad} |");
        for note in &diagnostic.notes {
            let _ = writeln!(out, "{pad} = note: {note}");
        }
        for help in &diagnostic.help {
            let _ = writeln!(out, "{pad} = help: {help}");
        }
        for suggestion in &diagnostic.suggestions {
            let _ = writeln!(out, "{pad} = help: {}", suggestion.message);
        }
    }
}
