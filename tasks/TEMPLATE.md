# Task NNNN: <title>

## Context
3–6 sentences. Link the ADR/spec sections to read first.

## Goal
One sentence, observable outcome.

## Files you may change
- explicit paths or globs

## Files you must NOT change
- `spec/`, `docs/adr/`, `docs/rfcs/`, `CLAUDE.md`, `AGENTS.md`, `rust-toolchain.toml`,
  `deny.toml`, `Cargo.lock` (unless listed above), everything else not listed above

## Requirements
1. Numbered, testable requirements.

## Acceptance
- Exact commands that must pass, e.g. `cargo xtask ci`.

## Out of scope
- What not to do.

## Notes
- Follow AGENTS.md. No new dependencies unless stated. Never weaken or delete tests.
