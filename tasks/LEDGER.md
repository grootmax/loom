# Delegation ledger

Every task delegated to an agent, with its outcome. Newest last.
Status values: `dispatched`, `in review`, `merged`, `rejected`, `superseded`.

| Task | Agent | Session / task ID | Paths | Dispatched | Status | PR | Notes |
|---|---|---|---|---|---|---|---|
| [0001](0001-ci-tier1-matrix.md) | Jules | 96cc89ee-6ac5-4a6b-8c6c-e04e3ed69921 | `.github/workflows/ci.yml` | 2026-10-23 | merged | #2 | Phase 0 pilot. As specified; 7/7 CI jobs green |
| [0002](0002-xtask-spec-check.md) | Jules | 120d618f-177b-445e-a8e4-cc85363f1feb | `xtask/src/` | 2026-10-23 | merged | #4 | Phase 0 pilot. Negative test verified; duplicated helper removed in follow-up |
| [0003](0003-issue-templates.md) | Jules | 61aa9dca-b639-4fa6-a5b8-26044cf5a9a4 | `.github/ISSUE_TEMPLATE/` | 2026-10-23 | merged | #3 | Phase 0 pilot. Maintainer created `rfc-idea`/`docs` labels |
