# ADR 0011: Editions and compatibility

- **Status:** Accepted
- **Date:** 2026-10-23
- **Deciders:** lead architect (promise text requires owner approval before 1.0)
- **Related:** [docs/compatibility.md](../compatibility.md)

## Context

A 100-year language must be able to change without breaking the code written in it.
Python 2→3 shows the cost of a split ecosystem. Rust editions and Go's compatibility
promise show two halves of a working answer.

## Decision

1. **Editions** (Rust model). Every package declares `edition = "YYYY"` in `loom.toml`.
   Breaking syntax or semantic changes happen only in a new edition. Packages of different
   editions link together forever, because editions change surface rules, not the ABI or
   the meaning of types across the boundary.
2. **Cadence:** at most one edition every 3 years. The first edition is named for the
   year of Loom 1.0 (working assumption: **2027**). Pre-1.0 code uses `edition = "preview"`,
   which carries no guarantees.
3. **Compatibility promise** (Go model): after 1.0, valid programs keep compiling and
   behaving the same within an edition. The exceptions are listed in `docs/compatibility.md`
   (soundness fixes, unspecified behavior, `unsafe` UB, `x/` packages).
4. **Deprecation:** warn for at least one full edition before removal. Removal happens only
   at edition boundaries, and every deprecation ships with a `loom fix` rewrite.
5. **Migration tooling:** `loom fix --edition YYYY` rewrites code mechanically. An edition
   change that cannot be automated needs extraordinary justification in its RFC.
6. **The spec is versioned per edition**: `spec/loom-spec.md` describes the current edition
   and notes earlier-edition differences inline.
7. **Std stability:** std items carry stability attributes (`@[stable(since: "1.0")]`,
   `@[unstable(feature: …)]`). Unstable items are usable only behind a feature gate on
   pre-release toolchains.

## What Rust does

Rust editions (2015, 2018, 2021, 2024) with `cargo fix --edition`, plus a stability
promise with documented exceptions. Loom adopts this design whole. From Go we add a
*written* compatibility document and a stated cadence ceiling.

## Alternatives considered

- **No editions, never break (C, Go):** rejected. Mistakes then accumulate forever.
- **Major-version language breaks (Python 3, Perl 6):** rejected. They split the ecosystem.

## Consequences

- The compiler must carry per-edition logic for its whole life. Keep edition differences
  small and confined to the front end.
- Conformance tests are tagged by edition.
