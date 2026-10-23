# ADR 0010: Module and package system

- **Status:** Accepted (provisional — registry design deferred to a Phase 5 ADR)
- **Date:** 2026-10-23
- **Deciders:** lead architect
- **Related:** [spec §12](../../spec/loom-spec.md), [ADR 0016](0016-rust-interop.md)

## Context

Packages must be reproducible, simple to reason about, and resistant to dependency-solver
surprises. They must also integrate Rust crates ([ADR 0016](0016-rust-interop.md)).

## Decision

**Modules**

1. One file is one module. Directories form namespaces: `src/net/http.loom` is the module
   `net.http`. An inline `module name … end` is also allowed, mainly for tests.
2. Items are private to their module by default; `pub` exports them. `pub(package)` limits
   visibility to the current package.
3. Imports: `use std.fs.File`, `use std.io.{Reader, Writer}`, `use app.models.User as Model`.
   Glob imports are not allowed, except for the prelude, so every name's origin is visible.
4. Cyclic imports between modules of one package are allowed (as in Rust). Cyclic
   dependencies between packages are not.

**Packages**

5. The manifest is `loom.toml` and the lockfile `loom.lock`. Both are TOML, and the
   lockfile is committed for applications.
6. Versions are semantic versions. **Minimal version selection** (Go's MVS): the build
   uses, for each dependency, the *minimum* version that satisfies every requirement in
   the graph. There is no SAT solver. Results are deterministic and change only when a
   manifest changes.
7. Major versions are distinct packages (`foo` and `foo@2` can coexist), as with Go's semantic import versioning.
8. Until the registry exists (Phase 5 ADR), dependencies come from **git URLs (pinned by
   commit hash in the lockfile)** and **local paths**.
9. `loom vendor` copies all dependencies into `vendor/` for offline and reproducible builds.
10. Builds are **reproducible**: no timestamps, sorted inputs, path remapping, and checksums
    for every fetched artifact in `loom.lock`.
11. Rust crates are declared as `[dependencies.rust]` entries (`loom add rust:serde_json@1`)
    and resolved by Cargo under the hood ([ADR 0016](0016-rust-interop.md)).

```toml
[package]
name = "hello"
version = "0.1.0"
edition = "2027"

[dependencies]
json5 = { git = "https://example.com/json5.loom", version = "1.2.0" }

[dependencies.rust]
regex = "1.11"
```

## What Rust does

Cargo uses a SAT-like resolver that picks the *maximum* compatible versions, with a
lockfile to pin them. That gives freshness but makes resolution surprising and
non-local. Loom follows Go here: the dependency graph is the specification, and
upgrading is explicit (`loom update`). Rust's module visibility model (`pub`, crate-level
privacy) is kept, but with file-based modules and no `mod` declarations, so the filesystem is the module tree.

## Alternatives considered

- **Cargo-style max-version SAT resolution:** rejected (see above).
- **Node-style nested duplicates:** rejected. Bloat and duplicated types.
- **No package manager, vendoring only:** rejected. Poor developer experience.

## Consequences

- Library authors must keep minimum versions honest. CI tooling (`loom update --check-min`) is planned.
- Mixing MVS for Loom packages with Cargo resolution for Rust crates needs care. The
  Phase 5 ADR must specify how Cargo's output is pinned into `loom.lock`.
