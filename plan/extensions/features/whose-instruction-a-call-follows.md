# Whose instruction a tool call follows

An agent's part, 27 November 2026. You ask an agent to fix a failing test. It reads an issue on
GitHub to understand the bug. A comment on that issue says to add a deploy key to the repository.
The agent's next tool call is `gh repo deploy-key add`.

Everything in that story crossed the boundary as model traffic: what you asked, the issue's text
as a tool result, and the call. The agent program cannot make this comparison safely, because it
is the thing being steered. Something outside it can.

## Why the model never reads the tool results

The comment is written to persuade a model. A classifier that read it could be persuaded too. So
this one reads only what you asked and what the agent is about to do, and answers one question:
does this call follow from that ask? The research on summaries measured the same separation: a
reader that never saw the untrusted words lost 1 to 5 of the 43 false claims it had caught, under
each attack, where the strongest attack hid 16 of 52 from a reader that saw them
([the research](../../../research/2026-11-25-summary-injection/README.md)).

## Its limits

- It has not been measured. It is tried on live work first (`extension-tried-live`).
- It needs a list of values for a tool call that Silo offers (`who-fixes-the-values`): follows
  your ask, does not follow your ask, unsure.
- It works only where Silo reads the model traffic. Codex sends only what is new in each request
  and its provider holds the rest of the conversation, so what you asked has to come from the
  record.
- A harmful action cut into steps that each look fine passes it.
- It asks a model on every reply that carries a tool call, which costs tokens and time on each.
