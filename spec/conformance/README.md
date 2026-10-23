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
3. `.diag` files list one diagnostic per line as `LINE:COL CODE`, e.g. `3:5 E0012`.
   Message text is **not** part of conformance; error codes and primary positions are.
4. Tests must be deterministic: no time, randomness, network, or environment dependence.
5. Tests are tagged by edition through their directory (`edition-preview/` once there are
   several editions).

The suite is empty in Phase 0. Phase 1 adds lexical and syntax cases; Phase 2 adds semantics.
