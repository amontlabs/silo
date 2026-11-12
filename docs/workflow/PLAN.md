# Faster agent builds: plan

Proposal of 2026-11-12, from the timing analysis of the T3 Code builds (forks/CI/containers,
Direction, MicroSandbox upgrade; about 81 hours of working time, reports in
`~/code/projects/research/silo-timing-2026-11/`) and the owner's decisions. Fable reviewed the
current setup and its advice is folded in.

## What we are fixing

| Measured cost (T3 era) | Cause | Fix in this plan |
|---|---|---|
| ~31% of working time: a child polling GitHub CI until green | CI used as the gate | No CI waits at all (rule 1) |
| ~2/3 of child time had one child running | Serial phases, no lookahead | Board + stacking + 4–6 children (rule 2, `board`) |
| ~12%: live tests, mostly polling, rerun from scratch, failing on setup | Ad-hoc scripts per item | One `live` harness with steps, resume and a done-file |
| ~10%: Rust test loops, 1/3 failing (most on compile errors), full suites, cold or mixed-toolchain builds | Tests run as a reflex | `affected` + `check`, pinned toolchain |
| 9-minute `runtime:prepare` run 4× in one effort | No cache | `runtime-cache` |
| Review rounds up to 7, each re-running tests; children reviewing themselves | Loose review rules | One review per item, by the orchestrator, max 2 rounds |
| Gemini/Flash implementers always redone by Sonnet; Gemini advisor 43 min vs Fable 2 min | Models picked ad hoc | Named lastdrop agents only |
| Contradictory rules across 40 memory files and two accounts | Rules saved as memories | Rules live in the repo; memories hold facts only |

## Rules (what the documents will say)

1. **No CI in the loop.** Agents never push to wait for CI, never poll `gh run`/`gh pr checks`,
   never rerun CI. The owner notices a red `main`; releases run the full checks.
2. **Keep work flowing.** The orchestrator plans items as a dependency graph with the files each
   touches. Disjoint items run in parallel worktrees; dependent items, or items on the same hot
   files, stack in one worktree and start as soon as the previous item commits (not after its
   review). 4–6 children at once. Whenever a child finishes or only one is running, the
   orchestrator looks at the board for something to start before it waits.
3. **Tests are tools, not rituals.** A child runs tests to prove its change works or to find what
   broke, using `check`, which runs only what the diff touches. Full suites run once per merge
   batch, in the background, never blocking new work. No `--workspace` runs by children. Linux is
   checked locally when the Linux machine answers; if it is offline, skip it.
4. **Flaky tests get fixed, not retried.** A test that fails then passes goes to
   `docs/workflow/FLAKY.md` and becomes a fix item; nobody loops on it.
5. **Reviews at the orchestrator's judgement, made cheap.** Only the orchestrator starts
   reviewers (children never review themselves). How many rounds is its call, by risk and by
   what the last round found. Each round is made cheap: the brief asks for every instance of
   each problem in one pass, with a concrete failure each; a follow-up round only checks the
   fixes and what they touch; fixes go back to the child that wrote the code; reviews run in
   the background while other work continues. Judgement calls and guesses go to the owner's log
   (`OWNER_REVIEW.md` of the build) and never block a merge.
6. **Judgement goes to Fable.** Design calls and disputed findings go to the `judge` agent; the
   orchestrator decides with it, records what the owner may want to change, and moves on.
7. **Agents by name only.** The orchestrator picks a lastdrop agent by its name and description.
   Models, fallbacks and limits are lastdrop's job and the owner's, not the agents'.
8. **One topic per child; reuse context.** Fixes and closely related follow-ups go back to the
   child that already holds the context (and its warm build), as long as its session stays
   small; a child whose session has grown large is replaced by a fresh one with a short
   handover. Children block on long commands and report once; they never end a turn to wait.
9. **Clean up as soon as nothing depends on it.** When an item is merged and no stacked item
   builds on it, its worktree, branch, build folder and temporary data (logs, temp homes, live
   test leftovers) are removed at once. `clean` does this and refuses while a dependent
   branch exists.

## Agents (set in lastdrop)

Models are ordered so that separately limited quotas are used first (Antigravity's Claude
quota, Codex) and the main Claude plan last. Two Claude models on the same plan are never each
other's fallback.

| Name | Models in order (effort) | For | Started with |
|---|---|---|---|
| `build` | antigravity claude-sonnet-5-5 → claude sonnet (high) | Implementation and fixes, one topic each | `delegate_task`, worktree or stacked |
| `build-hard` | antigravity claude-opus-5-5 → claude opus (high) | Cross-crate, concurrency, migration items | `delegate_task` |
| `review` | codex gpt-6.1-sol (high) → antigravity claude-opus-5-5 → claude opus (high) | Code reviews | `Agent`, background, read-only |
| `judge` | claude fable (high) | Design calls, disputed findings, advice | `Agent`, background |
| `live` | antigravity claude-sonnet-5-5 → claude sonnet (medium) | Runs the `live` harness, reads its logs | `delegate_task` |
| `scout` | claude haiku | Read-only lookups, preflight facts | `Agent`, background |
| `cheap` | zai glm-5.3-flash → antigravity gemini-3.8-flash | Only when the owner asks for cheap agents | as the role it replaces |
| `computer-use` | unchanged | GUI steps, called by `live` or the orchestrator | `Agent` |

Removed: `reviewer`, `advisor`, `gemini`, `flash`. Open check: whether Antigravity's Claude
models handle long Rust tasks as well as Claude's own (try on the dry run).

## Where the rules live

- `CLAUDE.md` (new, both checkouts): one line, `@AGENTS.md`, so Claude children always load it.
- `AGENTS.md`: the "Run checks appropriate to the change" list and the "Delegated work" section
  are replaced by short binding rules and a pointer to `docs/workflow/`.
- `docs/workflow/`:
  - `ORCHESTRATOR.md`: the loop, step by step (preflight, plan the graph, launch, on each report,
    merge, owner log), with the exact commands and scripts for each step.
  - `ROLES.md`: the agent table above, which tool starts each, and what to give it.
  - `CHILD.md`: rules every child follows (successor of `docs/direction/AGENT-BRIEF.md`, minus
    self-review): checkout, scope, `check`, blocking waits, report format.
  - `briefs/build.md`, `fix.md`, `review.md`, `live.md`, `scout.md`, `judge.md`: fill-in
    templates the orchestrator copies into each task.
  - `FLAKY.md`, and `state/board.tsv` per build (item, branch, base, state, owner).
- Memory: workflow memories are deleted and replaced by one pointer ("workflow rules are in
  `docs/workflow/`; never save workflow rules to memory"). Facts about the machine and safety
  stay (AZERTY, LCU, Linux machine, staging, no `/tmp` deliverables). To delete:
  subagent-model-sonnet, no-fable-subagents, fable-review-flash-build, no-codex-subagents,
  codex-review-loops, codex-bridge-wait-command, review-when-codex-out,
  reviewer-from-orchestrator, delegate-through-lastdrop, gemini-lastdrop-cargo-limits,
  never-idle-waiting, watch-upstream-prs, worktree-build-clones (its facts move to `CHILD.md`).
  Other accounts (`~/.claude-accounts/*`) have no memories of their own, so children running
  on them never saw these memories anyway; rules in the repo reach every account.

## Tools and scripts

Scripts only take over what is repetitive, intricate and deterministic (choosing test commands
from a diff, merge mechanics, setup of live tests, cleanup). Judgement stays with the agents:
which tests prove a change, whether a failure matters, what to review. Research of existing tools
(reports in `~/code/projects/research/silo-timing-2026-11/research-*.md`) decided what is adopted
and what is left to small scripts.

**Adopted, no new install:**
- `rust-toolchain.toml` pinning one Rust version. Mixed `+1.94.0`/`+1.95.0`/`+stable` runs
  invalidate both the build folder and sccache; this is the largest single rebuild saving.
- cargo-nextest filtersets (`rdeps()`, `package()`, `test()`) to run only affected crates/tests.
- Vitest's own `--changed <base>` and `vitest related <files>`; oxlint and `tsc -b` as the
  frontend's fast first gate.
- Plain git for stacking (`git rebase --onto`), driven by the board; git-spice only if agents keep
  getting restacks wrong.

**Skipped, with the reason:** Bazel/Buck2/Pants (would need a build rewrite; Tauri and the patched
MicroSandbox build do not map onto them), Nx/Turborepo (too heavy for one npm package and one
Cargo workspace), cargo-difftests (nightly and instrumented builds), mold/lld on macOS (little
gain), go-task (its cache lives per worktree, so a shared runtime cache needs a script anyway),
git-branchless and jj (unfinished or a different model from T3's git worktrees), Graphite (tied to
its service), Conductor/Crystal/Vibe Kanban (built around pull requests and CI), Earthly/Dagger
(cannot run macOS VMs), beads/taskwarrior (too heavy for ~30 items).

**Small scripts in `tools/agent/`:**
1. **`affected`** — maps a diff (against the integration branch) to affected crates with
   `cargo metadata`, reusing `app/SiloUI/scripts/ci-native-changes.py` for non-Rust files Rust
   tests read; prints the nextest filterset and the frontend files. `Cargo.lock`, the root
   `Cargo.toml` and the toolchain file mean "everything". `--why` explains each line.
2. **`check`** — the fast path a child uses when it wants proof: fmt on touched files →
   `cargo check --tests -p <affected>` → `cargo nextest run -E <affected>` (no fail-fast) →
   typecheck/oxlint/`vitest related` only if the frontend changed. Logs to a file, prints failures
   and a one-line verdict, and skips a run already green for the same tree hash, toolchain and
   command. Children may run narrower commands themselves when that proves the point faster.
3. **`board`** — one TSV per build (item, branch, base, depends on, state) with `ready` (its base
   is committed, so it can start stacked), `status`, and `cleanable` (merged, nothing unmerged
   depends on it) queries; `restack` moves dependents bottom-up after a lower branch changes
   (`git rebase --update-refs` skips branches checked out in other worktrees, so one at a time).
4. **`clean <item>`** — refuses while a dependent exists; otherwise removes the worktree, branch,
   build folder and temp data (logs, temp homes, live-test leftovers, stopping only the throwaway
   `e2e-*` computers that test started).
5. **`merge-batch <branch>...`** — merge in order, regenerate API files once, run `affected` tests
   on the union in the foreground, start the full suite in the background writing
   `full-suite.<sha>.{log,done}`; on failure, name the failing tests and the branch that touched
   those paths. Replaces `docs/direction/tools/integrate.sh`.
6. **`live <check> [--from step] [--host mac|linux]`** — a small bash step-runner: a fast preflight
   (paths, guest-image links, socket path length, Linux reachable in 3 s or "skipped: offline"),
   named steps with a stamp each so a rerun resumes at the failed one, and a verdict file written
   on every exit. A child waits with one blocking command on the verdict file. The checks in
   `docs/direction/checks/` become steps.
7. **`runtime-cache`** — wraps `runtime:prepare`, keyed on `runtime-inputs.json`, the patch
   hashes, toolchain and target, stored outside worktrees and restored by copy-on-write. First find
   out why the build runs on one core.
8. **`linux-warm`** — keeps a checkout, toolchain and build on the Linux machine.
9. **Flaky log** — `docs/workflow/FLAKY.md`, fed from nextest's JUnit output when a test fails and
   then passes.

## How it gets built

| Step | What | Who | Parallel with |
|---|---|---|---|
| 0 | Research existing tools (done) | 2 research agents | — |
| 1 | lastdrop agents changed; memories purged; fallback checked | orchestrator | 2–5 |
| 2 | `CLAUDE.md`, `AGENTS.md` trim, `docs/workflow/` documents and briefs | `build` child | 3–5 |
| 3 | `rust-toolchain.toml`, `affected`, `check`, flaky log | `build` child | 2, 4, 5 |
| 4 | `board`, `clean`, `merge-batch` | `build` child | 2, 3, 5 |
| 5 | `live` harness (porting the Direction checks as steps) | `build` child | 2–4 |
| 6 | `runtime-cache` (after a one-core investigation), `linux-warm` | `build` child | after 3 |
| 7 | One `review` (Fable) of the documents and scripts together; one fix round | orchestrator | — |
| 8 | Dry run: the next real Direction item run end to end under the new rules, timed | orchestrator | — |

Everything lands on `main` (it applies to every build) and is merged into `direction`.
Done means: a child given only `CHILD.md` and a brief can do an item with `check` and report;
the orchestrator can run a wave from `ORCHESTRATOR.md` alone; and the dry run shows no CI
waits, no full suite run by a child, and at least two children in flight whenever more than one
item is open.
