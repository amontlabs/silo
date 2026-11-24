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

An agent's part, 24 November 2026, from the prototype in `research/2026-11-24-morning-review`. The test of real traffic this
entry waits on, for Claude Code 2.1.331 and Codex 0.172.0 against a scripted model. Claude Code
does not resend its history as it was: it drops a leading `cd <dir> &&` from earlier tool inputs,
and inserts a message after the first turn. A test of "this request holds the last one, then
more" split every Claude Code conversation after turn 1. Comparing a tool call by its id and name
only, and joining on the program's session (`metadata.user_id` for Claude Code,
`prompt_cache_key` for Codex) when the tools are the same, kept each of the four conversations
whole. It supports the recommended option, and adds that the prefix test has to leave out what
the program rewrites.
