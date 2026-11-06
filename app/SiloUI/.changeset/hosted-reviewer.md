---
"silo-ui": minor
---

Pro: `silo review <fork|request>` (and `silo requests show --review`, and the MCP `review` tool) sends only a fork's diff to a hosted reviewer you configure with `reviewer.url` or `SILO_REVIEWER_URL`, and shows general, packages and code findings under an advisory note. It is off until configured. The service is in `reviewer-service/`.
