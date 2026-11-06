---
"silo-ui": patch
---

Fixes for the in-guest client: `silo guest-client install --binary` now uploads the binary before it creates a token, so a failed upload leaves no token. Non-root users in a computer can reach the bridge. Forking or promoting a running computer no longer fails after its agent has called the bridge. Every fork drops the guest-client token file it copied from its parent. Promoting a computer whose name is longer than 11 characters now works.
