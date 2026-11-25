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

## Which error each program reads (24 November 2026)

An agent's part, 24 November 2026. The error at the budget's limit answers the program's own
request to its model. How Claude Code 2.1.331 and Codex 0.172.0 take each status is measured in
`how-a-hold-ends`, in the boundary part. In short:

- Claude Code sends a 402 once and shows "API Error: 402" with Silo's words. 402 means payment
  required, which is what a budget at its limit is. It tries a 429 or a 5xx again and again, and
  shows a 403 as a failed sign-in.
- Codex sends a 402 six times, then shows it. It sends a 400 with a plain-text body once and shows
  the words as they are. A `response.failed` with the code `insufficient_quota` is also sent once,
  but Codex then shows its own words, "Quota exceeded. Check your plan and billing details.", which
  point you to OpenAI's billing instead of Silo's budget.

So the error is 402 for Claude Code and 400 with plain text for Codex. Each retry the program
makes anyway must get the same error at once.
