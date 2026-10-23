# AGENTS.md — instructions for coding agents (Jules and others)

You are contributing to **Loom**, a programming language implemented in Rust. A human-
accountable maintainer reviews every change as if it came from an untrusted contributor.
Changes that break these rules are rejected.

## Setup

```sh
# The toolchain is pinned in rust-toolchain.toml; rustup installs it automatically.
rustup toolchain install
cargo install cargo-deny --locked     # if not already installed
cargo xtask ci                        # must pass before you finish
```

## Hard rules

1. **`cargo xtask ci` must pass.** It runs `cargo fmt --check`, `cargo clippy -D warnings`,
   the test suite, `cargo doc` with `-D warnings`, and `cargo deny check`.
2. **Only change the files your task lists** under "Files you may change". If you believe
   another file must change, stop and say so in your summary instead of changing it.
3. **Never change these paths** unless your task explicitly lists them: `spec/`,
   `docs/adr/`, `docs/rfcs/`, `CLAUDE.md`, `AGENTS.md`, `GOVERNANCE.md`, `LICENSE-*`,
   `rust-toolchain.toml`, `deny.toml`, `Cargo.lock`, `.github/CODEOWNERS`.
4. **Never weaken or delete tests**, and never mark them ignored, to make CI pass.
5. **Never add dependencies** (Rust crates, GitHub Actions, npm packages, tools) unless
   your task says so explicitly.
6. **The spec is the source of truth.** If the task and `spec/loom-spec.md` disagree,
   follow the spec and point out the conflict in your summary.
7. **No `unsafe`** anywhere. All crates except `runtime/` forbid it.
8. **Keep it small:** about 400 changed lines at most. If the task needs more, finish a
   coherent part and explain what remains.
9. **Never commit secrets**, tokens, or machine-specific paths.

## Coding standards

- Stable Rust, edition 2024. Code formatted with the default `rustfmt`.
- Clippy `all` and `pedantic` are enabled. Fix the lint rather than allowing it. If an
  allow is truly needed, put it on the smallest scope with a comment explaining why.
- Public items have doc comments. Use full words in names (no abbreviations).
- Every behavior change comes with tests. Prefer table-driven tests with clear names.
- Match the style of the surrounding code: comment density, naming, idioms.

## Commits and PRs

- **Conventional Commits:** `feat(scope): …`, `fix(scope): …`, `test(scope): …`, `docs(scope): …`,
  `ci: …`, `chore: …`. The scope is the crate or area (`lexer`, `xtask`, `ci`).
- Sign off every commit (`git commit -s`).
- The PR description states which task file (`tasks/NNNN-*.md`) it implements and lists
  each acceptance criterion with how it was verified.

## Learned rules

Rules added after reviewing agent work. Each one cites the task where the problem appeared.

*(none yet)*
