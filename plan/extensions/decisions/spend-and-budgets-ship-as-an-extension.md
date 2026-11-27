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
say basically yes spend specific stuff typically is an extension". That is weaker than a yes to
this entry, so the entry stays not decided.

## What it covers

An agent's part, 27 November 2026. No entry was moved. If polarzero takes this, these entries of
the part `traffic` describe the extension and move with it: the budgets (`what-a-budget-promises`,
`what-a-conversation-budget-promises`, `budget-denies-or-holds`, `budget-of-a-fork`,
`budget-as-a-rate`, `budgets-together`, `budget-and-work-out-of-sight`,
`budgets-in-dollars-or-a-window`, `budget-on-a-computer`, `budget-on-a-run`, `budgets-on-rates`),
the prices (`where-prices-come-from`), spend in dollars (`how-spend-is-shown`,
`spend-per-conversation`, `spend-split-and-forecast`, `program-reports-its-own-spend`) and the
team's (`pooled-budget-ledger`, `team-budget`, `team-spend`, `spend-the-team-sees`).

To check then: `extension-spend` and `classifier-spend` say an extension's spend is "metered like
any computer". Counting its model calls stays the core's. Whether "metered" there means dollars is
to settle with this entry.
