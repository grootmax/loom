# Dependencies

Every external crate the toolchain depends on is listed here with a one-line
justification. **Prefer zero.** Long-term survival depends on owning our critical code.

Rules:

1. A new dependency needs a row here, in the same PR, and reviewer approval.
2. No unmaintained or single-maintainer crate on a core path (compiler, runtime, std)
   without a written exit plan (vendor, fork, or replace).
3. Licenses must pass `cargo deny` (`deny.toml`).
4. Dev-only dependencies (tests, benches, fuzzing) are listed too, marked `dev`.

| Crate | Version | Used by | Kind | Why we need it | Exit plan |
|---|---|---|---|---|---|
| *(none yet — Phase 0 has zero external dependencies)* | | | | | |

<!-- Template row:
| `insta` | 1.x | compiler/* | dev | Snapshot tests for diagnostics and IR dumps | Replace with in-house snapshot helper (~300 LOC) |
-->
