# Task 0017: Conformance cases for patterns and error handling (§9, §10, §14)

## Context
Phase 2 implements name resolution, type checking and the reference interpreter
(`docs/adr/0019-semantic-analysis-and-interpreter.md`). The normative rules are in
`spec/loom-spec.md` §9, §10 and §14; error codes are in Appendix C.4–C.5. The suite format is
`spec/conformance/README.md`: an accept case is `<name>.loom` + `<name>.stdout` (run with
`loom run`; optional `<name>.exit` with the expected exit status, e.g. `101` for a panic);
a reject case is `<name>.loom` + `<name>.diag` (`LINE:COL CODE` per line). Look at the
existing cases in the same directories for the style. Read AGENTS.md first.

## Goal
Every numbered rule of spec §9, §10 and §14 has at least one accept case and, where the rule
can be violated, one reject case.

## Files you may change
- new files in `spec/conformance/09-patterns/`, `spec/conformance/10-errors/`,
  `spec/conformance/14-execution/`

## Files you must NOT change
- every existing file (add new cases only), `spec/loom-spec.md`, `compiler/`, `std/`,
  everything else

## Requirements
1. First line of every case: `# spec: §N.M rule K` naming the rule tested.
2. Every accept case is a complete program with `def main`, deterministic, and its
   `.stdout` is exactly what the spec says it prints (work it out from the spec, then
   confirm with `cargo run -p loom_cli -- run <file>`).
3. Every reject case has exactly one error, at the position Appendix C names as "At".
   Avoid cascades: the rest of the program must be valid.
4. File names: `<section>_<short_description>.loom`, e.g. `7_8_break_with_value.loom`.
5. Keep each case small (3–30 lines) and focused on one rule.
6. Cover at least: every pattern form, capitalization (§9.1 rule 1), unqualified
   variants, or-patterns (E0402), duplicate bindings (E0403), shape errors (E0404),
   ranges (E0405), refutability (E0401), pattern tests (E0204), exhaustiveness (E0400),
   `?` on `Result` and `Option` with `From` conversion (E0313), panics (exit 101), and
   `main` returning `Result` (exit 1). Target: at least 35 accept and 20 reject cases.

## Acceptance
- `cargo xtask conformance` passes, with the new cases counted as passed (not skipped).
- `cargo xtask ci` passes.
- `git diff --stat origin/main` shows only new files in the allowed directories.

## Out of scope
- Ownership (§8), concurrency (§11), packages (§12.3).

## Notes
- Follow AGENTS.md (learned rule 4: run `loom check` on every `.loom` file you write; use
  only syntax the spec and the existing examples use). No new dependencies. Never weaken
  or delete existing tests.
- If the implementation disagrees with the spec, do **not** change the implementation and
  do not bend the test: list the case under "Suspected implementation bugs" in the PR
  description with the spec rule it follows, and leave the case out of the commit.
- Conventional Commit title: `test(conformance): patterns, error handling and execution (§9, §10, §14)`.
