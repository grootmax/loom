# ADR 0002: Implementation language and toolchain policy

- **Status:** Accepted
- **Date:** 2026-10-23
- **Deciders:** lead architect
- **Related:** [ADR 0017](0017-rust-as-the-base.md), [docs/dependencies.md](../dependencies.md)

## Context

The toolchain needs to be fast, memory-safe, portable to every Tier 1 target, and
maintainable for decades. Loom's semantics are derived from Rust, and Rust crate interop is
a core goal.

## Decision

1. **Language:** the compiler, runtime, native std layer, package manager, formatter,
   linter, language server and repository automation (`xtask`) are written in **stable Rust**.
   Nightly features are never used, not even behind cfgs.
2. **Edition:** Rust 2024 for all crates.
3. **Pinned toolchain:** `rust-toolchain.toml` pins an exact stable version (currently 1.99.1).
   CI and contributors use exactly that version.
4. **MSRV policy:** the minimum supported Rust version *is* the pinned version
   (`rust-version` in `Cargo.toml`). Loom is an application toolchain, not a library, so
   supporting older compilers buys nothing. The pin is bumped deliberately, by PR, at most
   once per Rust release and at least every 6 months. Each bump includes a `cargo xtask ci`
   run and a changelog entry.
5. **Safety:** `unsafe_code = "forbid"` in every crate via `[workspace.lints]`, except
   `runtime/` and future FFI shims. Those use `deny`, so each `unsafe` block needs a local
   `#[allow(unsafe_code)]` and a `// SAFETY:` comment (enforced by
   `clippy::undocumented_unsafe_blocks`).
6. **Quality gates:** `cargo fmt --check`, `cargo clippy -D warnings` (`all` + `pedantic`),
   tests, `cargo doc` with `-D warnings`, and `cargo deny check`. All of them run through
   `cargo xtask ci`, which is identical locally and in CI.
7. **Dependencies:** zero by default. Each one is justified in `docs/dependencies.md` with an
   exit plan. `xtask` uses only std.
8. **Workspace layout:** one crate per pipeline stage (`compiler/loom_*`), mirroring rustc,
   so boundaries are enforced by the module system, not by convention.

## What Rust does

rustc is self-hosted and bootstraps from a previous release. It pins its own toolchain
(`src/stage0`) and allows internal unstable features. Loom cannot depend on nightly, so it
uses only stable Rust. That keeps the bootstrap story simple: any released Rust compiler at
the pinned version builds Loom.

## Alternatives considered

- **C++ or C (like early LLVM-based languages):** rejected. They are memory-unsafe, have
  no package ecosystem, and give no semantic kinship with Loom.
- **Go:** fast builds and a simple runtime, but a GC'd implementation of an ownership
  language, and Rust interop would be second-class.
- **OCaml/Haskell:** excellent for compilers, but a small contributor pool, and a poor fit
  for the runtime and for Rust interop.
- **Self-hosting from the start:** rejected. It is too early, because the language does not
  exist yet. Self-hosting is a separate, later ADR.
- **A floating `stable` toolchain:** rejected. Builds would become non-reproducible, and CI
  could break spontaneously when a new Rust release adds lints.

## Consequences

- Contributors need rustup. It installs the pinned toolchain automatically.
- New Rust lints arrive only when we bump the pin, never by surprise.
- Pedantic clippy causes some friction. A lint may be allowed locally, with a comment.
