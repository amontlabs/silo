# What happens to the processes an agent program starts for its own work while a denied call is answered

## The choice

An agent's part, 24 November 2026. You work with OpenCode in main. A precaution denies
`rm -rf ~/projects/site`. The probe makes the command fail, as `denied-tool-call-mechanism` says.
OpenCode then runs `git` to take a snapshot of the work folder, as it does after each tool call.
That `git` fails too, since every process OpenCode starts in that window fails, and OpenCode ends
the session with "Unexpected server error".

The options:

- They fail, and the program waits for its check (recommended). The probe stays as simple as it
  is. A program that stops on such a failure does not pass `denial-per-program`. When the vendor
  documents a setting that avoids it, the app offers it and says what it turns off: for OpenCode,
  `"snapshot": false` turns off its own undo, which Silo's checkpoints cover. It costs these
  programs clean denials until you say yes or the vendor changes the program.
- The program's own commands are let through. The data file of each program names the commands it
  runs for itself, such as `git` for OpenCode, and the probe lets those run in the window when the
  program starts them directly. It costs a hole: a tool that runs `git` without a shell would get
  through, and the probe has to tell the program's own work from the agent's.
- Silo turns the setting off itself. It costs a change to the program's settings that you did not
  ask for, which `unmodified-agent-programs` avoids.

Recommended: they fail, and the program waits for its check. The window exists so that nothing of a
denied call runs; letting some commands through makes that depend on each program's habits.

## Notes

### What was seen (24 November 2026)

An agent's part, 24 November 2026, from the research in
`research/2026-11-24-every-agent-program`, with the spike of the boundary and the stand-in probe of
`denied-tool-call-mechanism`, on Linux.

- OpenCode 1.19.6: the denied shell call failed with `EPERM … posix_spawn '/usr/bin/bash'`. Then
  `git` failed twice (`posix_spawn 'git'`), and the session ended. With `"snapshot": false` in
  `~/.config/opencode/opencode.json`, the same scenario ran to the end: the denied calls failed,
  the allowed calls after them ran.
- Claude Code 2.1.331 also runs `git` after a write. That `git` failed with no message, and Claude
  Code carried on.
- Gemini CLI, Qwen Code, Cline and Goose started nothing of their own in the window.
