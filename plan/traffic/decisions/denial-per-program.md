# How each agent program takes a tool call that the probe makes fail

## The choice

An agent's part, 23 November 2026. An extension that protects main denies
`rm -rf ~/projects/site`, and the probe makes the command fail inside main. Nobody has yet tried
how Claude Code, or Codex, takes a command made to fail this way.

The options:

- Checked per program and version (recommended). A test denies one call of each tool. It shows
  that the program runs nothing of it, sends the error to the model, and goes on working, also
  after the conversation is resumed. A precaution can deny tool calls in a program only once it
  passes. On a version not checked yet, Silo does not pass on a reply whose tool call is denied,
  and the app says so. It costs a wait before tool calls are denied cleanly in a new program or a
  new version.
- Offered for every program at once. It costs a program that misreads the failure: it may stop, or
  try the same call again and again, and you find out in the middle of work.

Recommended: checked per program and version. A denial that breaks the program it protects costs
more than one that comes a little later.

## Notes

This entry was `changed-reply-per-program`, when a denial changed the reply. It was renamed on 23
November 2026, when the way proposed in `denied-tool-call-mechanism` became the probe.

Reading tool calls is tested (`reading-each-agent-program`). Making a call fail inside the
computer is not.

## What a first test found (24 November 2026)

An agent's part, 24 November 2026. A first run of the check this entry asks for, on Linux, with
stand-ins for the boundary and the probe. It is not Silo, and it is not a Mac.

### How it was run

- Claude Code 2.1.331 (`claude -p`, permissions skipped) and Codex 0.172.0 (`codex exec`, approvals
  and sandbox bypassed), in an Ubuntu 24.04 container with kernel 6.18, as root.
- Each program talked to a fake provider on the same machine, which played the model: its reply
  asked for one or two tool calls, and its next reply said "Done."
- The fake provider armed a stand-in probe just before it passed on a reply with a denied call, and
  disarmed it when a request carried a result for that call. That is the window
  `denied-tool-call-mechanism` describes.
- While armed, the probe made two things fail with "Operation not permitted" (EPERM) for the agent
  program and every process it started: starting a program, and opening a file for writing, apart
  from the program's own files (its home folder, `/dev/` and its temporary folder).
- The probe was a small C program that starts the agent program under a seccomp filter and
  answers its notifications (`SECCOMP_RET_USER_NOTIF`). A first version on fanotify gave the same
  results, see `denied-tool-call-mechanism`.
- The work folder held one file, `existing.txt`. Each denied call tried to remove, move or change
  it, or to add a file beside it.

### What each program did

| Tool | What ran | What the model got as the call's result |
|---|---|---|
| Claude Code, Bash | Nothing. Claude Code tried to start `/bin/bash` three times for the one call. | An error (`is_error: true`): `EPERM: operation not permitted, posix_spawn '/bin/bash'` |
| Claude Code, Write | Nothing. Claude Code writes a temporary file beside the target and renames it. | An error: `EPERM: operation not permitted, open '/work/site/new.txt.tmp.<pid>.<hex>'` |
| Claude Code, Edit | Nothing, the file was unchanged. | The same error, on `existing.txt.tmp.<pid>.<hex>` |
| Claude Code, Read | The file was read. The probe stops no read. | The file's content |
| Codex, `exec_command` | Nothing. Codex starts its own binary again (`/proc/self/exe`) and then `bash`; both were refused. | `exec_command failed: CreateProcess { message: "Rejected(\"Failed to create unified exec process: Operation not permitted (os error 1)\")" }` |
| Codex, `apply_patch` (change or add a file) | Nothing. Codex writes the file from its own process. | `Exit code: 1 … Failed to write file /work/site/existing.txt` |

For both programs:

- The program sent the error on its next request, once, and did not try the call again by itself.
  It went on, and both exited normally.
- A second call in the same reply, which was not denied, failed too, with the same error. This is
  the cost the entry `denied-tool-call-mechanism` names.
- The next call after the error, in a later reply, ran normally. Nothing stayed broken.
- Resumed later (`claude --resume`, `codex exec resume`), the conversation sent the model its own
  call and the error, unchanged. Claude Code kept the same session id.
- During the window, other work of the program failed too. Claude Code runs `git` after a Write;
  that `git` was refused, without any message.

### What the first test says about the answer

For the shell and file tools of these two programs, the four things the answer asks for held:
nothing of the call ran, the failure became the call's result, the program sent it once and went
on, and a resumed conversation shows the call and the error.

Two findings bear on how the check is written:

- **A tool that only reads cannot be made to fail.** A denied Read in Claude Code returned the
  file. The same holds, by how they work, for tools that only reach the network from the program's
  own process, such as Claude Code's WebFetch, and for tools the provider runs itself, such as
  Codex's `web_search`. These belong in the list of tools the probe cannot make fail.
- **Codex refuses some commands by itself.** With approvals and sandbox bypassed, Codex 0.172.0
  still refused `rm -f …` before running anything: "rm -f style commands are not permitted. Use a
  safer approach". A check that denies such a command would see Codex's refusal, not the probe's.
  The check needs a command Codex itself accepts, such as `mv`.

Not tested: a Mac, Claude Code's subagents, MCP tools, a shell left running in the background,
Codex signed in with ChatGPT (its model traffic then goes over a WebSocket), and how a real model
answers such an error.
