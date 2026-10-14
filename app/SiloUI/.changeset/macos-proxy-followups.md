---
"silo-ui": patch
---

macOS computers now count their network setup as part of being set up, so a setup that failed halfway can be retried. Silo checks that the guest really trusts its certificate, follows split-DNS and network changes, falls back to TCP for large DNS answers, and removes a deleted computer's certificate key.
