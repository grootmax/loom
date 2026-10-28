# Task 0009: Diagnostic renderer (rustc-style snippets)

## Context
`compiler/loom_diagnostics` holds the diagnostic data model (`Diagnostic`, `Label`,
`Suggestion`, `Severity`, `DiagnosticId`; read `src/lib.rs`). Positions come from
`loom_span` (`SourceMap`, `SourceFile`, `Span`, `LineColumn`; read
`compiler/loom_span/src/lib.rs`). Today diagnostics only have a one-line form
(`Diagnostic::short`). ADR 0008 §6 and ADR 0018 §3 call for an in-house renderer that
looks like rustc's. Read AGENTS.md first.

## Goal
`loom_diagnostics::render::render(&diagnostic, &sources, &options)` produces the
snippet format below, exactly, with optional ANSI color.

## Files you may change
- new file `compiler/loom_diagnostics/src/render.rs`
- `compiler/loom_diagnostics/src/lib.rs`: **only** add `pub mod render;` next to `pub mod codes;`
- new files under `compiler/loom_diagnostics/tests/`
- `compiler/loom_diagnostics/Cargo.toml`: only to add
  `[dev-dependencies] loom_snapshot = { path = "../loom_snapshot" }` if you use it

## Files you must NOT change
- `spec/`, `docs/adr/`, `CLAUDE.md`, `AGENTS.md`, `Cargo.lock` except the dev-dependency
  change, every other crate, and everything else not listed

## Requirements
1. Public API (exact names):
   ```rust
   pub struct RenderOptions { pub color: bool }   // derive Clone, Copy, Debug, Default
   pub fn render(diagnostic: &Diagnostic, sources: &SourceMap, options: &RenderOptions) -> String
   ```
2. **Format** for a diagnostic whose file is in `sources` (no color). Every line ends with `\n`:
   ```text
   error[E0007]: `!` is not an operator
    --> src/main.loom:2:6
     |
   2 |   if !ready
     |      ^ not an operator
     |
     = note: first note
     = help: use `not` for logical negation
     = help: use `not`
   ```
   - Line 1: `{severity}[{id}]: {message}`. `severity` is `error` or `warning`, and `id` is
     the code or lint name (`Display` of `DiagnosticId`).
   - Line 2: `{pad}--> {file name}:{line}:{column}` of the primary label's start, where
     `pad` is `width` spaces and `width` is the number of digits of the largest line
     number shown in the snippet.
   - Then `{pad} |`, then each source line shown as `{line number right-aligned to width} | {text}`,
     followed by its underline rows `{pad} | {spaces}{markers} {label}`. Drop the trailing
     space when the label is empty.
   - Primary label markers are `^`, secondary label markers are `-`. A marker row starts
     at the label's start column. Its length is the number of characters covered on that
     line, at least 1. An empty span, or a span at the end of a line or file, gets one
     marker at that position.
   - A span that covers several lines is underlined on its first line only, from its
     start to the end of that line. Append ` ...` before the label text.
   - Lines shown are the lines of every label in the same file as the primary, in line
     order, each once. If two shown lines are more than one line apart, put a row
     `...` between them, with no gutter and no `|`. Labels in other files are ignored
     (multi-file rendering is out of scope).
   - Several labels on the same line get one underline row each, ordered by start
     column.
   - After the snippet: if there are notes, help lines or suggestions, a `{pad} |` line,
     then `{pad} = note: …` for each note, `{pad} = help: …` for each help line, then
     `{pad} = help: {suggestion.message}` for each suggestion.
   - **Tabs** in a shown source line are printed as 4 spaces. Marker columns account for
     that, so the markers still sit under the right characters.
   - Columns count Unicode scalar values (`LineColumn` already does). Wide characters (CJK,
     emoji) count as 1. No grapheme or width handling.
   - Trailing whitespace of source lines is removed. Every line ends with `\n`, and the
     output ends with exactly one `\n`.
3. If the primary span's file is not in `sources` (for example `FileId::DETACHED`), render
   only the header line and the note, help and suggestion lines (with `pad` = 1).
4. **Color** (`options.color == true`) uses ANSI SGR codes, with a reset `\x1b[0m` after
   each colored piece: the severity and `[id]` are bold red (`\x1b[1;31m`) for errors and
   bold yellow (`\x1b[1;33m`) for warnings, the message is bold (`\x1b[1m`), the gutter
   (`-->`, `|`, line numbers, `=`) is bold blue (`\x1b[1;34m`), `^` markers and their label
   use the severity color, and `-` markers and their label use bold blue. With color off,
   the output contains no escape characters.
5. Tests (table-driven, comparing full expected strings) covering: a single primary label;
   notes, help and suggestions; a secondary label on another line, with the `...` gap
   row; a secondary label on the same line; a multi-line span; a tab before the span;
   a span at end of file; a warning; a detached file; two-digit line numbers (gutter
   width); color on, checking the exact escape sequences of one small case.
6. Doc comments on public items. No dependencies. Clippy pedantic clean.

## Acceptance
- `cargo xtask ci` passes.
- `cargo test -p loom_diagnostics` runs the new tests.
- `git diff --stat origin/main` shows only the allowed paths.

## Out of scope
- Using the renderer from the CLI (the maintainer wires it).
- Rendering suggestions as edited code.

## Notes
- Follow AGENTS.md. No new dependencies. Never weaken or delete existing tests.
- Conventional Commit title: `feat(diagnostics): add the rustc-style renderer`.
