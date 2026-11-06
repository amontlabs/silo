---
"silo-ui": minor
---

Pro: `silo review <fork|request>` (and `silo requests show --review`, and the MCP `review` tool) sends a fork's diff (and, for the code lens, a redacted text of its code changes) to a hosted reviewer you configure with `SILO_REVIEWER_URL` or `~/.config/silo/reviewer.yaml`, and shows general, packages and code findings under an advisory note. It is off until configured. The service is in `reviewer-service/`.
