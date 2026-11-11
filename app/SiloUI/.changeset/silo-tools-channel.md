---
"silo-ui": minor
---

Agents inside a computer reach Silo over a private network port of that computer with its own token, instead of a shared folder. `silo` and `silo mcp` work inside a computer with the same commands as on your device, and Silo decides what each call may do. New computers have Silo's tools from the start; turn them on for an existing computer with `silo tools enable <computer>` (it replaces `silo guest-client`), and off with `silo tools disable`.
