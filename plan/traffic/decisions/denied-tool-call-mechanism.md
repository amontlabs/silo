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
