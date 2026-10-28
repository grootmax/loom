use std::fmt::Write as _;

use loom_span::FileId;

/// Parses `source` and renders its green CST `debug_dump()`, optionally followed by
/// a `--- diagnostics ---` section listing `{id} {range} {message}` per diagnostic.
#[must_use]
pub fn render(source: &str) -> String {
    let parse = loom_parser::parse(source, FileId::DETACHED);
    let mut out = parse.syntax().debug_dump();
    if !parse.diagnostics().is_empty() {
        out.push_str("--- diagnostics ---\n");
        for d in parse.diagnostics() {
            let _ = writeln!(out, "{} {} {}", d.id, d.primary.span.range, d.message);
        }
    }
    out
}
