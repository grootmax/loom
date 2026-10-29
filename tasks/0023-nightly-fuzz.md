# Task 0023: Nightly fuzzing workflow

## Context
`tools/loom_fuzz` is the front-end fuzzer (`cargo run --release -p loom_fuzz -- --help`).
Phase 1's exit criterion needs long fuzz runs, and ADR 0018 §9 plans a nightly workflow
(4 jobs × 6 hours). Read AGENTS.md, `tools/loom_fuzz/src/` and `.github/workflows/ci.yml`.

## Goal
A scheduled GitHub Actions workflow fuzzes lexer, parser and formatter every night and
reports minimized failures.

## Files you may change
- new file `.github/workflows/fuzz.yml`

## Files you must NOT change
- everything else

## Requirements
1. Triggers: `schedule` (nightly, 02:00 UTC) and `workflow_dispatch` with an input
   `seconds` (default 21000).
2. A matrix of 4 jobs on `ubuntu-latest` with distinct seeds derived from the run id and
   the matrix index; each runs `cargo run --release -p loom_fuzz -- --target all --seconds
   <seconds> --seed <seed> --out target/fuzz` with `timeout-minutes: 360`.
3. On failure, upload `target/fuzz/` as an artifact named `fuzz-crashers-<index>`, and the
   job fails. Print the seed in the job summary (`$GITHUB_STEP_SUMMARY`).
4. Use the same toolchain setup and caching steps as `ci.yml`; pin actions as `ci.yml`
   does.

## Acceptance
- `actionlint` (if available) reports nothing; the workflow runs successfully when
  dispatched manually with `seconds: 60`.
- `git diff --stat origin/main` shows only `.github/workflows/fuzz.yml`.

## Notes
- Follow AGENTS.md. Conventional Commit title: `ci: nightly fuzzing of the front end`.
