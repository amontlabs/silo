---
"silo-ui": minor
---

`.silo/silo.yaml` takes per-agent overrides under `agents:` (recipes, fork secrets, fold, auto-keep rules and step checkpoints), chosen by `--agent`, `SILO_AGENT` or the MCP client's name. A keep that brings packages or settings files appends them to the recipe the computer came from (`--no-write-back` skips it), and `silo fold <fork> --pr` pushes a fork's code to a new branch and opens a pull request with the review summary.
