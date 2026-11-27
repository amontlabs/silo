# What you can add

Extensions: rules, a computer, or both, that someone made and you add to Silo.

## For

To let people watch a lot of agent work in their own way. What an extension executes can never
grant or destroy anything, and the rules it adds are shown before they are switched on.

## Why it is so

- **An extension that executes anything ships a computer** (`extension-code-in-its-own-computer`,
  `extension-computer-is-shown`). Its manifest names the recipe. What Silo only reads, such as
  rules, is not executed.
- **No tool it can use grants or destroys** (`no-tool-grants-or-destroys`).
- **Importing it changes nothing** (`importing-changes-nothing`). Installing it switches on what
  it showed.
- **Which tools it may use, that it may protect by itself, and what wakes it** are decided too
  (`extension-tools`, `extension-may-protect`, `what-wakes-an-extension`).

## What ships with Silo

- **Spend and budgets** (`spend-and-budgets-ship-as-an-extension`). Silo's core records each model
  call as facts, in the part `traffic`. This extension prices them from its price table and adds
  the budgets. A budget is a rule or a precaution, by what the boundary can see
  (`budget-rule-or-precaution`). What a budget promises when it is reached, and where prices come
  from, are still to decide (`what-a-budget-promises`, `where-prices-come-from`).

## Three examples

- **Registry rules.** No computer. It adds rules that allow `crates.io` on a computer. Importing
  it shows those rules, and installing it switches them on.
- **Disk watch.** Its computer wakes every minute, reads `/work` on main, creates a panel and
  alerts you. It can stop nothing.
- **Request check.** Its computer is asked when a network request of main waits at the boundary.
  It answers: let it go on, deny it, or hold it. It can never allow one that the rules deny.
