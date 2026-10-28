# Conformance suite

Implementation-independent test programs for the Loom specification. A future second
implementation of Loom should be able to run this suite unchanged.

## Layout

```
spec/conformance/
  <spec-section>/            e.g. 02-lexical/, 07-expressions/, 08-ownership/
    <name>.loom              the program
    <name>.stdout            expected standard output (accept case), or
    <name>.diag              expected diagnostics (reject case)
```

## Rules

1. Every numbered rule in `spec/loom-spec.md` has at least one **accept** case
   (`.stdout`) and one **reject** case (`.diag`) where rejection is possible.
2. The first line of every test is a comment naming the rule: `# spec: §2.7.1 rule 2`.
3. `.diag` files list one diagnostic per line as `LINE:COL CODE`, e.g. `3:5 E0012`, in
   source order. `LINE` and `COL` are 1-based; `COL` counts Unicode scalar values (a tab is
   one column). Codes are listed in spec Appendix C. Message text is **not** part of
   conformance; error codes and primary positions are. Warnings are not listed. A program
   with lexical or syntax errors lists only those (spec Appendix C).
4. Tests must be deterministic: no time, randomness, network, or environment dependence.
5. Tests are tagged by edition through their directory (`edition-preview/` once there are
   several editions).

## Running

`cargo xtask conformance` runs the suite against this repository's implementation.
In Phase 2 it checks:

- **reject cases** (`.diag`) whose codes are all implemented (E0001–E0499): the reported
  codes and positions must equal the file exactly;
- **accept cases** (`.stdout`): the program must check without errors, and `loom run`
  must write exactly the `.stdout` file to standard output and exit with status 0, or
  with the status in `<name>.exit` (for example `101` for a case that panics).

A case whose **second line** is `# phase: N`, with `N` later than the current phase, is
skipped, as is a reject case listing codes of a later phase (E0500 and above). Front-end
cases that are not complete programs (no `main`) belong in reject cases or must declare
the phase in which they become runnable.
