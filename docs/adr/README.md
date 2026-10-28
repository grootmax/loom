# Architecture Decision Records

An ADR records one significant decision: its context, the decision, what Rust does, the
alternatives we rejected, and the consequences. ADRs are how a future maintainer who never
met us learns *why* Loom is the way it is ([ADR 0001](0001-record-decisions-as-adrs.md)).

- Copy [template.md](template.md) to `NNNN-short-title.md` with the next free number.
- ADRs are immutable once accepted, except for status, typo fixes and links. To change a
  decision, write a new ADR that supersedes the old one and update the old one's status.
- Statuses: *Proposed*, *Accepted*, *Accepted (provisional)* (may be revisited at the end of
  the phase that implements it), *Proposed — owner decision* (waiting for the project owner),
  *Superseded by NNNN*, *Deprecated*.

| # | Title | Status |
|---|---|---|
| [0001](0001-record-decisions-as-adrs.md) | Record decisions as ADRs | Accepted |
| [0002](0002-implementation-language-and-toolchain.md) | Implementation language and toolchain policy | Accepted |
| [0003](0003-syntax-philosophy.md) | Syntax philosophy | Accepted (provisional) |
| [0004](0004-type-system-and-inference.md) | Type system and inference boundaries | Accepted (provisional) |
| [0005](0005-ownership-and-borrowing.md) | Ownership and borrowing model | Accepted (provisional); owner decisions recorded 2026-10-29 |
| [0006](0006-concurrency-model.md) | Concurrency model | Accepted (provisional) |
| [0007](0007-error-handling.md) | Error handling | Accepted |
| [0008](0008-compiler-architecture.md) | Compiler architecture | Accepted |
| [0009](0009-backend-strategy.md) | Backend strategy | Accepted (Rust-emit deferred by the owner) |
| [0010](0010-modules-and-packages.md) | Module and package system | Accepted (provisional) |
| [0011](0011-editions-and-compatibility.md) | Editions and compatibility | Accepted |
| [0012](0012-platform-tiers.md) | Platform tiers | Accepted |
| [0013](0013-licensing-and-governance.md) | Licensing and governance | Accepted |
| [0014](0014-name-and-trademark-risk.md) | Name and trademark risk | Proposed — owner decision |
| [0015](0015-agent-workflow.md) | Agent workflow (Claude Code + Jules) | Accepted |
| [0016](0016-rust-interop.md) | Rust interop | Accepted (provisional) |
| [0017](0017-rust-as-the-base.md) | Rust as the base: matches and deliberate differences | Accepted (living document) |
| [0018](0018-front-end-design.md) | Front-end design (Phase 1) | Accepted (provisional) |
| [0019](0019-semantic-analysis-and-interpreter.md) | Semantic analysis and the reference interpreter (Phase 2) | Accepted (provisional) |
