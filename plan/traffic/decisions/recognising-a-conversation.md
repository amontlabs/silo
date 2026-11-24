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

## What captured traffic shows (24 November 2026)

An agent's part, 24 November 2026. Read from the requests of Claude Code 2.1.331 and Codex 0.172.0
to a fake provider, in the runs of `denial-per-program`. Short conversations only: no compaction,
no going back.

- **Claude Code** names its session in two places in every request: the header
  `x-claude-code-session-id`, and `session_id` inside the JSON string `metadata.user_id`, beside
  `device_id` and `account_uuid`. A conversation resumed with `claude --resume` kept the same
  session id.
- **Codex** names its conversation in the headers `session-id` and `thread-id`, in
  `prompt_cache_key`, and in `client_metadata`, all the same id. It sent no
  `previous_response_id`: each request carried the whole conversation (`store: false`).
- **Each request began with the one before it**, for Codex as sent. For Claude Code only once the
  format is read: its first request gave one message's content as a list holding one text block
  (a description of the machine that the test machine's own setup added),
  and every later request gave the same content as a plain string. Compared as the API reads them,
  string and single text block being the same, every request began with the previous one and its
  reply. So the comparison the answer describes has to read the format, as it says, and cannot
  compare bytes.

Those requests were plain HTTP. Over Codex's WebSocket (see `reading-each-agent-program`), Codex
does send `previous_response_id`: after the first message of a connection, each message carries only the new items and names the previous
reply in `previous_response_id`. The link is the only way to join them.
