# How a conversation is recognised in each provider's traffic

## The choice

An agent's part, 25 November 2026, from the challenge of #355 (page 6), with the prototypes of #371 and #347
and #373's second opinion.

The case: Claude Code runs with `--fallback-model claude-sonnet-5-5`, gets three overloaded errors,
and sends the same 40 messages to Sonnet. Under the earlier answer, a request continues a
conversation only "when it goes to the same model", so this one starts a new conversation, and a
budget on the conversation starts again at zero. The word `conversation`, "a sequence of turns with
one model", says the same.

The options:

- Drop "the same model": each turn's model is a fact of that turn, and the word changes with it
  (recommended). The test also leaves out what a program rewrites in its own history, and compares
  tool calls by id and name: #371 found that Claude Code 2.1.331 drops a leading `cd <dir> &&` from
  earlier tool inputs and inserts a message after the first turn, which split every Claude Code
  conversation after turn 1 until the test left those out. And a conversation belongs to one
  computer: #347 found that forks share the program's session ids, so without that sentence a fork
  and main would add up in one budget (#373: count a conversation on one computer).
- Keep "the same model", as the earlier answer says. It costs the fallback case.

Recommended: drop "the same model". The word `conversation` is set again by this entry, so taking it
changes the word that `words-for-agent-work` set.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

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
