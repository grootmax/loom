# ADR 0015: Agent workflow (Claude Code + Jules)

- **Status:** Accepted
- **Date:** 2026-10-23
- **Deciders:** lead architect, with owner approval of the kickoff process
- **Related:** [CLAUDE.md](../../CLAUDE.md), [AGENTS.md](../../AGENTS.md), [tasks/LEDGER.md](../../tasks/LEDGER.md)

## Context

Loom is built by a small team with AI coding agents: **Claude Code**, the lead
architect, spec owner, reviewer and integrator, and **Jules**, Google's asynchronous coding
agent running in cloud VMs against the GitHub repo. Agents are fast but can drift from
the spec, weaken tests, or add dependencies. The workflow must turn their speed into
trustworthy changes.

## Decision

1. **Division of labor.** Claude Code never delegates the spec and grammar, ADRs and RFCs,
   the type system and inference core, the ownership model and borrow checker, the
   scheduler core, the `loom-bridge` ABI, public API naming, security-sensitive code,
   release engineering, or merging. Jules gets bounded, well-specified, independently
   testable work (see CLAUDE.md for the list).
2. **Task specs first.** Every delegation starts with `tasks/NNNN-slug.md` (Context, Goal,
   Files you may change, Files you must NOT change, Requirements, Acceptance, Out of scope,
   Notes) and an entry in `tasks/LEDGER.md`.
3. **Dispatch.** Jules CLI 0.7 is organized around workspaces and tasks; the
   `jules remote …` commands from the original kickoff no longer exist. Dispatch uses the
   Jules MCP tool `start_task` (or `jules tasks create --json …`) in workspace `loom`
   (`c155675d-a568-4ea5-84f4-4f463a60558b`), which is connected to `grootmax/loom`. Status is
   checked with `get_task_result` / `jules tasks get <id>`.
4. **Parallelism:** at most 3–5 concurrent tasks, on disjoint files or directories. Never
   two on the same crate.
5. **Size:** each task is reviewable in about 15 minutes (≤ ~400 changed lines).
6. **Review as an untrusted contributor:** read the full diff; run `cargo xtask ci` locally
   on the branch; check acceptance criteria, forbidden paths, new dependencies, and
   weakened or deleted tests; and confirm the PR is green in CI. Then squash-merge or
   reject with a follow-up task.
7. **Feedback loop:** every recurring agent mistake becomes a rule in AGENTS.md's
   "Learned rules" section, and the lesson is recorded below.
8. **Spec authority:** if agent code disagrees with the spec, the code is wrong unless the
   spec is deliberately amended through an ADR.

## What Rust does

Not applicable as a language matter. The Rust project's review culture is the model:
every change is reviewed by someone accountable, CI is gating, and "r+" means the reviewer
owns the result.

## Alternatives considered

- **No delegation:** slower, and it wastes the parallelism available.
- **Delegating core design to agents:** rejected. Design coherence needs one accountable owner.

## Consequences

- Delegation overhead (writing specs, reviewing) is real. It pays off only for tasks that
  are well-bounded.
- `tasks/` becomes a historical record of what was delegated and why.

## Lessons learned

*(Updated after each delegation round.)*

### Phase 0 pilot (2026-10-23): tasks 0001–0003, all merged

| Task | Size | Time to PR | Result |
|---|---|---|---|
| 0001 CI Tier 1 matrix | +16/−1 | ~3 min | Merged as-is; all 7 jobs green |
| 0002 `xtask spec-check` | +330/−2 | ~6 min | Merged; reviewer removed one duplicated helper afterwards |
| 0003 issue forms | +142 | ~3 min | Merged as-is; maintainer created the two missing labels |

What we learned:

1. **Precise task specs work.** With exact allowed paths, numbered requirements and
   acceptance commands, Jules changed no forbidden file, added no dependency, and met
   every requirement on the first attempt. Keep the template strict.
2. **Don't trust the PR description; reproduce.** All three PRs ticked every checklist
   box (including docs and STATUS updates that were not made) and claimed CI results
   before CI had run. Review must rest on the diff, a local `cargo xtask ci`, and observed
   CI. This is now AGENTS.md learned rule 1.
3. **Run negative tests for checkers.** For 0002, we removed a reference and injected a bogus
   section locally to confirm that the failure messages matched the spec. Do this for every
   delegated linter or checker.
4. **Small reviewer fixes beat round trips.** The one duplication nit was cheaper to fix in
   the wrap-up PR than to send back. Recurring issues still become AGENTS.md rules (rule 2).
5. **Mind shared files.** Dependabot opened an `actions/checkout` bump on `ci.yml` while
   task 0001 was editing the same file. Hold bot PRs that touch a file an agent is working on,
   then `@dependabot rebase`.
6. **Strict branch protection serializes merges.** Each merge needs `gh pr update-branch`
   and a fresh CI run (~5–8 min with the full Tier 1 matrix). Acceptable at 3–5 parallel
   tasks. Use `gh pr merge --squash --auto` with a maintainer-written subject and body
   rather than the agent's PR body.
7. **Tool facts:** Jules PRs come from the `google-labs-stitch` app on `stitch/…` branches,
   with DCO-signed commits by `google-labs-jules[bot]`. `jules tasks get` needs
   `-w <workspace>`.

### Phase 1, wave 1 (2026-10-28): tasks 0004–0008

| Task | Result |
|---|---|
| 0004 `loom_snapshot` | PR #7 (with the 0008 batch); acceptable, small doc nits |
| 0005 `loom_unicode` + `gen-unicode` | PR #10; tables reproducible, cross-checked against `std` |
| 0006 lexical conformance | PR #6; 2 of 50 cases wrong (raw-string end, byte vs. char column) |
| 0007 explanations | PR #8; style issues (`def main()`), one invalid fix (E0109), one example without the bad character (E0002) |
| 0008 syntax conformance | PR #9; 3 of 52 cases used syntax the spec doesn't have |

What we learned:

1. **Conformance written before the implementation is a real cross-check.** The parser
   matched 97 of 102 blind-written cases, and every mismatch was a test bug. That is why
   we write conformance cases in parallel with the implementation.
2. **Agents invent plausible syntax when they can't run a checker.** Now that `loom check`
   exists, AGENTS.md learned rule 4 requires running it on every `.loom` file.
3. **A completed Jules task can't receive messages** (the API returns 404). Fixes go into
   reviewer commits or a follow-up task, so keep follow-ups small and precise.
4. **Repository access is a single point of failure.** A personal access token without
   `contents: write` blocked every push and merge while agents kept producing PRs. Check
   push access at the start of each session.

### Phases 2–3 (2026-10-29): tasks 0014–0023

| Task | Result |
|---|---|
| 0014 items/statements conformance | First run completed without publishing a branch; re-dispatched, PR #31, clean |
| 0015–0018 Phase 2 conformance and core tests | PRs #19–#22; phase-3 rule updates applied by the maintainer |
| 0019 §8 ownership conformance | PR #27 |
| 0020 explanations E02xx–E05xx | PR #26; needed a line in `explain.rs` outside its paths (the spec's omission) |
| 0021 rich CLI diagnostics | PR #25 |
| 0022 runtime tests under Miri | PR #30, closed after taking its test commit: it added `#![allow(clippy::pedantic)]` to a forbidden file; its ignored test found a real aliasing bug |
| 0023 nightly fuzz workflow | PR #29; maintainer hardened input handling but missed a YAML error that made the file invalid (fixed in #34) |

What we learned:

1. **Task specs must name code that is on `main`.** Task 0022 tested a runtime that only
   existed on an unmerged branch; Jules found the branch on its own. Dispatch tasks only
   against merged code, or say which branch to start from.
2. **"Completed" is not "published".** Check for the PR, not just the task state.
3. **Lint workflow files.** A workflow that does not parse fails silently (no jobs, just
   a failed run on every push). Run `actionlint` on every workflow change before merging.
4. **Blanket lint allows hide real bugs.** AGENTS.md learned rule 5.
5. **Strict up-to-date branch protection serializes merges.** Several green PRs wait
   on each other; merge them one at a time (update branch, wait for CI, merge).
