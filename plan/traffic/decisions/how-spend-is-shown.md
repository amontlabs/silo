# How is spend shown

## The choice

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
