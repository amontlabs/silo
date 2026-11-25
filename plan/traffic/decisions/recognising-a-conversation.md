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

Those requests were plain HTTP. Over Codex's WebSocket (see `reading-each-agent-program`), each
message after the first of a connection carries only the new items, and names the reply before it
in `previous_response_id`. That link is the only way to join them.

## A fork sends the same session (24 November 2026)

An agent's part, 24 November 2026. In the check of `conversation-resumes-in-a-copy`, a
conversation taken up in a copy of the computer and in its parent went on as two conversations,
but both sides sent the same session identifiers, for Claude Code and for Codex alike.

The answer above then gets one case wrong. Its first test, by content, says that a request
continues a conversation when its messages begin with an earlier request of that conversation and
the reply to it. The first request of each side passes that test, so both sides count as the same
conversation. The join by session puts them together again after a compaction. A budget on "this
conversation" would then count the spend of the fork too, and the app would show one conversation
where you see two.

What I would put in its place: a conversation belongs to one computer. The tests by content and
by session apply only to requests from the same computer, and a fork starts its own conversations,
each with a link to the one it was copied from. Silo knows which computer sent each request
without asking the program, so this costs nothing to read. Whether a budget set on the parent's
conversation should also cover the fork's is the owner's to decide.

## Ways around a budget on a conversation (24 November 2026)

An agent's part, 24 November 2026. The note above leaves open "whether a program can get around a
budget on a conversation by starting a new one". Claude Code 2.1.331 ran against a fake provider
whose model asked for a subagent, then for `claude -p 'Say hi.'` from the shell.

- **A subagent stays in the conversation.** Its requests carried the parent's
  `x-claude-code-session-id`, and the same id in `metadata.user_id`, plus a header of its own,
  `x-claude-code-agent-id`. Its messages did not begin with the parent's, so the test by content
  alone would start a new conversation; the join by session keeps it in. Codex names the parent of
  its subagents too: its code reads and sends `x-codex-parent-thread-id` and `x-openai-subagent`
  (seen in its binary, not run).
- **A program started from a command is a new conversation.** The nested `claude -p` sent a new
  session id, and nothing in its requests named the conversation that started it. By traffic
  alone, its spend falls outside the parent's budget. An agent that hits its budget can go on
  this way, by accident or not: running `claude -p` or `codex exec` from a command is common.

What I would add to the answer: Silo knows more than the traffic. It sees which process opened
each connection (`processes-in-the-record`), and the nested program runs under a command that the
parent conversation's program started. So a conversation whose program was started, at any depth,
by a command of another conversation's program counts toward that conversation's budget, and the
app shows it inside it. A budget on the computer needs none of this, as the note says.

## A compaction, seen in the traffic (24 November 2026)

An agent's part, 24 November 2026. The answer "rests on a test of each program's real traffic,
still to run" for compaction. Both programs were made to compact against a fake provider.

- **Claude Code 2.1.331**, `/compact` on a resumed conversation: the request asking for the
  summary, and the next request, carried the same `x-claude-code-session-id`. The next request
  began with "This session is being continued from a previous conversation…", not with the
  earlier messages, so the test by content alone starts a new conversation there and the join by
  session keeps it in, as the answer expects.
- **Codex 0.172.0**, with `model_auto_compact_token_limit` set low and signed in to OpenAI with a
  key: it compacts on the provider's side. The request asking for it is an ordinary request to
  `/v1/responses` whose last input item is `{"type": "compaction_trigger"}`, and it expects an
  item of type `compaction` back, an opaque `encrypted_content`. The next request carried the
  same `session-id` and `thread-id`, and its `x-codex-window-id` went from `<thread>:0` to
  `<thread>:1`. So Codex names each new window of a conversation itself.

So the join by session holds through a compaction for both, and Codex's window number tells Silo
where one window ends. The text of a Codex compaction is encrypted, so Silo cannot show what the
summary kept. The scripts are in the project files, `research/2026-11-24-around-a-budget`.

An agent's part, 24 November 2026, from the prototype in `research/2026-11-24-morning-review`. The test of real traffic this
entry waits on, for Claude Code 2.1.331 and Codex 0.172.0 against a scripted model. Claude Code
does not resend its history as it was: it drops a leading `cd <dir> &&` from earlier tool inputs,
and inserts a message after the first turn. A test of "this request holds the last one, then
more" split every Claude Code conversation after turn 1. Comparing a tool call by its id and name
only, and joining on the program's session (`metadata.user_id` for Claude Code,
`prompt_cache_key` for Codex) when the tools are the same, kept each of the four conversations
whole. It supports the recommended option, and adds that the prefix test has to leave out what
the program rewrites.
