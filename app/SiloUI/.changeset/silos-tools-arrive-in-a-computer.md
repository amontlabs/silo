---
"silo-ui": minor
---

Silo's tools now arrive in a computer by themselves. A new Linux computer gets the `silo` command at `/usr/local/bin/silo` without anyone building or sending it, and Silo's tools are added to Claude Code and Codex there next to computer use. Silo checks both every time the computer starts: a command that was deleted or replaced comes back, and a fork gets its own tools on its first start. Turning the tools off takes them out of the agents again. If something cannot be put in place, `silo tools status` says what and why and the computer starts anyway. A development build made without Zig cannot install the Linux command and says so. On a Mac's computers the file with the tools' address is now readable only by the computer's own account.
