---
"silo-ui": minor
---

`auto_keep:` rules in `.silo/silo.yaml` (and `fold: auto-code`) keep a fork's changes without asking when every change is in the buckets a rule lists and, if the rule asks, the recipe's `test:` command passes in the fork. They apply to `silo fold --request` and at the end of `silo fork ... -- cmd`; anything else, and any fork with uncovered database changes, left-behind credentials or a stale review, gets a review request.
