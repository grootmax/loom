# ADR 0013: Licensing and governance

- **Status:** Accepted
- **Date:** 2026-10-23
- **Deciders:** project owner (license and governance model per kickoff §16), lead architect
- **Related:** [GOVERNANCE.md](../../GOVERNANCE.md), [CONTRIBUTING.md](../../CONTRIBUTING.md)

## Context

A language outlives its creators only if anyone can use, fork and maintain it, and if
decision-making doesn't depend on one person or company.

## Decision

1. **License:** dual MIT OR Apache-2.0 for all code, docs and the spec. The same applies to
   std, so Loom programs carry no copyleft obligations.
2. **Contributions:** the Developer Certificate of Origin (`git commit -s`), no CLA.
   Contributions are inbound=outbound under the dual license.
3. **Governance:** owner-led (BDFL) now; a core team with an RFC voting process before 1.0;
   a neutral foundation or fiscal host after 1.0 holding the trademark and infrastructure
   (detailed in GOVERNANCE.md). Changing governance needs an ADR.
4. **AI-authored contributions** (Claude Code, Jules, others) are accepted under the same
   rules. The human or maintainer who merges them is accountable, and the commit carries a
   `Co-Authored-By` trailer naming the agent ([ADR 0015](0015-agent-workflow.md)).
5. **Third-party code** is vendored only with a compatible license recorded in `deny.toml`
   and `docs/dependencies.md`.

## What Rust does

Rust is MIT/Apache-2.0 dual-licensed, uses no CLA, and evolved from Mozilla stewardship
to the Rust Foundation and project teams. Loom follows the same path from the start.
Apache-2.0 provides a patent grant, and MIT provides GPLv2 compatibility.

## Alternatives considered

- **MPL/GPL/LGPL:** rejected. Copyleft in std would burden every Loom program.
- **CLA:** rejected. It adds contributor friction and concentrates relicensing power.
- **Apache-2.0 only:** rejected. Incompatible with GPLv2-only projects.

## Consequences

- License headers are not required per file. The root `LICENSE-*` files apply.
- A trademark policy is needed once the name is final ([ADR 0014](0014-name-and-trademark-risk.md)).
