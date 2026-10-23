# RFCs

An RFC (Request for Comments) proposes a change to the Loom **language** (syntax,
semantics, type system), the **standard library's public API**, or the **compatibility
promise**. Toolchain architecture decisions use [ADRs](../adr/README.md) instead.

## When you need one

- New syntax or keywords, or changing what existing code means.
- New std modules or significant public API additions to `core`/`std`.
- Promoting something from `x/` into `std`.
- Anything touching editions, deprecation or the compatibility promise.

Bug fixes, diagnostics improvements and internal refactors do not need an RFC.

## Process

1. **Discuss** the idea in an issue first ("RFC idea"). Cheap feedback saves work.
2. **Write** the RFC: copy [0000-template.md](0000-template.md) to
   `docs/rfcs/0000-short-name.md` and open a PR. Every section is required, especially
   *Cost of carrying this forever*.
3. **Comment period:** at least 14 days once the RFC is marked ready.
4. **Decision:** currently by the project owner; later by the core team (see
   [GOVERNANCE.md](../../GOVERNANCE.md)). The outcome and reasoning are recorded in the PR.
5. **Accepted** RFCs get a number (the PR number), are merged, and are implemented behind
   a feature gate until stabilized. The spec is updated in the same PR that stabilizes.
6. **Postponed or rejected** RFCs are closed with reasons; they can be revisited later.

Macros and user-defined syntax extensions are not considered before 1.0.
