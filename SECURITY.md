# Security Policy

## Reporting a vulnerability

**Do not open a public issue.** Report vulnerabilities privately through GitHub's
[private vulnerability reporting](https://github.com/grootmax/loom/security/advisories/new).

Please include: affected component (compiler, runtime, std, tooling), version or commit,
a minimal reproduction, and the impact you believe it has.

We aim to acknowledge reports within 72 hours and to agree on a disclosure timeline with
you (default: 90 days, shorter for actively exploited issues).

## Scope

Security-relevant bugs include, among others:

- Memory unsafety or undefined behavior reachable from **safe** Loom code
  (this is a soundness bug in the language, always treated as security-relevant).
- Data races reachable from safe code.
- Flaws in `std` cryptography, TLS, HTTP parsing or other input-handling code.
- Supply-chain issues: release artifact integrity, build reproducibility, dependency compromise.

## Supported versions

Loom is pre-alpha; only `main` is supported. Before 1.0 this section will define how long
each release receives security patches across supported versions of the toolchain and std.

## Release integrity (planned)

Release artifacts will be reproducible, signed, and published with checksums and an SBOM
(see [docs/roadmap.md](docs/roadmap.md), Phase 7).
