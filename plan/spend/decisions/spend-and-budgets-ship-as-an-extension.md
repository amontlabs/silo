# Spend and budgets ship as an extension

## The choice

An agent's part, 27 November 2026.

The case: you run agents on a subscription and never want a budget. Today the plan puts budgets,
the price table and the daily fetch of prices in the part `traffic`, so they are part of what
every user gets.

The options:

- An extension that ships with Silo (recommended). The core keeps what only it can do: reading
  model traffic at the boundary and recording each call as facts. Dollars and budgets are added by
  an extension, through the manifest every extension uses (`shipped-extensions-use-the-manifest`).
  A budget stays a rule, as `budget-rule-or-precaution` says: the extension adds the rule and the
  boundary enforces it. It makes `what-silo-sends-home` simpler, since the fetch of the price table
  belongs to the extension. It costs a manifest that can carry rules on model calls and a price
  table, which no entry describes yet.
- Part of the core, as the entries are filed today. It costs every user a price table and its
  fetch, and it puts in the core a feature that takes a position on how you work.

Recommended: an extension that ships with Silo.

## polarzero's words

polarzero, 27 November 2026, asked whether budgets and receipts should become extensions: "I'll
say basically yes spend specific stuff typically is an extension".

polarzero, later the same day, shown this entry: "anything from model call that is a hard fact
deserves to be a fact, stuff that we can have as report can be useful, but to me anything about
pricing, budgets, price tables, dollars, really seems like an extension to me."

## What it covers

An agent's part, 27 November 2026. The entries about dollars, prices and budgets were in the part
`traffic`. They are now in the part `spend`, which holds this extension: the budgets, the prices,
spend in dollars, and the team's budget and spend. Their answers did not change with the move.

To check: `extension-spend` and `classifier-spend` say an extension's spend is "metered like any
computer". Counting its model calls stays the core's. Whether "metered" there means dollars is to
settle.
