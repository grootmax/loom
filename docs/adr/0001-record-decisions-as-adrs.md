# ADR 0001: Record decisions as ADRs

- **Status:** Accepted
- **Date:** 2026-10-23
- **Deciders:** lead architect
- **Related:** [GOVERNANCE.md](../../GOVERNANCE.md), [RFC process](../rfcs/README.md)

## Context

Loom is meant to last for decades. Its maintainers in 2060 will not be able to ask today's
authors why something is the way it is. Code shows *what* was decided, never *why*, nor
what was rejected. Languages that lose this history end up re-arguing settled questions or
breaking things whose purpose nobody remembers.

## Decision

1. Every significant architectural or design decision is recorded as an Architecture
   Decision Record in `docs/adr/NNNN-short-title.md`, using [template.md](template.md).
2. "Significant" means any of: hard to reverse, affects more than one crate, affects users,
   affects the spec, adds a dependency on a core path, or was contested.
3. Every ADR has a **"What Rust does"** section, because Rust is Loom's base
   ([ADR 0017](0017-rust-as-the-base.md)).
4. Accepted ADRs are immutable. A changed decision gets a new ADR that supersedes the old one.
5. Language changes after Phase 0 go through [RFCs](../rfcs/README.md); an accepted RFC may
   also produce an ADR for its implementation strategy.
6. PRs that implement a decision link the ADR.

## What Rust does

Rust uses RFCs for language and library changes and the rustc-dev-guide for architecture.
Design history is also spread across GitHub issues, Zulip threads and meeting notes, which
makes it hard to rediscover. We keep the RFC process and add ADRs as one indexed place for
architecture decisions.

## Alternatives considered

- **Design docs in a wiki.** Rejected: wikis drift away from the code and are not versioned with it.
- **Only RFCs.** Rejected: too heavy for toolchain-internal decisions, which also need recording.
- **Commit messages and PR descriptions.** Rejected: not discoverable and not organized by topic.

## Consequences

- There is a small writing cost per decision. It is paid back the first time someone asks "why?".
- `docs/adr/README.md` must be kept up to date as an index.
- Reviewers should ask "where is the ADR?" for any PR that makes a significant decision without one.
