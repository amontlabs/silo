# Does a budget deny or hold at each threshold

## The choice

An agent's part, 22 November 2026. Main has a budget of 50 dollars. You are out for the afternoon,
and an agent in main spends steadily.

The options:

- Deny, with an alert before (recommended). At 40 dollars you get an alert. At 50, main's agents get
  an error that says the budget is reached, and they cannot go on until you raise it. It costs an
  agent that cannot go on even when you are at your desk and would have said yes.
- Hold and ask you. At 50 the next model call waits, and you get an ask to let it pass. It costs a
  wait and an ask for every call while you are away: each call waits for the hold's limit and is
  refused anyway, and several agents make several asks.

Recommended: deny, with an alert before. A budget is a limit for the hours you are away, and the
alert gives you the chance to raise it while you are here.

## Whether the agent program shows the refusal (24 November 2026)

An agent's part, 24 November 2026. Measured on eight agent programs; the table is in the notes of
`how-a-hold-ends`. With a 403 that says the budget is reached, Cline, Goose, Gemini CLI, Codex and
Aider showed Silo's sentence. Claude Code, Qwen Code and OpenCode retried for the whole minute and
showed nothing. So the error the program gets is not enough to tell you: the alert in the app is
what says the budget is reached, and the record holds each retry as a refused request.
