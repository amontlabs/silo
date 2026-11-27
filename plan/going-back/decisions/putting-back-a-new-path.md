# Putting back a path the checkpoint does not have

## The choice

An agent's part, 25 November 2026. In the morning, Silo's list of what changed shows a new file,
`~/Library/LaunchAgents/com.sync.plist`, which Codex wrote at 03:10 and which runs
`curl … | sh` every time you log in. Claude Code's night in `~/work/site` is good. You run
`silo put-back 02:40 ~/Library/LaunchAgents/com.sync.plist`.

The options:

- Remove it (recommended). The preview says "removes ~/Library/LaunchAgents/com.sync.plist: not
  in 02:40". Silo takes a checkpoint of now, then removes the file inside main. Main is as it was
  at 02:40 at that path, and nothing else changes. It costs one more line in the preview, and a
  put-back that can now delete; the checkpoint before it is what makes that undoable, as for every
  put-back.
- Refuse it, as #370's specs do today ("A path not there is refused before anything happens").
  You remove the file yourself over SSH. It costs a way back that works for 8 of the 29 things
  an agent leaves to run later, measured below, and a removal the record does not tie to a
  checkpoint.

Recommended: remove it. "Put back this path as it was" includes "as it was: absent".

## Notes

Agent, 25 November 2026. From the research draft `research/2026-11-25-persistence/`.

On a Linux stand-in for main, a bad night left 29 things that run later: lines in shell start-up
files, crontabs, an `at` job, systemd units and their enable links, an autostart entry, git
hooks and `core.hooksPath`, `core.fsmonitor`, install scripts in `package.json`, `.npmrc`'s
`node-options`, a `.pth` file and `sitecustomize.py`, a program that shadows `git` on the PATH,
an SSH key, an SSH `LocalCommand`, `/etc/ld.so.preload`, a sudoers file, hooks in Claude Code's
and Codex's settings, an `AGENTS.md`, a VS Code task, an `.envrc`, and a live process.

Put back from the checkpoint before the night, as #370 specifies, brought 8 of 29 back: the ones
that changed a file that existed. The other 21 are new files, and #370's put-back refuses them.
With a path absent from the checkpoint removed instead, 29 of 29 came back.

#365's put-back of one run already removes the files the run created, under
`putting-back-a-runs-paths` (taken). This entry gives the plain put-back by path the same rule.
