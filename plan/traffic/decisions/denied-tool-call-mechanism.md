# How Silo makes the error for a denied tool call exist

## The choice

An agent's part, 23 November 2026. You install an extension that judges each tool call in main.
Claude Code, on your Max plan, gets a reply in which the model asks for `rm -rf ~/projects/site`,
and the extension answers deny.

The options:

- The probe makes the call fail (recommended). Silo passes the reply on unchanged once the probe
  inside main is ready. Claude Code tries to run the command, and it fails. Claude Code then sends
  the model its own call with the error, as it does for any command that fails. Nothing in the
  traffic changes, then or when the conversation is resumed. Claude Code's window shows the
  model's real command failing. It costs four things:
  - The probe runs inside main, where an agent with `sudo` can interfere with it. So this is a
    precaution, never a rule.
  - Until Claude Code sends the error, its other new commands fail too, such as a second call in
    the same reply or a subagent's command. They come back as errors, and the model runs them
    again.
  - The error is Claude Code's own, such as "Permission denied". It does not say that Silo denied
    the call, or why.
  - The denied command's text reaches main. A script that an earlier call left running could read
    it in Claude Code's transcript and run it.
- A harmless stand-in, the proposal of 22 November. Silo swaps the call's command for one that only
  says Silo denied it. On every later request of the conversation, Silo puts the model's own call
  back, with the denial as its result. It costs a change to a reply and to every later request,
  which Anthropic's terms may forbid on a Max plan. It also costs a stand-in for each tool of each
  program, and Claude Code's window shows the stand-in, not the model's call.
- Silo answers the model itself, polarzero's idea of 20 November. Silo keeps the reply from Claude
  Code, sends Anthropic the conversation with the call and the denial, and passes on the next
  reply. It costs one more model request for each denial, paid on your plan. That request carries
  your Max sign-in though Claude Code did not send it, and Anthropic's terms forbid a developer "to
  route requests through Free, Pro, Max, or Ultra plan credentials on behalf of their users".

Recommended: the probe. It is the only way that changes nothing in the traffic, and the error the
model sees is the one Claude Code itself sent.

## Why a precaution may rest on the probe

An agent's part, 23 November 2026.

- Denying a tool call is a precaution's (`rule-on-a-tool-calls-text`).
- The decision is still taken at the boundary: the reply waits there while the precaution answers.
  The probe only carries the denial out.
- A rule is enforced outside the computer (`where-a-rule-is-enforced`), and limits are set at the
  boundary (`where-limits-are-set`). Read this way, a precaution is not a limit, so it may rest on
  the probe.
- polarzero's reading, 23 November 2026: a denial of a tool call comes from an extension, not from
  the core of Silo.

## To find out

- On a Linux computer, the kernel comes from outside the computer's disk. Silo can build the part
  of the probe that makes a call fail into that kernel, and make the kernel refuse changes. An
  agent with `sudo` then needs a flaw in the kernel to remove it, not one command. What the probe
  says stays a report (`processes-from-inside-linux`).
- On a macOS computer, which main is first, the kernel is Apple's and is on the computer's own
  disk. The means there would be Apple's Endpoint Security, which can refuse a process start or a
  file open. What an agent with `sudo` can do to it is not tested.
- Where an agent program has hooks, a hook could ask Silo and refuse the call with Silo's reason,
  before the program tries to run it. The hook would decide nothing (`no-hook-for-a-denial`). If
  the agent removes it, the probe still makes the call fail, with the program's own error.
- Read on 23 November 2026, in an agent's own Claude Code session: Claude Code 2.1.330 runs each
  shell call as a new `/bin/bash -c` process that its own process starts, with the command inside
  an `eval '…'`. How it writes a file was not read.

## polarzero's way (20 November 2026)

polarzero: do exactly what an agent program's automatic mode does, and "simply return the denial
as the tool output", back to the provider.

Read as a mechanism: Silo does not pass the reply to the agent program. It sends the provider the
conversation with the model's call and a result that says denied, as the agent program would, and
passes the model's next reply to the agent program.

An agent's notes on it:

- For that step the model sees exactly what `denied-tool-call` asks: its own call, unchanged, with
  an error.
- The agent program never receives the denied call, so it cannot run it.
- The agent program keeps the conversation, and its copy holds neither the call nor the denial.
  From its next request on, the model sees only what it wrote afterwards, unless Silo adds the two
  entries back to each later request.
- Silo sends a request the agent program did not send, with the program's sign-in in it. To read
  against the terms, with `terms-on-changing-a-reply` and `anthropic-reads-intermediate`, and
  against "It does not imitate the agent program" of `forwarded-byte-for-byte`.
- Not tested.

## polarzero's answer (21 November 2026)

polarzero holds that returning the denial to the provider as the tool's result works as it does in
an agent program, and sees one real issue: Silo sending a request of its own. It is decided later,
in the Workshop.

An agent's note, not shared by polarzero: the provider keeps no conversation, so the agent
program's later requests carry neither the call nor the denial unless Silo adds them. To test when
this is built.

## A probe on Linux, tried (24 November 2026)

An agent's part, 24 November 2026. What the run of `denial-per-program` taught about the probe
itself, on Linux. Three ways were tried.

- **fanotify permission events** (`FAN_OPEN_EXEC_PERM`, `FAN_OPEN_PERM`). They worked, and the
  kernel gives the real path. But an open event does not say whether the file is opened for
  reading or for writing. So the probe had to refuse every open outside a list of folders, reads
  included. That broke more than the denied call: Codex reads its certificate file again when it
  builds the connection for its next request, was refused, and logged "failed to build HTTP client
  with custom CA". Through a real boundary, that next request would have lacked Silo's certificate.
- **seccomp user notification** (`SECCOMP_RET_USER_NOTIF`). It worked, and it sees the flags of
  each open, so it refuses writes and lets reads through. The filter is set when the agent program
  starts, every process the program starts inherits it, and no process can remove it, root
  included. Two costs:
  - The path is read from the program's memory, not resolved by the kernel. Claude Code writes
    through `/proc/self/fd/<n>/<name>`, and a first version of the probe let those writes through
    until it resolved such paths itself. The program can also change the path between the check
    and the open. This is fine for a precaution, and it would not be for a rule.
  - Every exec and every open for writing waits for the probe, armed or not. Codex 0.172.0 caused
    about 700 such waits in a short run, Claude Code about 80.
- **A BPF program on the kernel's security hooks** (`lsm/file_open`, `lsm/bprm_check_security`). It
  would see both the real path and the mode of each open, and cost nothing when unarmed. The kernel
  had it built in, but the container refused to load the program, so it was not tried. In a Linux
  computer Silo builds the kernel, so it can be turned on there.

Two findings for whoever builds the probe:

- **Arming and disarming each need an answer from the probe.** In the run, the fake provider
  passed on the next reply right after it wrote "disarm". In two runs, the probe read "disarm" only
  after that reply had been passed on. Had the reply asked for a command, it would have failed
  though nobody denied it. Silo has to wait for the probe to say it is disarmed before it passes
  on the next reply, as it waits for the probe to say it is armed.
- **The window ends on the result, as planned.** Both programs sent the result in their very next
  request, so the window lasted well under a second in every run.

## End to end through a spike of the boundary (24 November 2026)

An agent's part, 24 November 2026. The probe of 24 November (seccomp, see above), driven by a
spike of the boundary in Zig (see `forwarded-byte-for-byte`), not by the fake provider.

The spike held back a reply from the event where a tool call starts until the event where it
ends, and put the whole call to a precaution: deny when the input names `existing.txt`. On a deny,
it told the probe to arm and waited for its answer before it passed on the reply, unchanged. When
a request carried the result of that call, it told the probe to disarm and waited for its answer
before it forwarded the request.

- Claude Code 2.1.331: a denied `mv /work/site/existing.txt …` did not run. The model got
  `EPERM: operation not permitted, posix_spawn '/bin/bash'`, the next call ran, and a resumed
  conversation showed both.
- Codex 0.172.0: a denied `exec_command` and a denied `apply_patch` failed as in the first test.
  A third call, allowed, ran.
- The probe answered in 0.05 to 0.2 ms.
- A reply was held for as long as its tool call took to arrive, and no longer: about 180 ms for a
  call of 35 pieces sent 5 ms apart. The program got the call all at once at its end, and the
  reply took no longer as a whole.

## A call with no id, in Google's format (24 November 2026)

An agent's part, 24 November 2026. The window ends when a request carries the result of the denied
call, found by the call's id. In Google's format the model may give a call no id. Gemini CLI 0.71.1
then makes its own, such as `run_shell_command_1795545668931_0`, and sends the result under it. The
spike, matching by id, never saw the result: the probe stayed armed, and the allowed calls after
the denied one failed too. Matching a result by the call's name when the call had no id fixed it.
When the model gives an id, Gemini CLI sends it back unchanged. Two calls to the same tool in one
reply, with no ids, cannot be told apart this way; that was not tried.
