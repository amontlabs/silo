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

## Whether the agent program shows the refusal (24 November 2026)

An agent's part, 24 November 2026. Measured on eight agent programs; the table is in the notes of
`how-a-hold-ends`. With a 403 that says the budget is reached, Cline, Goose, Gemini CLI, Codex and
Aider showed Silo's sentence. Claude Code, Qwen Code and OpenCode retried for the whole minute and
showed nothing. So the error the program gets is not enough to tell you: the alert in the app is
what says the budget is reached, and the record holds each retry as a refused request.

## A refusal that every program shows (24 November 2026)

An agent's part, 24 November 2026, from the research in `research/2026-11-24-agent-programs-follow-ups`. This refines the section above.

A stand-in refused every model request, and only those, with one status in the provider's own error
shape and the sentence "Silo: this computer's budget is reached. Raise it in the Silo app." Calls
that are not model requests failed as they would without Silo. Eight programs, 45 seconds each, on
Linux.

- **400 was the only status every program stopped on and showed.** With
  `invalid_request_error` (Anthropic, OpenAI) or `{"error": {"code": 400}}` (Google), Claude Code,
  Codex, Gemini CLI, OpenCode, Cline, Aider, Goose and Copilot CLI with its own key all stopped
  within 9 seconds and printed the sentence. Goose sent the request 8 times first, the others once
  or twice.
- **With 403**, seven of eight showed it, but Aider retried for the whole run and Copilot printed
  its own message instead.
- **Some statuses make the program say something else.** On 402, Goose tells you to add credits
  with your provider. On 404, Claude Code says the model may not exist. On 401, Claude Code retries
  and shows nothing.
- Here Claude Code stopped on a 403 and showed the sentence. The section above saw it retry, with a
  stand-in that refused every request to the host, its sign-in and settings calls included. The
  likely cause of the difference is that, not the status.

So a refused metered request could answer 400 with Silo's sentence in the provider's error shape,
and the user would read it in the program, as well as in the app. A 400 says "your request is
wrong", which is not true of a budget, and a program may one day react to it by changing the
request. That is the cost. The alert in the app stays what tells you for sure.
