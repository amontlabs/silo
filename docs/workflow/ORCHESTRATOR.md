# Orchestrator: the loop

You plan, launch, review, merge and clean. Children build. Rules: [AGENTS.md](../../AGENTS.md#agent-workflow). Agents and how to start them: [ROLES.md](ROLES.md). Children follow [CHILD.md](CHILD.md). Every script has `--help`.

## Setup (once per build)

A build has one fixed **state directory** outside every checkout: `<integration checkout>.state`, next to the integration checkout (for example `/Users/polarzero/code/projects/silo-direction.state`). It holds `board.tsv`, `flaky.log` and the logs, survives any worktree cleanup, and is where `board`, `clean`, `merge-batch` and `check` look by default. You may work from any checkout; the board does not live in one. The owner log is `OWNER_REVIEW.md` in the state directory, unless the build already has one in its docs folder (Direction: `docs/direction/OWNER_REVIEW.md`); then keep using that one, edited and committed in your own worktree and merged like any item, never directly in `INT`.

```sh
export INT=<absolute path of the integration checkout>
export SILO_BUILD_STATE=$INT.state
export BOARD_INTEGRATION=$(git -C "$INT" branch --show-current)   # main or direction
export SILO_INTEGRATION_BRANCH=$BOARD_INTEGRATION
mkdir -p "$SILO_BUILD_STATE" && touch "$SILO_BUILD_STATE/board.tsv" "$SILO_BUILD_STATE/OWNER_REVIEW.md"
```

`INT` below is the integration checkout. The three variables are the defaults when unset, so setting them only matters if you want another place. Only you edit the board.

## 1. Preflight

Start a `scout` ([brief](briefs/scout.md)) in the background for facts that would otherwise break an item late:

- release files and version numbers the items touch (`docs/SiloUI-RELEASES.md`, changesets);
- the toolchain: `rust-toolchain.toml` equals `toolchain` in `app/SiloUI/runtime-inputs.json`; `cargo nextest --version` is 0.9.151 (`cargo install cargo-nextest --locked --version 0.9.151`);
- hard-coded versions, paths or counts the items would have to change (API generator output);
- whether the Linux machine answers: `tools/agent/linux-warm --dry-run` (prints `skipped: offline` when not).

Read its answer before planning. Fix a wrong toolchain or version first, as its own item.

## 2. Plan the graph

For every item write: name, one-line goal, **files it touches**, **depends on**. Rules:

- Disjoint files and no dependency: parallel, each in its own new worktree.
- Dependent, or same hot file (`registry.rs`, `application-source.ts`, `application-shell.tsx`, fixtures): stacked, in order.
- One topic per item. A cross-crate, concurrency or migration item is `build-hard`; the rest `build`.
- Ask `judge` ([brief](briefs/judge.md)) for design calls you cannot settle in a few minutes; record the decision and move on.

Put every item on the board:

```sh
tools/agent/board add <item> --branch <branch> --base <base-branch> --depends <item,item> --note "<goal>"
tools/agent/board status
```

An item with two unmerged dependencies waits until one is merged.

## 3. Launch

Start 4–6 children at once. Fill [briefs/build.md](briefs/build.md) per item.

```sh
tools/agent/board ready        # item<TAB>stack-on=<branch>
```

- No dependency: `delegate_task`, `target.model` = `build`, `isolation: worktree` (cut from `$BOARD_INTEGRATION`).
- `stack-on=<branch>`: `delegate_task` into a new worktree on that branch's commits (or the item below's worktree), as soon as the item below has committed. Do not wait for its review.
- After starting: `tools/agent/board set <item> state=running worktree=<absolute path>`.

**Whenever a child finishes, and whenever only one is running,** run `tools/agent/board ready` and start what it lists before you wait for anything.

## 4. On each report

1. `tools/agent/board set <item> state=committed`; read the report (≤400 words). Check that no claimed check is missing from its commands.
2. Review at your judgement, by risk and by what the last round found. Skip for mechanical changes with passing checks. Otherwise start a `review` ([brief](briefs/review.md)) in the background and keep working. Diff: `git diff <base>...<branch>`.
3. Doubt about a finding: ask `judge`. Otherwise fix what is real.
4. Fixes go to the **same child** (SendMessage, or `delegate_task` into its worktree) with [briefs/fix.md](briefs/fix.md). Reuse it while its session is small; if the session has grown large, start a fresh `build` in the same worktree with a short handover.
5. Follow-up round: the review brief's round 2 checks only the listed fixes and what they touch. Stop when a round finds nothing that matters.
6. Live checks: start `live` ([brief](briefs/live.md)). It runs with `SILO_AGENT_ITEM=<item>` so its state belongs to the item.
7. Flaky tests: `check` appends them to `$SILO_BUILD_STATE/flaky.log` and edits no tracked file. At the end of every round move new lines into [FLAKY.md](FLAKY.md) (commit it in the integration checkout between merges; merge-batch needs a clean tree), remove them from the log, and make a fix item on the board for each.
8. A lower branch changed after fixes: `tools/agent/board restack <item>`. It stops with exit 2 on a conflict and names the files; give those to the child.
9. Reviewed or waived: `tools/agent/board set <item> state=reviewed`.

## 5. Merge

In `INT` (clean tree), merge reviewed items in dependency order, in one batch:

```sh
cd $INT && tools/agent/merge-batch --into $BOARD_INTEGRATION <branch> [<branch> ...]
```

- **Merges are recorded by `merge-batch`.** It sets `state=merged` on the board for each branch it actually merged. Never set `state=merged` by hand, and never filter its output away: a branch is merged only if `merge-batch` printed `merged <branch>`.
- **A branch needs a green `check` for its exact tree** (run it in the child's worktree). `merge-batch` refuses one without; `--force` merges it anyway (record why in the owner log).
- It merges with `--no-ff`, regenerates the API files once, runs `check` on the union in the foreground, and starts the full suite in the background.
- **The suite runs in `<INT>.suite`**, one fixed worktree reset to the exact SHA each run, so builds stay warm. Files: `app/SiloUI/target/agent/full-suite.<sha>.{log,merged,done}` there. A new suite cancels any older running suite of the same integration branch; its verdict says it was superseded. Edits in `INT` do not disturb the suite, but `merge-batch` refuses a dirty `INT`.
- It stops with exit 2 on a conflict and names the files. After resolving by hand (merge in `INT`, commit), run `tools/agent/merge-batch --suite`: it records the merged branches on the board and starts the suite on the current `HEAD` without merging or running `check`.
- The suite worktree gets the native runtime (`runtime-cache` on a hit, else a clone of `INT`'s `src-tauri/binaries` and `runtime`) and `INT`'s `github-build.local.json` (synthetic test values when absent). The isolation evals (Linux only in CI) and `reviewer-service` (no pytest and no cached venv) are skipped, not failed: the log and the verdict's `skipped:` lines say so.

Keep launching and reviewing while the suite runs. At the **next merge**, run `tools/agent/merge-batch --last` first: it prints the verdict, and on FAIL the failing tests (nextest, cargo test, unittest, pytest, vitest, node --test, cargo build errors) with their step and the merged branches that touched them. Send that to the child that wrote it; fix before merging more.

## 6. Clean

```sh
tools/agent/board cleanable
tools/agent/clean <item>          # dry run: shows what goes
tools/agent/clean <item> --yes    # then do it
```

`clean` refuses while an unmerged item depends on it. It removes the worktree, the merged branch, the temp data (`$TMPDIR/silo-agent/<item>`, temp homes, the board's `temp` paths) and sets `state=cleaned`. It never deletes computers: if a `live` state still lists some, it refuses; stop them first, or pass `--leave-computers` when they are not yours to stop. Run it right after each merge for everything `cleanable` lists, and remove any other temporary data you made.

**At the end of every build, and whenever free disk is low (`df -h ~`: under about 80 GiB), also run:**

```sh
tools/agent/clean --merged          # dry run: worktrees whose branch is merged into the integration branch or main and has no local changes
tools/agent/clean --merged --yes    # then do it; it also deletes those merged branches
```

It never touches a worktree with local changes or an unmerged branch.

## 7. Owner log

Append to the owner log (see Setup): every judgement call, guess, skipped review and decision the owner may want to change, one or two lines each, with the item. Never ask the owner and never block on the log.

## 8. How to wait

- Never end a turn unless something will wake it: a running child or reviewer (completion arrives as a notification), a background command, or a scheduled wake-up.
- Background command: run it with `run_in_background` and wait for its notification, or block on its `.done` file with one command. For a live check: `tools/agent/live wait <state-dir>`.
- Never poll CI and never push to see CI. The owner watches `main`.
- If nothing is running and `board ready` is empty, the build is done or blocked: say which in the final message.
