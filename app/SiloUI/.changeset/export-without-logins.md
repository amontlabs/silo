---
"silo-ui": minor
---

An export of a Linux computer now removes its known login files from the computer in the file. Silo makes a short-lived hidden copy of the computer, removes the sign-ins it knows (Claude Code including `~/.claude.json`, Codex, Gemini CLI, the GitHub CLI, cloud and registry credentials, SSH keys), exports the copy under the computer's own name and drops it. Traces of the removed files may remain in the file's disk layers, so keep an export private either way. The dialog before an export lists the files, says they are removed and that traces may remain, and has a choice to include them; `silo export --include-logins` does the same on the command line. The result and `--json` say which files were removed (`leftOut`). Exports of a checkpoint, of a macOS computer and of a container computer still keep the logins in the file, and still warn.
