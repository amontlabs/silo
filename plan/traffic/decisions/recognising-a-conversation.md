# How a conversation is recognised in each provider's traffic

## The choice

An agent's part, 22 November 2026. One Claude Code conversation in main has a budget of 5 dollars.
It has spent 3 dollars when Claude Code compacts its long history. Later you go back two turns and
try again.

The options:

- By what each request holds, joined by the program's session (recommended). Going back two turns,
  and a retry, stay in the same conversation. After the compaction the conversation goes on at 3
  dollars, because both requests carry the same Claude Code session, and the app shows it as one. It
  costs a start at zero when a program has no session of its own or changes it. It also rests on a
  test of each program's real traffic, still to run.
- By what each request holds, alone. Each compaction starts a new conversation with its budget at
  zero. It costs the budget on the long conversations, the ones most likely to run away, since
  Claude Code compacts them again and again.

Recommended: joined by the program's session. A budget on a conversation then holds through its
compactions, and where a conversation does start again at zero, the budget on main still holds.

## Notes

To find out, with whether a program can get around a budget on a conversation by starting a new one. A budget on a computer has no such way around.

## A conversation forked across computers, measured (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-forked-conversations`. Claude Code 2.1.332 and Codex 0.172.0 each took
one conversation up in a fork of main, a fork with a new machine id, a fork of a checkpoint, a fork
of a fork, and a fork that then became main, against a stand-in provider.

- **Nothing in the traffic tells the two sides apart.** Every identifier was the same on main and
  on each fork: Claude Code's session id and `device_id` (kept in `~/.claude.json`), Codex's
  `session-id`, `thread-id`, `prompt_cache_key` and `installation_id` (kept in
  `~/.codex/installation_id`), also with a new machine id. So "a conversation belongs to one
  computer" rests on the computer alone, which Silo knows without asking the program.
- **Where a fork's conversation comes from.** In every case the fork's first request continued,
  by content, the last request its disk's source had sent before the fork. What the answer could
  add: a fork's request that starts a new conversation is looked up among the requests that the
  computers its disk came from (its parent, and theirs, at any depth) sent before its disk was
  taken; when it continues one, the fork's conversation is linked to that conversation at that
  request. The record shows the link, and the fork reads none of what its parent sent after.
- **The link carries the cache.** The provider's prompt cache is per account, not per computer:
  the fork's first request was 99% cache reads of main's cache. An estimate that starts a fork's
  conversation with no cache priced it 7 to 9 times too high.
- **A fork that becomes main** (`fork-replacing-main`): main's next request continues main's
  old conversation by content, at the turn before the fork, unless Silo treats main's disk as
  the fork's from then on. It must: the conversations of main after the replacement are the
  fork's, linked as above.
- **The program's own fork inside one computer.** `claude --resume <id> --fork-session` and
  `codex exec fork <id>` send a new session id, and their first request continues the original
  by content, so the answer keeps them in the conversation. Codex also sends
  `forked_from_thread_id` and `forked_from_ordinal_exclusive` in `client_metadata` on every
  request of the fork: a link the program states itself, which holds after a compaction.
