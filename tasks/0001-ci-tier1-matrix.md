# Task 0001: Extend CI to the remaining Tier 1 hosts

## Context
Loom's Tier 1 platforms (docs/adr/0012-platform-tiers.md) are Linux x86_64 and aarch64
(glibc and musl), macOS aarch64 and x86_64, and Windows x86_64 (MSVC). CI currently runs
`cargo xtask ci` only on `ubuntu-latest`, `macos-latest` (aarch64) and `windows-latest`
(see `.github/workflows/ci.yml`). The toolchain is pinned by `rust-toolchain.toml`, and
rustup installs it with `rustup toolchain install`. Read AGENTS.md first.

## Goal
CI builds and tests the workspace on every Tier 1 host and libc combination on every PR.

## Files you may change
- `.github/workflows/ci.yml`

## Files you must NOT change
- Everything else, in particular `rust-toolchain.toml`, `xtask/`, `Cargo.toml`, `Cargo.lock`.

## Requirements
1. Add `ubuntu-24.04-arm` (Linux aarch64, glibc) to the existing `ci` matrix, so it runs
   `cargo xtask ci` like the other hosts.
2. Add `macos-15-intel` (macOS x86_64) to the existing `ci` matrix.
3. Add a separate job `musl` on `ubuntu-latest` that:
   - installs the pinned toolchain (`rustup toolchain install`);
   - installs the musl target with `rustup target add x86_64-unknown-linux-musl`;
   - installs `musl-tools` with apt;
   - runs `cargo test --workspace --locked --target x86_64-unknown-linux-musl`.
4. Use only actions already used in `ci.yml` (`actions/checkout@v5`, `Swatinem/rust-cache@v2`,
   `taiki-e/install-action@v2`). Add no new actions.
5. Keep `fail-fast: false`, `permissions: contents: read`, and the existing jobs unchanged
   apart from the matrix additions.
6. Give every job a readable `name:` that includes the OS or target.

## Acceptance
- The workflow file is valid YAML (`python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml'))"`).
- All CI jobs on the pull request are green, including the 3 new ones.

## Out of scope
- Tier 2 targets (Windows aarch64, FreeBSD, wasm32-wasi).
- Release builds, caching changes, cross-compilation via `cross`.

## Notes
- Follow AGENTS.md. Conventional Commit title: `ci: extend matrix to Linux aarch64, macOS x86_64 and musl`.
- No new dependencies. Never weaken or remove existing checks.
