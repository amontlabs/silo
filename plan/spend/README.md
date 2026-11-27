# Spend and budgets

An extension that ships with Silo: what model calls cost in dollars, the price table, and budgets.

## For

To know what your agents cost and to stop them at an amount you set, for those who want it. Silo's
core records each model call as facts, in the part `traffic`. Everything about dollars is added
here, so someone who never wants a budget gets no price table and no budget.

## Why it is so

- **It is an extension, not the core** (`spend-and-budgets-ship-as-an-extension`). It uses the same
  manifest as an extension someone else made (`shipped-extensions-use-the-manifest`).
- **A budget is a rule or a precaution, by what the boundary can see**
  (`budget-rule-or-precaution`): the same split as everywhere else in Silo, between what is exact
  and what can be wrong.
- **What a budget promises when it is reached, and where prices come from, are still to decide**
  (`what-a-budget-promises`, `where-prices-come-from`).

## One example

You set a budget of $20 on main. The extension prices each model call the core recorded, from its
price table. When what is left no longer covers a call, the rule it added stops that call at the
boundary, and the agent program gets an error that says why.
