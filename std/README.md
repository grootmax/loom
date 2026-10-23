# Loom standard library

`core` (no OS required) and `std` (OS services), written in Loom with minimal Rust shims.
Work starts in Phase 5 (see `docs/roadmap.md`). Scope and the rule for what enters std
are described in the kickoff and will be recorded in a Phase 5 ADR: something enters
`std` only if it is broadly needed, stable in shape, and we will maintain it for decades.
Everything else starts in `x/`.
