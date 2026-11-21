# Orchestrator rules

1. Start with `atelier status` and `atelier agents`. Fix a failed preflight line first, as its own item.
2. Plan items, one topic each. Per item: `atelier start <branch> --path <worktree>` (`--on <lower branch>` when it builds on one), then `atelier task build <branch> goal=… files=… done=… risks=… decisions=…` (or `build-hard`). Start that role's agent with the printed line as its prompt. Start an item once what it builds on has committed. When a worker finishes, start new work before you wait.
2b. Decide before the worker starts. Review does not catch what nobody decided (5 problems left with written decisions, 13 without). `decisions=` says how the item behaves at every entry point it touches (API, CLI, background jobs, recovery, import/export, retry, approvals, deletion, limits) and with older saved data. Answer from the decision log, then the docs; ask `judge` the rest. Never stop the build to ask the owner.
   Keep a decision log, one line each: what, who decided (you, judge or worker), the item, whether the owner should look. Copy in each worker's "Decisions I made".
3. On each report, read its tests and its `compile:` line. Review by risk; skip review for a mechanical change.
   - Round 1: `atelier task review <branch> intent=… risks=…`. Later rounds: `atelier task review <branch> --fixes findings=@<file>`.
   - Risky items (security, policy, persistence, recovery) get two independent round-1 reviews at high effort: 2-3x the findings of one default-effort review.
   - Stop when a round reports nothing critical or high. Fix medium findings at your judgement. Low findings never start a round.
   - Reviewers get the task, not the decision log. Fixes go back to the worker that wrote the code.
   - Review a stacked branch for its own commits only; its lower branch is approved on its own and merges in the same batch or earlier.
   - Then `atelier approve <branch> <the commit you reviewed>`.
4. Merge everything approved in one batch, as a background command: `atelier merge <branch>…`. It runs the full check on the merged tree. A branch that changes `.atelier/` merges alone. When a merge ends, the next batch takes everything approved since.
   - Exit 1, or 5 "with main": send the output to the branch's worker; for a conflict it runs `atelier follow` and resolves.
   - Exit 5 "with <earlier branch>": merge again without the later branch. Once the earlier one lands, the later branch's worker runs `atelier follow`.
   - Exit 7: leave the main checkout alone; run `atelier promote` once it is clean.
   - "main moved": run the same merge again.
   - "<branch> is behind a rewrite of <lower>": that branch's worker runs `atelier follow`.
   - "gate worktree has unknown files": keep what someone saved, remove the rest, merge again.
   - "a promotion was interrupted": stop merging, change nothing in the main checkout, give the owner atelier's list.
   - "rules changed": re-read these rules; tell running workers to `atelier follow` and re-read.
4b. Worktrees are the default, not a requirement; you decide.
   - Several small items with disjoint files may share one worktree and branch, reviewed, approved and merged as one. Tell each agent its files; one commit per item, so a failing one can be dropped.
   - A change only to files that no build, test or check reads (top-level `docs/` and Markdown outside the app; not bundled help) may be committed straight to `main` by one agent, after checking its relative links, and only while no merge is running: a commit on `main` voids a running merge.
   - Anything else goes through `atelier merge`, however small.
5. A test `atelier status` lists as flaky is a fix item, with its failing output, before anything else.
6. Clean right after every green merge, before starting new items: `atelier clean`, read the plan, then `atelier clean --yes`.
7. Done when everything is merged and no promotion is pending.

Agents: pick by role from `atelier agents`. Effort: low for lookups and mechanical edits, medium for an ordinary item, high for cross-crate, concurrency, migration or security work and for reviews of risky diffs. Never name models, retries or CI in a task. Reuse a worker for its fixes while its session is small; otherwise start a fresh one in the same worktree with a short handover. A report that only points elsewhere ("the report above stands") is lost: ask for it in full.

## Silo
One session works on one entry of the plan, a decision or a feature, from discussion to done.

Start with `git pull` and `plan`, then `plan show <id>` for the entry at hand: it prints the entry, what it links to and what binds it.

Discussing:
- A proposal is shown to the owner in the conversation first, and written into an entry once the owner has reacted.
- What we agree goes in the entry's file and its notes, not only in the chat.
- Anything about another entry goes in that entry's notes. A new question or idea gets its own entry: a file in its part, and its place in `plan/order.json`.
- A statement the owner has not taken is never written into a list: it sits under `sets` of the entry that proposes it.
- A verdict the owner gives in the conversation: `plan take <id>` or `plan drop <id>`. "Not now" is a move to a later stage of `plan/order.json`.
- Commit the plan files straight to `main` and push, so the Workshop shows it. No branch, review or approve. First run `tools/ci/check.sh plan`, then `links`, then `build` (seconds each; they read `plan/`), and commit only while no merge is running. Plan edits made while building stay on the build's branch.
- A rejected push means the owner decided meanwhile: when no merge is running, `git pull --no-rebase`, then push again. Never force.

Decisions (rule 2b):
- About this entry: its notes file is the decision log.
- Binding every entry: the plan's lists (`words.json`, `principles.json`, each part's `properties.json`) and every taken decision that is not replaced. `plan show <id>` prints the ones that bind an entry. Read them before deciding anything.
- A task, for a worker or a reviewer, names the properties and decisions that apply to it, so that nobody decides again or reports as a defect what was decided on purpose.
- A challenge to a decision, yours or one from a worker's or a reviewer's report, goes to the owner in the conversation as soon as you have it. Write it and the owner's answer in that decision's notes. When the owner agrees, the change is a new decision, as "Changing what was taken" in `plan/README.md` says.

Building, only a feature the owner took:
1. `plan show <id>`: every entry it needs is done.
2. Build it by the rules above. Each property it adds gets a check, and the property's `at` names that check's file.
3. Write `built` with today's date in the feature's file, in the last branch, when every part is merged and proven. `plan check` refuses it until the feature's needs are done and each property it adds has its `at`. Push.
