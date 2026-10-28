# Task 0015: Conformance cases for types, inference, traits and generics (§5, §6)

## Context
Phase 2 implements name resolution, type checking and the reference interpreter
(`docs/adr/0019-semantic-analysis-and-interpreter.md`). The normative rules are in
`spec/loom-spec.md` §5 and §6; error codes are in Appendix C.4–C.5. The suite format is
`spec/conformance/README.md`: an accept case is `<name>.loom` + `<name>.stdout` (run with
`loom run`; optional `<name>.exit` with the expected exit status, e.g. `101` for a panic);
a reject case is `<name>.loom` + `<name>.diag` (`LINE:COL CODE` per line). Look at the
existing cases in the same directories for the style. Read AGENTS.md first.

## Goal
Every numbered rule of spec §5 and §6 has at least one accept case and, where the rule
can be violated, one reject case.

## Files you may change
- new files in `spec/conformance/05-types/`, `spec/conformance/06-types/`

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
6. Cover at least: every primitive type and its `Display` text (§7.12 rule 2), integer
   literal inference and defaults, E0310, E0282, coercions (`Never`, `Fn` kinds,
   `dyn Trait`), generic functions and types with bounds (E0320), associated types,
   derives (E0329), and trait objects. Target: at least 40 accept and 25 reject cases.

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
- Conventional Commit title: `test(conformance): types, inference, traits and generics (§5, §6)`.
