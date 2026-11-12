---
"silo-ui": minor
---

When the agent inside a computer asks for something only you can allow (merging into its own computer, replacing main, a port on your device, SSH access, a secret, GitHub access, a fork with more access than it has, a container computer), the request now waits for your answer instead of simply failing. Silo notifies you, and `silo approvals` lists the requests; `silo approvals allow` or `deny` answers one. On allow, Silo checks the request again and runs it exactly as you saw it, once; the agent reads your answer and what came of it. A merge shows its changes through the fork's review request. Approval notifications have their own switch, on by default.
