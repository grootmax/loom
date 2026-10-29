# Changelog

All notable changes to the Loom toolchain are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the toolchain uses
[Semantic Versioning](https://semver.org/). Language changes are tracked by edition
(see [docs/compatibility.md](docs/compatibility.md)).

## [Unreleased]

### Added
- Phase 3 part 1: `ref T` types, MIR (`loom check --dump=mir`) and the ownership and
  borrow checker (E0500–E0504), ADR 0020 and normative spec §8.
- Phase reports for Phases 1, 2 and 3 (`docs/reports/`).

### Changed
- Programs that return a field of a borrowed value now need `.clone` (spec §8); `Option`,
  `Result` and `Range` are `Copy` when their contents are.

### Fixed
- The interpreter now copies the captures of escaping closures (spec §8.4 rule 1); an
  assignment inside such a closure no longer changes the outer binding.
- MIR: a borrow returned by a call and passed on in a borrow position is reborrowed, not
  copied into a temporary; temporaries of expression bodies and of statements left early
  (`return`, `?`, `break`, `next`) and default argument values are dropped.
- `loom fmt`: a line break inside brackets after which a `.` is followed by a space no
  longer changes indentation on the second run (found by fuzzing).

### Added
- Phase 2: ADR 0019 (semantic analysis and the interpreter) and normative spec text for
  §3–§7, §9, §10, §12 and §14; HIR, name resolution, type inference and checking, the
  core library in Loom (`std/core/`), and the reference interpreter.
- `loom run` and `loom test`; `loom check` reports name and type errors and accepts
  `--dump=hir`; `cargo xtask examples`; conformance accept cases now run.
- Phase 1 begins: ADR 0018 (front-end design), spec Appendix C (diagnostic codes) and
  disambiguation rules, `loom_span` (spans, line/column mapping, source map), and the
  `loom_diagnostics` data model and code registry.
- Lexer, lossless CST, parser with error recovery, typed AST, and the canonical formatter.
- `loom check`, `loom fmt [--check]`, `loom explain`, and `cargo xtask conformance`.
- Phase 0 foundations: repository layout, Cargo workspace with stub crates, `cargo xtask ci`,
  governance and contribution docs, ADRs 0001–0017, specification skeleton and grammar,
  example programs, CI on Linux/macOS/Windows, VS Code syntax highlighting, and the
  agent delegation workflow (`tasks/`).
- CI on all Tier 1 hosts including Linux aarch64, macOS x86_64 and musl (#2).
- `cargo xtask spec-check`, run as part of `cargo xtask ci` (#4).
- GitHub issue forms for bugs, RFC ideas and documentation (#3).
