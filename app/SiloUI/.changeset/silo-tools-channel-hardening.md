---
"silo-ui": patch
---

Silo's tools in a computer are harder to misuse from the device. The computer's token no longer travels over the connection: Silo sends a one-time challenge and the computer answers with a proof that only works for that challenge on its own port. Restarting Silo ends every token and the computer gets a new one within seconds; `silo` inside the computer picks it up on its own. After you turn the tools off, Silo keeps the computer's old port closed to other programs until the computer restarts. If another program took a computer's port while Silo was closed, `silo tools status` says the computer needs a restart before it can use the tools again. A copy of the token in a fork or an export no longer works.
