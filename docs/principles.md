# Design principles

## The pillars, in priority order

When two pillars conflict, the higher one wins. An ADR or RFC that trades a higher
pillar for a lower one must say so explicitly.

1. **Correctness and safety.** No undefined behavior in safe code. No null. No data races.
   Errors are values the type system tracks.
2. **Readability.** Code is read far more than written. There is one obvious way to do
   common things. Types are explicit at boundaries and inferred inside bodies.
3. **Stability.** Code written for Loom 1.0 must still compile decades later. Breaking
   changes happen only at edition boundaries, with automated migration.
4. **Developer experience.** Helpful errors with concrete fixes, fast feedback, one tool,
   great documentation.
5. **Performance.** Predictable and good by default — no garbage-collection pauses,
   no hidden allocations in hot paths — and tunable when needed.
6. **Portability.** The same code runs everywhere the [platform tiers](adr/0012-platform-tiers.md)
   promise.

## Working rules that follow from them

- **Spec first.** The specification is normative. Implementation behavior that contradicts
  it is a bug.
- **Write it down.** Every architectural decision is an ADR; every language change is an RFC
  with a "cost of carrying this forever" section.
- **Saying no is a feature.** A small, orthogonal core outlives a large one.
- **Boring technology.** Prefer proven techniques with prior art (Rust, Go, Swift, Koka,
  Lean, Crystal, Gleam, Zig) and cite it.
- **Own the critical path.** Few dependencies; none on unmaintained or single-maintainer
  crates for core paths without an exit plan.
- **Costs are visible.** Shared ownership, dynamic dispatch, detached tasks and `unsafe`
  are spelled out in the code so they show up in review.
- **Errors teach.** Every diagnostic says what went wrong, why, and what to do next.
