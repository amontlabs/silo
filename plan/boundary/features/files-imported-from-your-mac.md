# Files imported from your Mac

## The choice

An agent's part, 22 November 2026. You have a folder of mockups on your desktop and an agent in main
that should build from them. You bring the folder into main. It is copied once.

The options:

- Build it (recommended). The agent works on its own copy, and nothing of your Mac stays shared. It
  costs disk for the second copy, and the two copies drift: a mockup you change on your Mac
  afterwards is not in main until you import it again. Bringing a file back from a computer to your
  Mac is not in this entry.
- Leave it out. A file reaches a computer through git or a download from inside it. It costs you a
  detour for every file that is only on your Mac.

Recommended: build it. Only you start it and it is a copy, so it gives the computer exactly what you
chose and nothing after.

## polarzero's words (21 November 2026)

> User machine to computer really should be when user specifically wants to import something.

## An agent's notes

- Only you start an import. polarzero did not say whether an agent may ask for one.
  `what-an-agent-inside-may-do` says that anything that would grant a computer more asks the user,
  so an ask would fit it. Until polarzero says, this entry has no ask.
- It is a copy made once, so `nothing-shared-from-your-device` holds.
- `agent-programs-moving-in` is the same act at the first run, offered for agent programs, settings,
  skills and repositories.
- polarzero did not speak of the other direction, a file from a computer to your Mac.
  `files-through-a-channel` covers both.
- Stage: placed in the app's stage beside `ways-out-shown` by an agent. polarzero did not say when.

## From option A's first-run transcript

Agent, 25 November 2026, from `research/2026-11-25-first-run-option-a/` (gap 2 there). #373 moves this feature out of option A, with `look` and `put-back`, as small once the disk reader exists. Import does not use the reader: #370's `drivers/import.zig` is a copy over SSH. Without it, the first run ends with no repository in main, and you copy them by hand (`rsync -a ~/work/app main:work/`), which also copies the `.env` files Git ignores that #354's import left on the Mac. Recommended: keep it in A. Also found: repositories copied in keep `git@github.com:` remotes, and main has no key for them (Silo's entry forwards no SSH agent). In A the way is HTTPS with `gh auth login` inside main, which leaves a GitHub token in main where every agent can use it. The import could say so, and offer to switch the remotes to HTTPS.
