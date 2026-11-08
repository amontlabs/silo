---
"silo-ui": minor
---

Share a whole computer as a file with `silo export` and `silo import`. Exports no longer carry the guest image when the computer uses the one Silo pins, so they are smaller, and a fork's record of what it started from travels with it, so an imported fork can be kept into your own main with `silo fold <computer> --into main`. Before a computer is exported, Silo names the logins it finds on it (a Claude Code, Codex, Gemini CLI or GitHub CLI login, cloud credentials, SSH keys) and asks, in the command line and in the app. Files are never removed from the export.
