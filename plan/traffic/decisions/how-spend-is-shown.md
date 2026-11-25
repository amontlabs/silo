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
