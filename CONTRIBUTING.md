# Contributing to Loom

Thank you for helping. Loom aims to be a language people can still build on in a hundred
years, so we value clarity, stability and written reasoning over speed.

## Before you start

- Read [docs/principles.md](docs/principles.md). When pillars conflict, the order there decides.
- **The spec is the source of truth.** If code disagrees with [spec/loom-spec.md](spec/loom-spec.md),
  the code is wrong — unless the spec is deliberately amended through an ADR or RFC.
- Language changes (syntax, semantics, std API shape) need an [RFC](docs/rfcs/README.md).
  Architectural changes to the toolchain need an [ADR](docs/adr/README.md).

## Setup

```sh
# rustup installs the pinned toolchain from rust-toolchain.toml automatically.
cargo install cargo-deny --locked   # optional locally, required in CI
cargo xtask ci                      # must pass before you open a PR
```

## Pull requests

- **Small and focused.** Aim for changes a reviewer can read in 15 minutes (≈ 400 lines).
- **Reference your reason.** Every PR links an ADR, RFC, spec section, issue or `tasks/` file.
- **Conventional Commits** for titles: `feat(parser): parse closure parameters`,
  `fix(lexer): …`, `docs(adr): …`, `test(…)`, `refactor(…)`, `ci: …`, `chore: …`.
- **Squash-merge only.** The PR title becomes the commit message on `main`.
- **`main` is always green.** Never disable or weaken a test to make CI pass.
- **Dependencies:** adding one requires a justification line in
  [docs/dependencies.md](docs/dependencies.md) and reviewer approval. Prefer none.

## Developer Certificate of Origin

All commits must be signed off (`git commit -s`), certifying the
[Developer Certificate of Origin 1.1](https://developercertificate.org/): you wrote the
change or have the right to submit it under the project's licenses.

## Code standards

- Stable Rust only. `#![forbid(unsafe_code)]` everywhere except `runtime/` and FFI shims,
  where every `unsafe` block carries a `// SAFETY:` comment.
- `cargo fmt`, `cargo clippy -D warnings` and `cargo deny` are enforced by CI.
- Public APIs: small surface, full words (no abbreviations), documented with an example.
- Tests accompany behavior changes. Diagnostics and IR dumps are snapshot-tested.

## Licensing

By contributing you agree your work is dual-licensed under MIT and Apache-2.0, matching
the project, without additional terms.
