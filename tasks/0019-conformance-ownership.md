# Task 0019: Conformance cases for ownership and borrowing (§8)

## Context
Phase 3 implements the ownership and borrow checker (`docs/adr/0020-mir-and-ownership-checking.md`,
spec §8 and §5.6; codes E0500–E0504 in Appendix C.6). The suite format is
`spec/conformance/README.md`: accept case `<name>.loom` + `<name>.stdout` (run with
`loom run`), reject case `<name>.loom` + `<name>.diag` (`LINE:COL CODE`). Read AGENTS.md,
spec §8 completely, and ADR 0020 §3–§4 first. Run `cargo run -p loom_cli -- check <file>`
on every case you write.

## Goal
Every numbered rule of spec §8.1–§8.4 and §8.3 rule 3 (borrow returns) has accept cases
showing what is allowed and reject cases showing each error code at its exact position.

## Files you may change
- new files in `spec/conformance/08-ownership/`

## Files you must NOT change
- everything else, including existing cases, `spec/loom-spec.md` and `compiler/`

## Requirements
1. First line `# spec: §8.N rule K`. Small, focused cases (3–30 lines), each reject case
   with exactly one error and an otherwise valid program.
2. Cover at least:
   - moves: binding, assignment, `own` argument and receiver, return, constructor field,
     array element, `for` over an iterator; use after move on one branch only (E0500);
     re-assigning a moved `var`; Copy types never move; partial moves of a field;
   - moving out of a borrowed parameter, `self`, an index, or a borrowing pattern
     binding (E0501), and the `.clone` fix as an accept case;
   - borrow returns: `-> ref T` from `self`, from the only borrowed non-Copy parameter,
     and E0504 with two or zero sources; using the result while mutating the source
     (E0502) and after the source's last use (accepted, non-lexical);
   - iterating a collection while mutating it (E0502); two arguments where one is `mut`
     (E0502); `list.push(list.length)` (accepted, two-phase);
   - escapes (E0503): a borrowed value stored in a struct field, returned as a borrow of a
     local, a by-reference closure passed to an `own` parameter;
   - closures: by-reference capture visible after the call, escaping closures returned from
     functions (by move), mutation through a captured `var`;
   - pattern binding modes (spec §8.3 rule 6): looking into an `Option` field and
     continuing to use it (accepted), moving out of an owned local in a `case` arm.
3. Target: at least 40 accept and 30 reject cases.
4. If you believe the implementation is wrong, do **not** change it: list the case under
   "Suspected checker bugs" in the PR description with the spec rule, and leave it out.

## Acceptance
- `cargo xtask conformance` passes with the new cases counted.
- `cargo xtask ci` passes.
- `git diff --stat origin/main` shows only new files in `spec/conformance/08-ownership/`.

## Notes
- Follow AGENTS.md. Never weaken or delete existing tests.
- Conventional Commit title: `test(conformance): ownership and borrowing (§8)`.
