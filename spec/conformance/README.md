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
   conformance; error codes and primary positions are. Warnings are not listed.
4. Tests must be deterministic: no time, randomness, network, or environment dependence.
5. Tests are tagged by edition through their directory (`edition-preview/` once there are
   several editions).

## Running

`cargo xtask conformance` runs the suite against this repository's implementation.
In Phase 1 only the front end exists, so the runner checks:

- **reject cases** (`.diag`) whose codes are all lexical or syntax codes (E0001–E0199):
  the reported codes and positions must equal the file exactly;
- **accept cases** (`.stdout`): the program must produce no lexical or syntax
  diagnostics. Its output is checked from Phase 2, when the interpreter exists.

Cases with codes from later phases are skipped until their phase.
