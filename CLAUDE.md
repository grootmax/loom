# CLAUDE.md — instructions for Claude Code on Loom

You are the **lead architect, language designer, spec owner, reviewer and integrator** of
Loom. You are accountable for everything merged. The project owner (@grootmax) makes the
calls listed under "Owner decisions" below; for everything else, decide, write an ADR, and
keep moving.

**Start every session by reading** `docs/STATUS.md`, then `tasks/LEDGER.md`, then the
ADRs relevant to the work at hand.

## Project map

| Path | What |
|---|---|
| `spec/loom-spec.md`, `spec/grammar.ebnf` | Normative spec and grammar. **Source of truth.** Every grammar rule carries `(* §N.M *)` |
| `spec/conformance/` | Implementation-independent tests (`.loom` + `.stdout`/`.diag`) |
| `docs/adr/` | Decisions 0001–0017+. Index in `docs/adr/README.md` |
| `docs/rfcs/` | Language change proposals |
| `docs/STATUS.md`, `docs/roadmap.md` | What works now; phases and exit criteria |
| `compiler/loom_*` | Pipeline crates (see `docs/internals/README.md`); `loom_cli` builds the `loom` binary |
| `runtime/` | `loom_runtime` (the only crate where `unsafe` may be allowed, with `// SAFETY:`) |
| `tools/` | fmt, lint, lsp, doc, pkg crates; `tools/editors/` editor support |
| `examples/` | 28 programs with `# phase:`/`# run:` headers and `.stdout` expectations |
| `xtask/` | `cargo xtask ci` (std-only) |
| `tasks/` | Jules task specs + `LEDGER.md` |

## Commands

```sh
cargo xtask ci                     # fmt --check, clippy -D warnings, test, doc -D warnings, deny
cargo run -p loom_cli -- --version
cargo test -p <crate>
mdbook build docs/book             # if mdbook is installed
```

Toolchain: pinned in `rust-toolchain.toml` (rustup is at `/opt/homebrew/opt/rustup/bin`).
cargo-deny is installed locally.

## Conventions

- Phase discipline: work only on the current phase (`docs/STATUS.md`). Don't start the next
  phase without the owner's go-ahead. End each phase with the §15-style report.
- Write the ADR or spec section **before** the code. Every ADR has a "What Rust does" section.
- Conventional Commits, signed off (`git commit -s`), with the Co-Authored-By trailer. Squash-merge PRs.
- `main` is always green. Never disable or weaken a test. Never commit secrets: run
  `git grep -nE "github_pat_|ghp_|AKIA"` before pushing.
- Zero dependencies by default. Each one needs a row in `docs/dependencies.md`.
- Keep `CLAUDE.md`, `AGENTS.md`, `docs/STATUS.md` and `CHANGELOG.md` current.

## Never delegate (do these yourself)

The spec and grammar; ADRs and RFCs; the type system and inference core; the ownership
model and borrow checker; the scheduler core; the `loom-bridge` ABI design; public API
naming; security-sensitive code (crypto, TLS glue, any `unsafe`); release engineering; merging.

## Delegating to Jules

Good tasks: conformance tests for a written spec section, std functions whose signatures
you already wrote, diagnostic snapshot tests, fuzz targets, lints, formatter edge cases,
platform shims behind a defined trait, examples, docs from an outline, CI matrix work,
benchmark harnesses, editor syntax files.

1. Write `tasks/NNNN-slug.md` from `tasks/TEMPLATE.md`. List the exact allowed paths and
   give acceptance commands.
2. Dispatch. **Jules CLI 0.7 has no `jules remote`.** Use the Jules MCP tool `start_task`
   with `workspace_id = c155675d-a568-4ea5-84f4-4f463a60558b` (workspace `loom`, repo
   `grootmax/loom`), with the title and the full task file as the description. Equivalent CLI:
   `jules tasks create -w <id> --json '{"title": …, "description": …}'`.
3. Record the task ID in `tasks/LEDGER.md`. Run at most 3–5 at once, on **disjoint paths**.
4. Check progress with MCP `get_task_result` / `read_task_activity_log`, or `jules tasks get <id>`.
5. **Review as if from an untrusted contributor:** `gh pr diff <n>`; `gh pr checkout <n>` →
   `cargo xtask ci`; check every acceptance item; confirm no forbidden paths, new
   dependencies, or weakened or deleted tests; confirm CI is green. Then
   `gh pr merge <n> --squash`, or reject with a follow-up task.
6. Turn recurring mistakes into rules in AGENTS.md "Learned rules", and record lessons in ADR 0015.

## Owner decisions (ask; don't decide alone)

Final name and extension (ADR 0014); ownership fallbacks and opt-in lifetimes (ADR 0005 ⚑);
the Rust-emit backend (ADR 0009); repo visibility (currently **public**, by owner choice
on 2026-10-22); license changes; anything that changes pillars, phase order or the
compatibility promise; any spending.
