# How is spend shown

## The choice

An agent's part, 25 November 2026, from the challenges of #350 (C2) and #355 (page 6), with #374's prototype.

The case: you pay for a Max plan. What you watch is the week: whether main will leave you enough of
it for the weekend. The earlier answer shows dollars at API prices first. One point of the week went
from $4.45 to $2.31 in a month, so no dollar figure means "30 percent of my week" for long. The
tools subscribers star most (CodexBar, ccstatusline, Claude-Code-Usage-Monitor) show the window and
the pace.

The options:

- For a subscription, the window first and the dollars beside it; for an API key, dollars
  (recommended). It costs a second way of showing spend. The window is the account's, as the
  provider reports it: #374 found that a share per computer cannot be read from any reply.
- Dollars first for everyone, as the earlier answer says.

Recommended: the window first for a subscription. It is the unit those users set limits in, and
`word-budget` now lets a budget be counted in it.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

An agent's part, 22 November 2026. One of main's conversations today cost 3.20 dollars at
Anthropic's API prices. You pay for main's model calls with your Max plan, and Anthropic says your
plan's 5-hour limit is 18 percent used.

The options:

- Dollars everywhere, and the plan's share for the plan (recommended). The conversation shows 3.20
  dollars, marked as at API prices, and your plan shows 18 percent used. One model call also shows
  its token counts, which explain its price. It costs a figure that is not what you paid on a plan,
  and is read as what the work would have cost.
- Dollars for an API key, a share for a plan, as proposed so far. It costs the conversation its own
  figure on a plan, since providers give the share for the whole account and not for one
  conversation.
- Tokens. It costs a figure that cannot be compared between models with different prices, and that
  no budget is set in.

Recommended: dollars everywhere, and the plan's share for the plan. One unit lets you compare
conversations and computers, and the share tells you when the plan runs out.

## A plan's window, per computer (24 November 2026)

An agent's part, 24 November 2026, from `research/2026-11-24-spend-budgets`. Claude Code signed in with a subscription worked
under a cap of 50% of the plan's 5-hour window, while other devices of the same account also used
it. The cap held: Claude Code stopped at 51%, from the account's own reading in each reply.

The computer's share did not. The boundary put on the computer the rise of the window between its
own calls: 0.19 of the window. The stand-in had counted 0.14 from this computer and 0.06 from the
others. A reading covers the whole account, so the app can show the plan's share for the account,
and a cap on it, but not a share per computer or conversation.

## A figure the computer wrote

Agent, 25 November 2026, from `research/2026-11-25-techniques-from-two-tools/`. An open-source
gateway once summed a cost found in a task's log, where the agent's own output was copied too. A
forged line claiming $999,999 refused every task until it aged out. A cost an agent prints is the
agent's claim: Silo shows it with the agent as its author, if at all, and never adds it to what
Silo counted.

## Prices that depend on where and how the call runs (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-model-prices`.

- Anthropic charges 1.1 times every token price for US-only inference (`inference_geo: "us"`) on
  Claude 4.6 and later. Claude on Bedrock and on Google Cloud costs 1.1 times as much on a regional
  endpoint as on the global one; AWS's own price list confirms it on every Claude model. OpenAI
  charges 10% more on regional endpoints for models released on or after 5 March 2026. Spend "at
  the provider's API prices" has to include these, and the research's boundary counts the first.
- Anthropic, OpenAI and Google sell volume discounts by contract. The research's table takes a
  discount you write in your own prices file. Whether the app shows that discounted figure, or the
  list price as this entry proposes, is still open. Showing the list price keeps conversations
  comparable between people. Showing the discounted one keeps a budget in the dollars you pay.

## A plan's share, provider by provider (25 November 2026)

An agent's part, from `research/2026-11-25-plan-windows`. What the app can show for one computer
on a plan, by what Silo sees:

- Anthropic, traffic read: the account's figures from every reply; the computer's own share only
  as #355's estimate.
- Anthropic, traffic passed unread (#393's `claude-key-or-plan`): no figure at all. Dollars only if
  Claude Code reports them, as `program-reports-its-own-spend` asks.
- OpenAI, ChatGPT sign-in: the account's figures from every reply. A figure per conversation exists
  only behind a separate call with the user's sign-in (`thread_usage/query_v2`).
- Google, Google sign-in: the plan counts model requests per day, so the computer's own share is a
  count Silo makes itself, exactly. The account's remainder is in what Gemini CLI fetches every 30
  seconds or more.

So "18 percent used" in the example is the account's, and a computer's share can be shown as a
fact only for Google.

## Spend a program reports about itself (25 November 2026)

An agent's part, from `research/2026-11-25-spend-report`, for a computer whose model traffic
passes unread and whose program reports its own spend (`program-reports-its-own-spend`). Silo
compares each reported call with the connection it saw, so the figure comes with the report's
state:

- Whole: "$4.12 today, as Claude Code reports it. Its 212 calls match the connections Silo saw."
- Silent: "$2.80 as Claude Code reported it until 02:14. Since then, 96 connections to Anthropic
  with no report; Silo estimates them at $1.90 from their sizes."
- Contradicted: "Claude Code reported $0.48. The connections Silo saw were larger than that: Silo
  estimates $4.34." The calls that disagree are listed with their sizes.

The estimate is shown apart from the report and never as a fact. A lower bound from sizes alone is
not shown: it came to a twentieth of the bill, and an image in a prompt breaks it.
