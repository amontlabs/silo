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

### The program's own commands, let through by their command line (24 November 2026)

An agent's part, 24 November 2026, from the research in `research/2026-11-24-agent-programs-follow-ups`. This tries the second option and
challenges the recommendation.

The stand-in probe was given a list of command lines, as prefixes. In the window, an exec whose
whole command line starts with one of them runs. Every other exec is refused, as before, and
everything the process that runs writes is judged as before: outside the program's own folders,
refused. For OpenCode 1.19.6, three prefixes covered all 15 processes it started in the window,
all of them `git` on its own snapshot folder:

```
git rev-parse --path-format=absolute --git-path info/exclude
git -c core.autocrlf=false -c core.longpaths=true -c core.symlinks=true -c core.quotepath=false --git-dir <home>/.local/share/opencode/snapshot/
git --git-dir <home>/.local/share/opencode/snapshot/
```

With them, the denied `rm` failed with `EPERM`, the session went on to the end with snapshots on,
and `existing.txt` stayed.

The hole the option names is narrower than it looks. A shell call starts `bash -c …`, so it never
matches a prefix that starts with `git`: a prefix is matched from the first word. A `git` that
does match cannot write the work folder, run a hook, a pager or a helper, or reach the network,
since each of those is a write or an exec that the probe judges on its own. What is left is a tool
that runs `git --git-dir <OpenCode's snapshot folder> …` directly, without a shell, which can then
write only inside that folder.

A rule that disarms after the first refusal was tried too, and fails: Copilot CLI 1.0.115 starts
its own `bash -l` to read the login shell's environment before the call's shell, so the first
refusal would be its own, and the call would run.

Seen in the window, with nothing let through: Claude Code started 9 processes of its own (`git`,
`rg`, `env`, its shell snapshot) and carried on; Copilot started 1 and carried on; Codex, Gemini
CLI, Cline and Goose started none.

So I would change the recommendation to the second option, with the prefixes as a field of the data
file (`agent-program-data-file`), shown by the live check of `denial-per-program` for each
version. It keeps OpenCode's undo, which is what its users rely on, without a setting changed for
them, and the window still refuses every write that matters.
