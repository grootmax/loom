# ADR 0012: Platform tiers

- **Status:** Accepted
- **Date:** 2026-10-23
- **Deciders:** lead architect
- **Related:** [ADR 0009](0009-backend-strategy.md), `.github/workflows/ci.yml`

## Context

Promising every platform equally means promising none. Users need to know what is tested,
and contributors need to know what blocks a release.

## Decision

| Tier | Targets | Promise |
|---|---|---|
| **1** — builds and full test suite on every PR | Linux x86_64 and aarch64 (glibc and musl), macOS aarch64 and x86_64, Windows x86_64 (MSVC) | A release blocker if broken |
| **2** — builds on every PR, tests nightly | Windows aarch64, FreeBSD x86_64, wasm32-wasi | Fixed before release when feasible |
| **3** — best effort | iOS aarch64, Android aarch64/x86_64, wasm32 (browser), RISC-V 64 Linux | Community-supported; promoted as mobile/web frameworks mature |

- Target names in `loom build --target` use Loom's short triples (`aarch64-linux-musl`,
  `x86_64-windows-msvc`, `aarch64-macos`), which map to Rust triples internally.
- Cross-compilation is a flag. Loom bundles what it needs (linker, libc for musl) by Phase 7.
- **Phase 0 reality:** CI runs Ubuntu x86_64, macOS aarch64 and Windows x86_64. musl,
  Linux aarch64 and macOS x86_64 jobs are added by task 0001 (`tasks/`). Until Loom
  produces binaries, "tests" means the toolchain's own test suite on each host.
- Release artifacts are signed, reproducible, and published with checksums and an SBOM (Phase 7).
- Promoting a target needs: CI capacity, a maintainer, and passing the full suite for 3 months.

## What Rust does

Rust has the same three-tier model with similar promises. We copy its structure and keep
the Tier 1 list short.

## Alternatives considered

- **Single tier:** dishonest; it degrades into best effort for everything.

## Consequences

- macOS x86_64 runners are getting scarce in hosted CI. If they disappear, macOS x86_64
  moves to "build + cross-test under Rosetta" and that change is recorded.
