---
"silo-ui": minor
---

`silo fork -n N` makes several forks at once (capped by `fork_ram_budget` and Apple's two macOS VMs), `silo fork <source> -- <cmd>` runs a command in the fork, `silo fork ./repo` copies a git repository into a fresh Linux fork and `silo fold` brings only its commits back (with `silo undo` to go back), and `silo fold --request` records a review request in the engine.
