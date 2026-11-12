# Orchestrator: the loop

You plan, launch, review, merge and clean. Children build. Rules: [AGENTS.md](../../AGENTS.md#agent-workflow). Agents and how to start them: [ROLES.md](ROLES.md). Children follow [CHILD.md](CHILD.md). Every script has `--help`.

Variables below: `$BUILD` is the build's state folder (`docs/<build>/state/`, holds `board.tsv`, the owner log and the full-suite files); `$INT` is the integration branch (`main`, or `direction`).

## 1. Preflight

Start a `scout` ([brief](briefs/scout.md)) in the background for facts that would otherwise break an item late:

- release files and version numbers the items touch (`docs/SiloUI-RELEASES.md`, changesets);
- the toolchain: `rust-toolchain.toml` matches what `runtime:prepare` and CI need;
- hard-coded versions, paths or counts the items would have to change (pinned counts in `registry.rs` and `schema.rs`, API generator output);
- whether the Linux machine answers (`ssh 10.77.77.2 true`, 3 s limit).

Read its answer before planning. Fix a wrong toolchain or version first, as its own item.

## 2. Plan the graph

For every item write: name, one-line goal, **files it touches**, **depends on**. Rules:

- Disjoint files and no dependency: parallel, each in its own new worktree.
- Dependent, or same hot file (`registry.rs`, `application-source.ts`, `application-shell.tsx`, fixtures): stacked in one worktree, in order.
- One topic per item. A cross-crate, concurrency or migration item is `build-hard`; the rest `build`.
- Ask `judge` ([brief](briefs/judge.md)) for design calls you cannot settle in a few minutes; record the decision and move on.

Put every item on the board:

```sh
tools/agent/board add <item> --branch <branch> --base <base-branch> --depends <item,item>
tools/agent/board status
```

## 3. Launch

Start 4–6 children at once. Fill [briefs/build.md](briefs/build.md) per item. New stack: `delegate_task` with `isolation: worktree`, `target.model` = `build`; the worktree is cut from `$INT`. Stacked item: `delegate_task` into the worktree of the item below, as soon as that item has committed (do not wait for its review).

```sh
tools/agent/board set <item> running
```

**Whenever a child finishes, and whenever only one is running,** run `tools/agent/board ready` and start what it lists before you wait for anything.

## 4. On each report

1. `tools/agent/board set <item> reported`; read the report (≤400 words). Check that no claimed check is missing from its commands.
2. Review at your judgement, by risk and by what the last round found. Skip for mechanical changes with passing checks. Otherwise start a `review` ([brief](briefs/review.md)) in the background and keep working. Give it the diff: `git diff <base>...<branch>`.
3. Dispute or doubt about a finding: ask `judge`. Otherwise fix what is real.
4. Fixes go to the **same child** (SendMessage, or a new `delegate_task` into its worktree) using [briefs/fix.md](briefs/fix.md). Reuse it while its session is small; if the session has grown large, start a fresh `build` in the same worktree with a short handover in the brief.
5. Follow-up round: the review brief's second form checks only the listed fixes and what they touch. Stop when a round finds nothing that matters.
6. Live checks: start `live` ([brief](briefs/live.md)) with the check name; it runs `tools/agent/live <check>` and reads the verdict file.
7. A flaky test in a report: add it to [FLAKY.md](FLAKY.md) and make a fix item.
8. Lower branch changed after review fixes: `tools/agent/board restack <item>`, bottom-up, one at a time.
9. Reviewed or waived: `tools/agent/board set <item> approved`.

## 5. Merge

Merge approved items in dependency order, in one batch:

```sh
tools/agent/merge-batch <branch> [<branch> ...]
```

It merges in order, regenerates the API files once, runs the affected tests in the foreground, and starts the full suite in the background (`$BUILD/full-suite.<sha>.log` and `.done`). On failure it names the failing tests and the branch that touched those paths: send that to the child that wrote it. Do not wait for the full suite: keep launching and reviewing. At the **next merge**, read the previous `full-suite.*.done` first; if it failed, fix that before merging more.

## 6. Clean

```sh
tools/agent/board cleanable
tools/agent/clean <item>
```

`clean` refuses while a dependent branch exists. Run it right after each merge for everything `cleanable` lists, and remove any other temporary data you made.

## 7. Owner log

Append to `$BUILD/OWNER_REVIEW.md`: every judgement call, guess, skipped review and decision the owner may want to change, one or two lines each, with the item. Never ask the owner and never block on the log.

## 8. How to wait

- Never end a turn unless something will wake it: a running child or reviewer (their completion arrives as a notification), a background command, or a scheduled wake-up.
- Waiting on a background command: run it with `run_in_background` and wait for its notification, or block on its `.done` file with one command.
- Never poll CI and never push to see CI. The owner watches `main`.
- If nothing is running and `board ready` is empty, the build is done or blocked: say which in the final message.
