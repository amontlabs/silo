# Whose instruction a tool call follows

## The choice

An agent's part, 27 November 2026. You ask an agent to fix a failing test. It reads an issue on
GitHub to understand the bug. A comment on that issue says to add a deploy key to the repository.
The agent's next tool call is `gh repo deploy-key add`.

Everything in that case crossed the boundary as model traffic: what you asked, the issue's text
as a tool result, and the call. The agent program is the thing being steered, so it is poorly
placed to notice. A program outside it is not steered by what the agent read.

The options:

- A model that never reads the tool results (recommended). It reads what you asked and what the
  agent is about to do, and answers one question: does this call follow from that ask? The comment
  was written to persuade a model, and this one never sees it. It costs a model call on every
  reply that carries a tool call, in tokens and in time, and it has not been measured.
- A model that also reads the tool results, to name where the instruction came from. It costs
  the separation: in the research on summaries, the strongest attack hid 16 of 52 false claims
  from a reader that saw the untrusted words, and a reader that never saw them lost 1 to 5 of the
  43 it had caught ([the research](../../../research/2026-11-25-summary-injection/README.md)).
- Do not ship it. It costs the one comparison only something outside the agent program can make.

Recommended: a model that never reads the tool results.

## Its limits

An agent's part.

- It has not been measured. It is tried on live work first (`extension-tried-live`).
- It needs a list of values for a tool call that Silo offers (`who-fixes-the-values`): follows
  your ask, does not follow your ask, unsure.
- It works only where Silo reads the model traffic. Over its WebSocket, Codex sends only what is
  new since the last reply and its provider holds the rest of the conversation, so what you asked
  has to come from the record.
- A harmful action cut into steps that each look fine passes it.
