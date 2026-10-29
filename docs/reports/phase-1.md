# Phase 1 report: front end

- **Tag:** `v0.1.0-phase1` (commit `bd68ef5`, PR #16)
- **Dates:** 2026-10-27 to 2026-10-29
- **Design:** [ADR 0018](../adr/0018-front-end-design.md), spec Appendix C

## Exit criteria

| Criterion | Result |
|---|---|
| Parses all examples | Met. All 28 examples parse with no diagnostics (`cargo xtask examples`). |
| Parser fuzzed 24 h clean | Met. 24 CPU-hours (8 processes × 3 h, 1.48 billion mutated inputs) over lexer, parser and formatter. Two formatter bugs were found and fixed earlier in the run. The run ended with 0 crashes and 0 property failures. Eight "slow" reports all happened in the same second during a macOS maintenance wake from sleep; each input re-runs in under 10 ms. The in-house fuzzer is `tools/loom_fuzz` (#16), and a nightly CI workflow is dispatched as task 0023. |
| Round-trip and formatter idempotence pass | Met. Every source re-prints byte for byte from the lossless CST. `fmt(fmt(x)) == fmt(x)` holds on examples, conformance cases and every fuzz input. |
| Diagnostics snapshot-tested | Met. Every grammar production and syntax error has a parser snapshot (#13), plus renderer snapshots (#15). |

## What was built

- `loom_span`, `loom_diagnostics` (data model, code registry, rustc-style renderer),
  `loom_lexer` (all of spec §2), `loom_syntax` (lossless CST), `loom_parser` (recursive
  descent and Pratt parsing with error recovery), `loom_ast` (typed AST and literal
  values), `loom_fmt` (canonical formatter).
- `loom check`, `loom fmt [--check]`, `loom explain <code>` with embedded explanations
  (#8, #14).
- `cargo xtask conformance` and `cargo xtask spec-check`; generated Unicode tables (#10).

## Delegation

Jules delivered #6–#10 and #13–#16. Every PR was reviewed as untrusted. Lessons are in
ADR 0015 and AGENTS.md "Learned rules".

## Bugs found by testing

- Pratt precedence: `1 + 2 * 3` parsed as `(1 + 2) * 3`; fixed with a conformance case.
- Formatter, found by fuzzing:
  - A newline inside brackets was treated as a statement boundary.
  - Re-printing could merge adjacent tokens (`.. =` became `..=`).
  - An indented `#!` became a shebang.

## Known limitations carried forward

- The formatter does not re-wrap long lines.
- Identifiers are not NFC-normalized.
