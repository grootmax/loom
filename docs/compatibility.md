# Compatibility promise (draft)

> **Draft.** This promise takes effect at Loom 1.0. Until then, anything may change —
> but changes are still recorded in ADRs and the changelog.

## The promise

After 1.0, **a program that compiles and runs correctly with one Loom toolchain will
compile and behave the same with every later toolchain, as long as it stays on the same
edition.** Crates of different editions link together forever.

## What can change within an edition

- **Bug fixes**, including soundness fixes. If code compiled only because of a bug that
  makes safe code unsound, it may stop compiling. Such breaks are announced in advance
  where possible, with a lint first and a `loom fix` when feasible.
- **New features** that do not change the meaning of existing programs. New keywords are
  reserved only at edition boundaries.
- **New warnings and lints.** Warnings never fail a build unless the project opts in.
- **Additions to std** (new items, new trait implementations), with care taken over
  inference breakage; std additions that risk ambiguity go through an RFC.
- **Diagnostics wording, performance, binary layout of unstable internals.**

## What changes only at edition boundaries

- Syntax and keyword changes.
- Changes to the meaning of existing code.
- Removals of deprecated features.

Each edition ships a `loom fix --edition <year>` migration. Editions appear at most once
every three years.

## Deprecation policy

A feature is deprecated with a warning for at least one full edition before it is
removed. Removals happen only at edition boundaries. Every deprecation ships with an
automated `loom fix` rewrite.

## Not covered

- Behavior that the spec marks as unspecified or implementation-defined.
- Programs using `unsafe` in ways the spec says are undefined behavior.
- Experimental `x/` packages and anything explicitly marked unstable.
- Tooling output formats not marked stable (human-readable diagnostics, IR dumps).

See [ADR 0011](adr/0011-editions-and-compatibility.md) for the reasoning.
