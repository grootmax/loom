# Task 0004: `loom_snapshot`, an in-house snapshot-testing crate

## Context
Phase 1 snapshot-tests every diagnostic and every CST dump (ADR 0018 §8, in
`docs/adr/0018-front-end-design.md` once merged; the requirements below are complete on
their own). The dependency policy (`docs/dependencies.md`) is "prefer zero", so instead of
`insta` we write a small std-only helper. Test crates will use it as a dev-dependency:
`loom_snapshot = { path = "../loom_snapshot" }`. Read AGENTS.md first.

## Goal
A new workspace crate `compiler/loom_snapshot` providing `assert_snapshot!`, with file-based
snapshots, a bless mode and readable diffs.

## Files you may change
- new directory `compiler/loom_snapshot/` (Cargo.toml, `src/`, `tests/`)
- `Cargo.toml` (root): **only** add the line `"compiler/loom_snapshot",` to `members`,
  directly after `"compiler/loom_span",`
- `Cargo.lock`: only the change Cargo makes for the new crate

## Files you must NOT change
- `spec/`, `docs/adr/`, `docs/rfcs/`, `CLAUDE.md`, `AGENTS.md`, `rust-toolchain.toml`,
  `deny.toml`, every other crate, and everything else not listed above

## Requirements
1. `compiler/loom_snapshot/Cargo.toml` follows the other crates exactly (copy
   `compiler/loom_span/Cargo.toml` and change name and description to
   `"In-house snapshot testing helper (dev-dependency only)."`). No dependencies.
2. Public API (exact names):
   - `pub fn assert_snapshot_at(directory: &Path, name: &str, actual: &str)`, which
     compares `actual` with the file `<directory>/<name>.snap`.
   - `#[macro_export] macro_rules! assert_snapshot { ($name:expr, $actual:expr) => … }`, which
     calls `assert_snapshot_at` with
     `Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("snapshots")`. The `env!` must
     expand in the *calling* crate, so it has to be inside the macro.
3. **Snapshot file format:** the first line is the header
   `# loom-snapshot: <name>` and the second line is empty. After that comes `actual`
   exactly, and the file ends with exactly one `\n` (add it if `actual` doesn't end with
   one; when comparing, ignore exactly one trailing `\n`). Line endings are normalized:
   `\r\n` in the file is treated as `\n`, so a Windows checkout with autocrlf still passes.
4. **Behavior:**
   - If the file exists and matches, return.
   - If the environment variable `LOOM_BLESS` is `1`, write (or overwrite) the file
     (creating directories as needed) and return.
   - If the file is missing and the environment variable `CI` is **not** set, create it,
     print `loom_snapshot: created <path>` to stderr, and return.
   - Otherwise panic. The message names the snapshot path and includes a line diff
     (`-` expected / `+` actual, with 2 lines of context around each change, computed by
     a simple LCS over lines; quadratic is fine up to 2000 lines, and above that fall back
     to printing both texts). End the message with the hint
     `run with LOOM_BLESS=1 to update`.
5. `name` must match `[a-z0-9_\-/]+` (a `/` creates a subdirectory). Otherwise panic with a
   clear message. This keeps file names portable across operating systems.
6. Doc comments on all public items, including a crate-level example (as an `ignore`d or
   `no_run` doctest is fine, but prefer a real doctest using `assert_snapshot_at` with a
   temporary directory under `std::env::temp_dir()`).
7. Tests in `compiler/loom_snapshot/tests/` covering: a match; a mismatch panics, and the
   message contains the diff and the hint; bless mode overwrites; a missing file is created
   when `CI` is unset; a missing file panics when `CI` is set; CRLF normalization; an
   invalid name panics. Tests that change environment variables must not race. Put all
   environment-dependent cases in one `#[test]` function that runs them sequentially, or
   factor the logic into an internal function that takes the two flags as parameters and
   test that instead (preferred). Use unique temporary directories.
8. Standard library only. Clippy `pedantic` with `-D warnings` must pass. No `unsafe`.
   Note: `std::env::set_var` is `unsafe` in edition 2024, so you must not call it. Use the
   preferred design from requirement 7.

## Acceptance
- `cargo xtask ci` passes.
- `cargo test -p loom_snapshot` runs the new tests.
- `git diff --stat origin/main` shows only `compiler/loom_snapshot/**`, `Cargo.toml`
  (one added line) and `Cargo.lock`.

## Out of scope
- Using the crate from other crates (later tasks do that).
- Inline snapshots, redactions, glob tests.

## Notes
- Follow AGENTS.md. No new dependencies. Never weaken or delete existing tests.
- Conventional Commit title: `feat(snapshot): add in-house snapshot testing crate`.
