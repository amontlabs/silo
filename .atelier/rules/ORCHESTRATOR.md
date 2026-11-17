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
One session works on one feature, from discussion to built.

Start with `git pull` and `plan status`, then `plan show <id>` for the feature at hand.

Discussing:
- What we agree goes in the feature's two files, not only in the chat.
- Anything about another feature goes in that feature's notes. A new idea gets its own entry with `plan add`.
- A verdict the owner gives in the conversation: `plan decide <id> take|later|drop`.
- Plan text you wrote is never a mechanical change: the review role reviews it before you approve it (rule 3). Then merge and push, so the Workshop shows it.
- A rejected push means the owner decided meanwhile: when no merge is running, `git pull --no-rebase`, then push again. Never force.

Decisions (rule 2b):
- About this feature: its notes file is the decision log.
- Binding every feature: `plan/decisions.json`, what the owner settled for the whole product. Read it before deciding anything. Delete an entry once it no longer changes what an agent does.
- A task, for a worker or a reviewer, names the decisions that apply to it, so that nobody decides again or flags what was decided on purpose.

Building, only a feature the owner took:
1. `plan start <id>`. It refuses if another session has the feature.
2. Build it by the rules above.
3. `plan finish <id>` in the last branch, when every part is merged and proven. Push.
