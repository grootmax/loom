# ADR 0016: Rust interop (`loom-bridge`, `loom-embed`, crates.io access)

- **Status:** Accepted (provisional — ABI details designed in Phase 5; that design is never delegated)
- **Date:** 2026-10-23
- **Deciders:** lead architect
- **Related:** [ADR 0005](0005-ownership-and-borrowing.md), [ADR 0010](0010-modules-and-packages.md), [ADR 0017](0017-rust-as-the-base.md)

## Context

Rust is Loom's base. The crates.io ecosystem (regex, serde, rustls, tokio-based clients,
image codecs…) should be usable from Loom from the first day of package support, and Rust
projects should be able to adopt Loom gradually.

## Decision

1. **Never depend on Rust's unstable ABI.** All interop goes through a stable C ABI shim.
2. **`loom-bridge`** (a Rust crate, Phase 5) provides `#[loom::export]` for Rust functions,
   structs and enums. Its build step generates:
   - a C-ABI shim (`extern "C"` functions, `#[repr(C)]` mirror types or opaque handles);
   - Loom declarations (`.loom` extern module) with idiomatic names and types.
3. **Automatic type mapping:**

   | Rust | Loom | Mechanism |
   |---|---|---|
   | integers, floats, `bool`, `char` | same | by value |
   | `String` / `&str` | `String` / borrowed `String` | (ptr, len, cap) triple; ownership moves across |
   | `Vec<T>` / `&[T]` | `Array[T]` / borrowed `Array[T]` | same layout contract as Loom arrays |
   | `Option<T>` / `Result<T, E>` | `Option[T]` / `Result[T, E]` | tagged unions with `#[repr(C)]` mirrors |
   | `#[repr(C)]`-able structs/enums | structs/enums | generated mirrors |
   | other types | opaque handle with `Drop` | boxed, methods exported individually |
   | `Arc<T>` | `Shared[T]` | same refcount semantics, via handle |

   Because Loom ownership *is* Rust ownership ([ADR 0005](0005-ownership-and-borrowing.md)),
   moves and borrows map directly. A borrowed Loom parameter becomes `&T` and an `own`
   parameter becomes `T`. Rust lifetimes that cannot be expressed in Loom (borrowed
   returns outside elision rules, borrowed fields) produce opaque handles or are skipped
   with a generated note.
4. **`loom add rust:<crate>@<version>`** adds the crate to `[dependencies.rust]`, then
   generates a thin bridge crate in the build directory that wraps the crate's public API
   (using rustdoc JSON as the API description), and invokes **Cargo under the hood**. Users
   never manage two build systems. Hand-written bridge annotations can refine generated bindings.
5. **`loom-embed`** (a Rust crate) lets Rust applications compile and call Loom code at
   build time (`build.rs`) and link it as a static library. Loom panics are converted to
   Rust panics at the boundary. The Loom runtime is initialized lazily on first call.
6. **Async Rust crates** are driven by a Loom-runtime-backed executor shim, so they look
   like blocking calls to Loom code ([ADR 0006](0006-concurrency-model.md)).
7. **C ABI FFI** (`extern "C"` declarations plus a header binding generator) is the
   general foundation; Rust interop is a high-level layer on top of it.

## What Rust does

Rust itself has no stable ABI and relies on `extern "C"`, `cbindgen` and `bindgen` for
cross-language work, plus newer projects such as `cxx`, `diplomat`, `uniffi` and
`crabi`/`stabby`. We follow `uniffi`/`diplomat` in shape: a stable C core plus generated
idiomatic bindings. We should evaluate reusing one of them in Phase 5 before building our own.

## Alternatives considered

- **Linking against Rust's ABI directly:** rejected. It is unstable between compiler versions.
- **Rust-emit backend as the interop mechanism:** see [ADR 0009](0009-backend-strategy.md).
  It would give trivial interop but costs compile speed and independence.

## Consequences

- Rust crates require a Rust toolchain at build time. `loom doctor` detects and explains this.
- Generated bindings for large crates need curation. Phase 5's exit criterion asks for
  5 popular crates working, which is a deliberately modest bar.
- Rustdoc JSON is unstable. Pin its version, and keep the generator tolerant of format changes.
