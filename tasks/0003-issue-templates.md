# Task 0003: GitHub issue forms

## Context
Loom is a public repository for a new programming language. Contributors should report
bugs, propose ideas, and flag documentation problems in a structured way. Language changes
go through the RFC process (`docs/rfcs/README.md`), which starts with an "RFC idea" issue.
Security problems must **not** be reported as issues; see `SECURITY.md`. Read AGENTS.md first.

## Goal
Opening a new issue offers three structured forms and links to the security policy, with
blank issues disabled.

## Files you may change
- `.github/ISSUE_TEMPLATE/bug_report.yml` (new)
- `.github/ISSUE_TEMPLATE/rfc_idea.yml` (new)
- `.github/ISSUE_TEMPLATE/docs.yml` (new)
- `.github/ISSUE_TEMPLATE/config.yml` (new)

## Files you must NOT change
- Everything else, including `.github/workflows/` and `.github/PULL_REQUEST_TEMPLATE.md`.

## Requirements
1. All four files use GitHub's issue-forms YAML syntax and are valid YAML.
2. `bug_report.yml` ("Bug report", label `bug`) has required fields: what happened; the
   expected behavior; minimal reproduction (a `render: loom` textarea for Loom code);
   `loom --version` output; OS and architecture (dropdown: Linux x86_64, Linux aarch64,
   macOS aarch64, macOS x86_64, Windows x86_64, Other). It also has a component dropdown
   (lexer/parser, type checker, interpreter, code generation, runtime, std, tooling, docs, other).
3. `rfc_idea.yml` ("RFC idea", label `rfc-idea`) has fields: the problem (required); a
   proposed Loom code example (required, `render: loom`); "What does Rust do here?" (required);
   alternatives considered; and "Cost of carrying this forever" (required). Its
   description links to `docs/rfcs/README.md` using a full
   `https://github.com/grootmax/loom/blob/main/...` URL.
4. `docs.yml` ("Documentation", label `docs`) has fields: the page or file (required); the
   problem (required); a suggested fix.
5. `config.yml` sets `blank_issues_enabled: false` and has a contact link named
   "Report a security vulnerability" pointing to
   `https://github.com/grootmax/loom/security/advisories/new`.
6. The wording is short, friendly and plain English. Every form starts with a markdown
   block reminding reporters to search existing issues first.

## Acceptance
- Each file parses: `python3 -c "import yaml,glob; [yaml.safe_load(open(f)) for f in glob.glob('.github/ISSUE_TEMPLATE/*.yml')]"`.
- `cargo xtask ci` still passes (no Rust changes are expected).

## Out of scope
- Creating labels, discussions, or changing the PR template.

## Notes
- Follow AGENTS.md. Conventional Commit title: `chore: add GitHub issue forms`.
