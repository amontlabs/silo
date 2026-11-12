---
"silo-ui": minor
---

The engine now knows who is calling it: the app and command line on this device, another device through Connections, or the agent inside a computer. An agent inside a computer can work on itself and the computers it made, make and fork computers with access it already has, and is refused anything that reaches other computers or your device's settings; a call that would widen its access (a merge into main, a published port, a new secret, GitHub access) is answered as needing your approval. Every call that changes something, and every refused or held call, is recorded in an audit log that `audit.list` reads.
