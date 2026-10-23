# ADR 0014: Name and trademark risk

- **Status:** Proposed — owner decision
- **Date:** 2026-10-23
- **Deciders:** project owner (final name and extension per kickoff §16)
- **Related:** [ADR 0013](0013-licensing-and-governance.md)

## Context

"Loom" is the working name. A programming language name must be searchable, unambiguous in
its ecosystem, and legally usable for decades, including a future foundation's trademark.

## Known collisions (from desk research; not legal advice)

| Collision | Domain | Severity for us |
|---|---|---|
| **Project Loom** (OpenJDK) | Java virtual threads, the *same topic* as Loom's concurrency story | **High** for search and confusion: "loom concurrency" and "loom virtual threads" already mean Java |
| **`loom` crate** (tokio-rs) | A Rust concurrency **model checker**, which our own runtime plans to use for testing (ADR 0006) | **High inside the Rust ecosystem**: crates.io name taken; `loom` in `Cargo.toml` already means something to Rust developers; awkward internal naming |
| **Loom** (video messaging, acquired by Atlassian) | SaaS product with an active trademark | **Medium**: different class of goods, but strong brand and legal resources |
| Other "loom" uses (weaving software, AI-model tools, game engines) | Various | Low–medium; hurts search |

The `.loom` extension does not appear to be registered by a widely used tool, but this is
not verified for all ecosystems (GitHub Linguist has no entry).

## Decision (proposed)

1. **Keep "Loom" as a working name through Phase 1.** Renaming is cheap now and gets more
   expensive every phase. A **decision is needed before Phase 5** (packages and registry
   publish names) and absolutely before any public announcement.
2. Mitigations while the name stands:
   - Internal crates use the `loom_` prefix and are `publish = false`. No crate is ever
     published as plain `loom`.
   - Docs say "the Loom programming language" in the first mention.
3. **Name check checklist** (no spending without owner approval): a USPTO/EUIPO/WIPO search
   in class 9 (software) and class 42; crates.io, npm, PyPI, Homebrew and GitHub org
   availability; domain availability (`loom-lang.org`, `.dev`); a search-engine collision
   test; Linguist and editor extension IDs; and pronunciation and meaning in major languages.
4. If renaming: choose a short, pronounceable, unique word with a free crates.io name,
   GitHub org and `.dev` domain. The file extension follows the name.

## What Rust does

Rust's name was chosen early, and it collides with a video game ("Rust", 2013), which
mostly causes search noise, not legal trouble. The Rust Foundation now holds the
trademark and has a published trademark policy, which is the model for our foundation stage.

## Alternatives considered

- **Rename now:** the owner has not chosen a name yet. Picking one unilaterally is out of scope (§16).
- **Ignore the risk:** rejected. The Project Loom and `loom` crate collisions are directly
  in our topic area and ecosystem.

## Consequences

- ⚑ **Owner decision needed:** keep "Loom" (accepting the collisions above) or pick a
  new name before Phase 5. If you rename, a mechanical rename is cheap until Phase 5; the
  crate prefix, extension, CLI name and docs change together.
