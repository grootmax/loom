# Governance

Loom is meant to outlive any single person or company. This document describes how
decisions are made today and how that is expected to change. Changes to this document
require an ADR.

## Today: owner-led (pre-1.0)

- The **project owner** (@grootmax) is the final decision-maker (BDFL model).
- Day-to-day architecture decisions are made by the lead maintainer and recorded as
  [ADRs](docs/adr/). Anyone may challenge an ADR by opening an issue or a superseding ADR.
- Decisions reserved for the owner: the project name, design pillars and their order,
  phase order, the compatibility promise, licensing, repository visibility, and any spending.
- AI coding agents may author changes, but a human-accountable maintainer reviews and
  merges every change (see [ADR 0015](docs/adr/0015-agent-workflow.md)).

## Next: core team (target: before 1.0)

When at least three people have made sustained contributions, the owner appoints a
**core team**. Language changes then require an accepted [RFC](docs/rfcs/README.md):
a public comment period of at least 14 days, followed by a core-team decision by rough
consensus, with the owner breaking ties.

## Later: foundation (target: after 1.0, as adoption warrants)

Trademark, infrastructure, and funds move to a neutral non-profit foundation or fiscal
host. Technical decisions remain with teams (language, compiler, std, tooling,
frameworks), each with documented membership and decision rules. The BDFL role ends.

## Principles for all stages

- Decisions are written down, with reasoning, where anyone can read them.
- The spec, not any implementation, defines the language.
- No single company may hold a majority of seats on any decision body after the
  foundation stage.
- Conduct is governed by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
