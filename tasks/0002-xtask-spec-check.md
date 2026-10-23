# Task 0002: `cargo xtask spec-check` — grammar rules must cite existing spec sections

## Context
`spec/grammar.ebnf` is the formal grammar of Loom, and `spec/loom-spec.md` is the normative
specification. By convention (see the header comment of `grammar.ebnf`), **every grammar
rule is immediately preceded by a comment `(* §N.M *)`** naming the spec section that
defines it. A section reference is a `§` followed by dot-separated numbers, e.g. `§2.7.1`,
`§3`, `§12.1`. Spec sections are Markdown headings such as `## 3. Items`,
`### 3.1 Functions`, `#### 2.7.1 Integer literals`. We want CI to enforce the convention,
so that the grammar and the spec cannot drift apart. `xtask/` is a std-only Rust crate.
Read AGENTS.md first.

## Goal
`cargo xtask spec-check` fails with clear messages when a grammar rule lacks a section
reference or references a section that does not exist, and `cargo xtask ci` runs it.

## Files you may change
- `xtask/src/main.rs`
- new files under `xtask/src/` (e.g. `xtask/src/spec_check.rs`)

## Files you must NOT change
- `spec/` (if the check finds real problems in the spec or grammar, report them in your
  PR description instead of editing those files), `xtask/Cargo.toml`, `Cargo.lock`,
  and everything outside `xtask/src/`.

## Requirements
1. Add a `spec-check` command to xtask and list it in the `HELP` text.
2. **Rule detection:** a grammar rule starts on a line whose first non-whitespace text is
   an identifier (`[A-Za-z_][A-Za-z0-9_]*`) followed by optional whitespace and `=`,
   provided that the line is *not inside* a `(* … *)` comment. Comments can span multiple
   lines, and the large header comment at the top of the file must be ignored.
3. **Reference detection:** for each rule, the closest preceding non-blank line must be a
   comment containing exactly one section reference `§N(.N)*`. Otherwise, report
   `grammar.ebnf:LINE: rule `NAME` has no (* §N.M *) section reference`.
4. **Section existence:** collect section numbers from headings in `loom-spec.md`. A
   heading is a line starting with 2–4 `#` characters, a space, then a section number
   (digits separated by `.`, with an optional trailing `.` as in `## 3. Items`) and a space.
   Report each reference to a missing section as
   `grammar.ebnf:LINE: rule `NAME` cites §X, which is not a heading in spec/loom-spec.md`.
5. Print all problems (not just the first), then exit non-zero if there were any. On
   success, print `spec-check: N grammar rules, all cite existing spec sections`.
6. Run `spec-check` as the **first** step of `cargo xtask ci`.
7. Structure the code so that parsing is in pure functions taking `&str`. Add unit tests
   (`#[cfg(test)]`) covering: a valid rule; a missing reference; a reference to a missing
   section; a rule-like line inside a multi-line comment (must be ignored); `## 3. Items`
   style headings with a trailing dot; and nested section numbers like `§2.7.1`.
8. Standard library only. Stay within clippy `pedantic` with `-D warnings`.

## Acceptance
- `cargo xtask spec-check` succeeds on the current repository (the grammar is expected to
  be fully referenced; if it is not, explain exactly which lines fail in the PR description).
- `cargo xtask ci` passes.
- Temporarily deleting one `(* §… *)` comment in a local copy makes `spec-check` fail with
  the message from requirement 3 (verify, then revert; do not commit spec changes).

## Out of scope
- Checking that every spec section is referenced by the grammar (the reverse direction).
- Parsing EBNF beyond detecting rule names.

## Notes
- Follow AGENTS.md. Conventional Commit title: `feat(xtask): add spec-check for grammar section references`.
- No new dependencies. Never weaken or delete existing tests.
