# Task 0010: `loom explain` with embedded, verified explanations

## Context
Each error code has a Markdown explanation in `compiler/loom_diagnostics/explanations/EXXXX.md`
(format in that directory's `README.md`). The registry of codes is
`loom_diagnostics::codes::ALL` (each `Code` has `id` and `title`). `loom explain EXXXX`
lives in `compiler/loom_cli/src/main.rs` and currently prints only the title. The parser
`loom_parser::parse(text, FileId::DETACHED)` returns a `Parse` whose `diagnostics()` have
`.code()` (an `Option<Code>`). Read AGENTS.md first.

## Goal
`loom explain E0007` prints the full explanation embedded in the binary, and tests prove
every explanation is present, well-formed, and that its examples really produce, or fix,
the error.

## Files you may change
- `compiler/loom_cli/src/main.rs` (only the `explain` function, and a `mod explain;` line)
- new file `compiler/loom_cli/src/explain.rs`
- new files under `compiler/loom_cli/tests/`
- `compiler/loom_cli/Cargo.toml`: only to add `loom_parser` and `loom_span` as
  dev-dependencies (path dependencies) if your tests need them
- `compiler/loom_diagnostics/explanations/*.md`: **only** to fix explanations that fail
  the checks in requirement 4 (see the known issues there)
- `Cargo.lock`: only the change Cargo makes for path dependencies

## Files you must NOT change
- `spec/`, `docs/adr/`, `CLAUDE.md`, `AGENTS.md`, every other crate, everything else

## Requirements
1. `compiler/loom_cli/src/explain.rs` defines
   `pub(crate) static EXPLANATIONS: &[(&str, &str)]`, one entry per code, written as
   `("E0001", include_str!("../../loom_diagnostics/explanations/E0001.md"))`, in code
   order, plus `pub(crate) fn explanation(id: &str) -> Option<&'static str>`.
2. `loom explain <code>` (case-insensitive) prints the Markdown text unchanged to stdout
   and exits 0. An unknown code prints
   `loom explain: \`X\` is not a Loom error code` to stderr and exits 1. A missing
   argument, or more than one, is a usage error (exit 2, as today).
3. Unit tests in `explain.rs`:
   - every code in `loom_diagnostics::codes::ALL` has an entry, and every entry is a code
     in `ALL` (no extras, no duplicates);
   - every `.md` file named `E*.md` in the explanations directory (read at test time via
     `env!("CARGO_MANIFEST_DIR")`) has an entry;
   - each explanation's first line is exactly `# {id}: {title}`, using the registry
     title, and it contains a `## Example` section and a `## How to fix it` section,
     each with a fenced code block marked `loom`.
4. **Example checks** (tests in `compiler/loom_cli/tests/explanations.rs` or in
   `explain.rs`). For each code, extract the first ```` ```loom ```` block after
   `## Example` and the first after `## How to fix it`:
   - parsing the example must report that code (`diagnostic.code().map(|c| c.id) == Some(id)`);
   - parsing the fix must report no errors.
   Skip E0001 (an invalid UTF-8 byte cannot appear in Markdown) and E0112 (too large to
   show), with a comment explaining why. **Fix any explanation file that fails**, by
   correcting its example or fix to follow the spec. Known issues to fix:
   - Many examples write `def main()` with empty parentheses. Loom style omits them
     (`def main`, see `examples/`). Change every zero-parameter `def name()` to `def name`.
   - E0109's fix assigns `@x = 0` in a non-`mut` method. Assigning to a field needs
     `def mut reset`.
   - E0002's example only *mentions* the bidi character in a comment. Either put the
     actual U+202E character in the example (it must then produce E0002), or
     construct the input in the test by appending `"\u{202E}"`, and say in the file
     that the character is invisible. Prefer the first, so the file shows a real case.
5. Doc comments on new items. Clippy pedantic clean.

## Acceptance
- `cargo xtask ci` passes.
- `cargo run -q -p loom_cli -- explain e0007` prints the E0007 file.
- `git diff --stat origin/main` shows only the allowed paths.

## Out of scope
- Rendering Markdown (print it raw), and explanations for codes that don't exist yet.

## Notes
- Follow AGENTS.md. No new dependencies. Never weaken or delete existing tests.
- Conventional Commit title: `feat(cli): embed and verify error explanations for loom explain`.
