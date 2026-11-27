# Can a restarted engine take back its helpers

## The choice

An agent's part, 22 November 2026. Silo's engine crashes while main and two forks are running, then
starts again. The three computers kept running while it was away.

The options:

- Take them back (recommended). The engine finds each running computer again, and you can fork,
  restore and change rules as before, with nothing stopped. A computer it cannot reach counts as not
  observed, never as stopped. One it has no trace of keeps running under its rules, and you get an
  alert. It costs a trial nobody has run yet: whether a computer started by the engine can outlive
  it, on a Mac and on Linux. If it cannot for one kind of computer, those computers stop with the
  engine, and the question comes back to you.
- Stop them and start them again. The engine stops each computer it finds and starts it again from
  its disk. It costs every running program at each crash: the agent's job ends midway, which is what
  keeping computers running through a crash was for.
- Leave them as they are. They keep running under their old rules, and the engine does not touch
  them. It costs control after every crash: you can neither fork nor restore them nor change their
  rules, and stopping one means ending it by hand.

Recommended: take them back. Keeping computers running through a crash is worth it only if the
engine finds them again, and it never stops a computer it is unsure of.

## Notes

To find out, for Apple's framework and for the virtual machine monitor of Linux computers
(`linux-vm-engine`).

- A virtual machine lives in the process that created it.
- libkrun runs a virtual machine in the process that calls it. Fact as of 19 November 2026, from
  [the research in the notes of `linux-vm-engine`](linux-vm-engine.md).

## The first build's spec

Agent, 24 November 2026, from [the build specs of the first
build](../../../research/2026-11-24-first-build-specs/README.md). The specs make each helper a
launchd job of its own, so launchd ending the engine never ends a computer. A started engine
connects to `run/helpers/<id>.sock`, and both sides prove they hold the computer's key with HMAC
over a nonce. A helper that answers with the boot the engine last saw is taken back with nothing
done. If the Mac kit shows this cannot work, the engine stops each computer at its own start
instead, and the first build promises only that main runs while the engine is down.
