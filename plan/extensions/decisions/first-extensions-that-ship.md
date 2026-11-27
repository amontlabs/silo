# The first extensions that ship

## The choice

An agent's part, 27 November 2026. polarzero's principle is a strong core with few opinions, and
opinionated extensions on top, some shipped with Silo. `mechanism-or-use` gives the test. Spend
and budgets were the first to move (`spend-and-budgets-ship-as-an-extension`). What ships next
decides what Silo looks like on the first day. It also tests the extension interface, since each
of Silo's own extensions has to be written against it.

The options:

- Four that each use a different part of the interface (recommended).

  | Extension | What it needs that the others do not |
  |---|---|
  | `said-compared-with-done` | Reads the record, gives an opinion, calls a model of its own |
  | `whose-instruction-a-call-follows` | Is asked while a model's reply waits |
  | `transaction-guard` | Reads a request's body, holds with an explanation, reads the response |
  | The rules of `destructive-calls-held` | No computer: rules only |

  It costs four things to build well before anyone outside writes one.
- Spend and budgets only. It costs an interface that only one extension, made of rules, was written against.
- Everything that takes a position, at once: receipts, which run did it, package age. It costs
  the first build.

Recommended: four that each use a different part of the interface.

Receipts stay a view of the core for now. A receipt is made from the record, and which lines
matter in it is a position, so it belongs with these. No tool lets an extension write a document
or add output to the `silo` command yet. To find out.
