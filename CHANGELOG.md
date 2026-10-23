# Changelog

All notable changes to the Loom toolchain are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the toolchain uses
[Semantic Versioning](https://semver.org/). Language changes are tracked by edition
(see [docs/compatibility.md](docs/compatibility.md)).

## [Unreleased]

### Added
- Phase 0 foundations: repository layout, Cargo workspace with stub crates, `cargo xtask ci`,
  governance and contribution docs, ADRs 0001–0017, specification skeleton and grammar,
  example programs, CI on Linux/macOS/Windows, VS Code syntax highlighting, and the
  agent delegation workflow (`tasks/`).
- CI on all Tier 1 hosts including Linux aarch64, macOS x86_64 and musl (#2).
- `cargo xtask spec-check`, run as part of `cargo xtask ci` (#4).
- GitHub issue forms for bugs, RFC ideas and documentation (#3).
