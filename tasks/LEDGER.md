# Delegation ledger

Every task delegated to an agent, with its outcome. Newest last.
Status values: `dispatched`, `in review`, `merged`, `rejected`, `superseded`.

| Task | Agent | Session / task ID | Paths | Dispatched | Status | PR | Notes |
|---|---|---|---|---|---|---|---|
| [0001](0001-ci-tier1-matrix.md) | Jules | 96cc89ee-6ac5-4a6b-8c6c-e04e3ed69921 | `.github/workflows/ci.yml` | 2026-10-23 | merged | #2 | Phase 0 pilot. As specified; 7/7 CI jobs green |
| [0002](0002-xtask-spec-check.md) | Jules | 120d618f-177b-445e-a8e4-cc85363f1feb | `xtask/src/` | 2026-10-23 | merged | #4 | Phase 0 pilot. Negative test verified; duplicated helper removed in follow-up |
| [0003](0003-issue-templates.md) | Jules | 61aa9dca-b639-4fa6-a5b8-26044cf5a9a4 | `.github/ISSUE_TEMPLATE/` | 2026-10-23 | merged | #3 | Phase 0 pilot. Maintainer created `rfc-idea`/`docs` labels |
| [0004](0004-loom-snapshot.md) | Jules | 9c287ed6-c8bc-44c2-813c-42f89926a8ce | `compiler/loom_snapshot/` | 2026-10-28 | merged | #7 | Phase 1 |
| [0005](0005-loom-unicode.md) | Jules | 5ec07319-df78-43b9-b598-8addef87ab66 | `compiler/loom_unicode/`, `xtask/src/gen_unicode.rs` | 2026-10-28 | merged | #10 | Phase 1 |
| [0006](0006-conformance-lexical.md) | Jules | 4368b162-a38c-4521-9ba7-8d9a3e5dd480 | `spec/conformance/02-lexical/` | 2026-10-28 | merged | #6 | Phase 1; maintainer fixed a raw-string case and an E0022 position |
| [0007](0007-error-explanations.md) | Jules | 828cf13c-1558-4b83-97e0-874445ea1acc | `compiler/loom_diagnostics/explanations/` | 2026-10-28 | merged | #8 | Phase 1 |
| [0008](0008-conformance-syntax.md) | Jules | 443c3305-a541-422d-b371-3ee99209166e | `spec/conformance/{03,04,07,09}-*/` | 2026-10-28 | merged | #9 | Phase 1; maintainer fixed `var` fields, a standalone `do` closure, and five cases invalid under the phase-2 rules |
| [0013](0013-literal-values.md) | — | — | `compiler/loom_ast/src/literals.rs` | — | superseded | — | Implemented by the maintainer in Phase 2 (needed by HIR lowering before push access returned) |
| [0009](0009-diagnostic-renderer.md) | Jules | e2fb3f87-631d-476a-9c59-be770aec68ef | `compiler/loom_diagnostics/src/render.rs`, `tests/` | 2026-10-28 | dispatched | — | Phase 1 wave 2 |
| [0010](0010-loom-explain.md) | Jules | c72f3615-568b-4eea-8e6a-8053bb6e29bf | `compiler/loom_cli/src/explain.rs`, explanations | 2026-10-28 | dispatched | — | Phase 1 wave 2 |
| [0011](0011-parser-snapshot-tests.md) | Jules | 76ea4b7b-1b13-423a-8ee6-ce9ae525db0a | `compiler/loom_parser/tests/` | 2026-10-28 | dispatched | — | Phase 1 wave 2 |
| [0012](0012-loom-fuzz.md) | Jules | 54bc1897-89dd-49fb-9288-a188bc816d9d | `tools/loom_fuzz/` | 2026-10-28 | dispatched | — | Phase 1 wave 2 |
