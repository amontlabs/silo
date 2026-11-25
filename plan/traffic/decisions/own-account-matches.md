# Does each agent program's own account match the boundary

## The choice

An agent's part, 22 November 2026. You turn on the check against Claude Code's own account for main.
On 19 November, behind an ordinary inspecting proxy standing in for Silo, Claude Code 2.1.327
matched on every request: the same request id, model and token counts. Codex has not been compared
yet.

The options:

- Only where a run matched (recommended). For each program and version, a run of real work through
  Silo compares the program's account with what Silo saw, request by request. The check is offered
  only when every request matched, and the app says which programs have no such check. It costs the
  check for Codex and any other program until it is compared, and for a new version until it is
  compared again.
- For every program that gives an account. The check is offered at once. It costs false alerts: a
  program that leaves some of its own requests out of its account, or counts tokens another way,
  raises a mismatch on every such request, and you learn to ignore the alert that should mean
  something.

Recommended: only where a run matched. A mismatch is meant to be a rare sign that something went
around the agent program, and it means that only for a program whose account agrees with Silo when
nothing is wrong.

## Notes

To find out, for each program.

- In the test of 19 November 2026, Claude Code's own report matched the boundary on every request: the same request id, the same model, the same token counts. The request id joins the two accounts.
- Still to find out: the same for Codex and the other programs.

## Codex 0.172.0 against the boundary (24 November 2026)

An agent's part, 24 November 2026. Codex ran a conversation of four replies over its WebSocket,
through the spike of the boundary (`forwarded-byte-for-byte`), in front of a fake provider. The
fake provider gave each reply different, random token counts.

- Codex writes its own account in its session file (`~/.codex/sessions/…/rollout-*.jsonl`), as one
  `token_usage_record` per reply. Each record carries the reply's `response_id`, so the two
  accounts can be joined on it. `codex exec --json` also prints the turn's total.
- For the three replies that answered the conversation, Codex's counts were the same as the
  provider's, on every field: input, cached input, output, reasoning, total.
- **Codex leaves its warm-up out.** The first message on each socket is a warm-up
  (`"generate": false`, see `reading-each-agent-program`). The fake provider gave its reply token
  counts too, and Codex's account has no record of that reply. Whether OpenAI counts anything for a
  warm-up is not known.

This bears on the answer, which offers the check only when every request of the run matched: as
written, Codex fails it on the first reply of every socket. If a warm-up costs nothing, the check
could leave out replies to `"generate": false` and still mean what it should. That is for whoever
writes the check, once a warm-up's real cost is known.
