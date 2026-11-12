# Agents

Pick by name. Models, fallbacks and limits belong to lastdrop and the owner; do not name them. Rules: [AGENTS.md](../../AGENTS.md#agent-workflow).

| Name | For | Starts with | Give it |
|---|---|---|---|
| `build` | Implementation and fixes, one topic each | `delegate_task`: `target.model` = `build`; `isolation: worktree` for a new stack, or the item below's existing worktree for a stacked item | [briefs/build.md](briefs/build.md) or [briefs/fix.md](briefs/fix.md); it reads [CHILD.md](CHILD.md) |
| `build-hard` | Cross-crate, concurrency, migration items | as `build`, `target.model` = `build-hard` | as `build` |
| `review` | Code reviews of diffs and plans | `Agent`, `model` = `review`, background, read-only | [briefs/review.md](briefs/review.md): worktree path, `{DIFF_CMD}`, intent, risk areas |
| `judge` | Design calls, disputed findings, advice | `Agent`, `model` = `judge`, background | [briefs/judge.md](briefs/judge.md): the question, the options, the facts, the files |
| `live` | Runs `SILO_AGENT_ITEM=<item> tools/agent/live <check>` and reads its logs | `delegate_task`, `target.model` = `live`, in the item's worktree | [briefs/live.md](briefs/live.md): check name, host, the Dev engine it may use |
| `scout` | Read-only lookups, preflight facts | `Agent`, `model` = `scout`, background | [briefs/scout.md](briefs/scout.md): the questions and where to look |
| `cheap` | Cheap build or review; **only when the owner asks** | as the role it replaces | as the role it replaces |
| `computer-use` | GUI steps in macOS apps through LCU | `Agent`, `model` = `computer-use`; called by `live` or the orchestrator | One bounded task: app, steps, what to report |

## What every task contains

- The brief filled in completely; no placeholder left.
- The absolute worktree path and the base branch.
- The build state directory (`$SILO_BUILD_STATE`, absolute) for `build`, `build-hard` and `live`: `check` writes `flaky.log` there. A task without it uses the default next to the integration checkout.
- Where to write the report if it is long (a file in the worktree, outside Git).
- The sentence: "Your final message is the whole report; stop every background wait first and never end a turn to wait." (every brief already has it).
- Nothing about models, retries or CI.

## Notes

- Only the orchestrator starts `review`, `judge` and `scout`. Children never start reviewers.
- `review` and `scout` are read-only: they change no files.
- `live` and `computer-use` may use only the Dev build and throwaway `e2e-*` computers ([AGENTS.md](../../AGENTS.md#running-and-debugging)).
- A report that is only a pointer ("the report above stands") is lost: ask the child to resend it in full (SendMessage) before judging the item.
- Fixes go to the child that wrote the code, not to a new agent, while its session is small.
