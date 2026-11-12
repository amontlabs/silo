# Faster agent builds: plan

Proposal of 2026-11-12, from the timing analysis of the T3 Code builds (forks/CI/containers,
Direction, MicroSandbox upgrade; about 81 hours of working time, reports in
`~/code/projects/research/silo-timing-2026-11/`) and the owner's decisions. Fable reviewed the
current setup and its advice is folded in.

## What we are fixing

| Measured cost (T3 era) | Cause | Fix in this plan |
|---|---|---|
| ~31% of working time: a child polling GitHub CI until green | CI used as the gate | No CI waits at all (rule 1) |
| ~2/3 of child time had one child running | Serial phases, no lookahead | Board + stacking + 4–6 children (rule 2, `stack`) |
| ~12%: live tests, mostly polling, rerun from scratch, failing on setup | Ad-hoc scripts per item | One `live` harness with steps, resume and a done-file |
| ~10%: Rust test loops, 1/3 failing (most on compile errors), full suites, cold or mixed-toolchain builds | Tests run as a reflex | `select-tests` + `check`, pinned toolchain |
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
5. **One review per item, by the orchestrator.** Children never start reviewers. The `review`
   agent (Fable first) reports every instance of each problem in one pass, with a concrete
   failure each. One fix round, then one confirmation pass limited to those findings. What
   remains, and anything that is a judgement call or a guess, goes to the owner's log
   (`OWNER_REVIEW.md` of the build) without blocking the merge.
6. **Judgement goes to Fable.** Design calls and disputed findings go to the `judge` agent; the
   orchestrator decides with it, records what the owner may want to change, and moves on.
7. **Agents by name only.** The orchestrator picks a lastdrop agent by its name and description.
   Models, fallbacks and limits are lastdrop's job and the owner's, not the agents'.
8. **One topic per child.** A fix goes back to the same child (it keeps its context and warm
   build). Children block on long commands and report once; they never end a turn to wait.

## Agents (set in lastdrop)

| Name | Models in order (effort) | For | Started with |
|---|---|---|---|
| `build` | sonnet (high) → opus (medium) | All implementation and fixes, one topic each | `delegate_task`, worktree or stacked |
| `build-hard` | opus (high) → sonnet (high) | Cross-crate, concurrency, migration items; 1–2 per wave | `delegate_task` |
| `review` | fable (medium) → opus (high) → codex sol (high) | One review per item | `Agent`, background, read-only |
| `judge` | fable (high) → opus (high) | Design calls, disputed findings, advice | `Agent`, background |
| `live` | sonnet (medium) → opus (medium) | Runs the `live` harness, reads its logs | `delegate_task` |
| `scout` | haiku → sonnet (low) | Read-only lookups, preflight facts | `Agent`, background |
| `computer-use` | unchanged | GUI steps, called by `live` or the orchestrator | `Agent` |

Removed: `reviewer`, `advisor`, `gemini`, `flash` (a name that exists gets used).
One check after the change: confirm lastdrop falls back mid-task, not only at launch.

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
  The second account's memories get the same purge.

## Scripts (`tools/agent/`)

1. **`select-tests [base]`** — maps changed paths to the smallest useful commands and prints
   them: crate → `cargo nextest run -p <crate> <module filter>`; `api/` → both generators;
   `.ts/.tsx` → `vitest related <files>`; scripts → their own tests; docs only → nothing.
   Builds on `app/SiloUI/scripts/ci-native-changes.py`. `--why` explains each line.
2. **`check [--quick|--run]`** — the only check a child runs: fmt on touched files →
   `cargo check --tests -p <touched>` → selected tests → typecheck/lint only if frontend changed.
   Log to a file, print failures and a one-line verdict, `--no-fail-fast` always, and "already
   green" on an unchanged tree. Uses the toolchain from a new `rust-toolchain.toml`.
3. **`merge-batch <branch>...`** — for the orchestrator: merge in order, regenerate API files
   once, run `select-tests` on the union in the foreground (1–2 min), then start the full suite
   in the background writing `full-suite.<sha>.{log,done}`. On failure: failing tests and the
   branch that touched those paths. Replaces `docs/direction/tools/integrate.sh`.
4. **`live <check> [--from step] [--host mac|linux]`** — one harness for all live checks: shared
   setup (temp home, guest-image links, short socket path, Dev engine identity), named steps with
   `.ok` markers so a rerun resumes at the failed step, a `done` file with the verdict, Linux
   probed with a 3-second ssh timeout ("skipped: offline"). Existing checks in
   `docs/direction/checks/` become steps.
5. **`stack new|add|status|clean`** — worktrees and the board: create, stack an item on an
   existing branch, show what is running or ready, remove worktrees and targets after merge.
6. **`runtime-cache`** — wraps `runtime:prepare`, keyed on MicroSandbox revision, patch hashes,
   toolchain and target, stored outside worktrees, restored by copy-on-write. First find out why
   the build uses one core.
7. **`linux-warm`** — keeps a checkout, toolchain and build on the Linux machine, so a Linux run
   does not start cold (a cold build there cost 21 minutes).

## How it gets built

| Step | What | Who | Parallel with |
|---|---|---|---|
| 1 | lastdrop agents changed; memories purged; fallback checked | orchestrator | 2–5 |
| 2 | `CLAUDE.md`, `AGENTS.md` trim, `docs/workflow/` documents and briefs | `build` child | 3–5 |
| 3 | `select-tests`, `check`, `rust-toolchain.toml` | `build` child | 2, 4, 5 |
| 4 | `merge-batch`, `stack`, board | `build` child | 2, 3, 5 |
| 5 | `live` harness (porting the Direction checks as steps) | `build` child | 2–4 |
| 6 | `runtime-cache` (after a one-core investigation), `linux-warm` | `build` child | after 3 |
| 7 | One `review` (Fable) of the documents and scripts together; one fix round | orchestrator | — |
| 8 | Dry run: the next real Direction item run end to end under the new rules, timed | orchestrator | — |

Everything lands on `main` (it applies to every build) and is merged into `direction`.
Done means: a child given only `CHILD.md` and a brief can do an item with `check` and report;
the orchestrator can run a wave from `ORCHESTRATOR.md` alone; and the dry run shows no CI
waits, no full suite run by a child, and at least two children in flight whenever more than one
item is open.
