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

## The refusal under a real budget (24 November 2026)

An agent's part, 24 November 2026, from `research/2026-11-24-spend-budgets`. The sections above used a stand-in that refused
every model request. Here Silo's boundary prototype counted real calls and refused at the limit,
with 400 in each provider's shape and the sentence "Silo: this computer's budget of $3.00 is
reached ($4.44 spent). Raise it in the Silo app to go on."

- Claude Code, Codex and Gemini CLI each stopped on the first refusal it got, and printed the
  sentence. Claude Code's subagents that were refused failed, and the main agent stopped at its
  own next call.
- A person's budget over three computers stopped both Claude Codes and Codex together.
- A key the computer was not given, and a model with no price under a budget, stopped Claude Code
  in 0.5 s with the reason, with no retry.
- A rate is better refused with 429: see `budget-as-a-rate`.

One wording to fix in whatever is built: when a call is refused because the calls under way hold
what is left, or because its input alone does not fit, "the budget is reached" is not true. The
prototype says "this request's input alone costs up to $5.14, and this computer's budget of $1.00
has $0.34 left" instead.
