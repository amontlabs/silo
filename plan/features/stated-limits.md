## Proposed

- Every claim
  - Rule: Every public security claim names the mechanism that enforces it, the adversary it holds against and what it does not stop.
  - Rule: Every public security claim separates what code enforces from what the agent is only told.
  - The docs list every override: what Silo, an update or a default can change without the user.
  - The docs say what Silo's maker can see of a user's computers and work and what Silo sends off the device, even when the answer is "nothing".
- What Silo trusts
  - Docs: The security page names each part Silo trusts and has not proven (the hypervisor and VM runtime, Apple's framework, the guest and host kernels, pinned toolchains, bundled binaries, the hardware).
  - Docs: For each trusted part, the security page says what a break would cost.
  - Docs: The security page explains in plain words what trusting the kernel and trusting the hardware mean.
  - Risk: A VM escape can happen (Firecracker, reported 6 November 2026), so the design makes one gain little, through network rules and secrets that are never inside.
- What is not stopped
  - The docs say that prompt injection through anything the agent reads (an issue, a web page, `gh view`, a skill or package registry) is not prevented.
  - The docs say that the rules bound what an agent can do after a prompt injection.
  - Docs: For drop, restore and merge, the docs state what the operation undoes and what it cannot, such as anything already done outside the computer.
  - Docs: The threat model says an attacker plants access and waits, so nothing is trusted for having been quiet.
  - Find out whether a link opened from a computer can trigger a Mac app through a custom URL scheme or x-callback-url, and state the result in the threat model.
- Advice to users
  - The docs advise leaving nothing in a computer that you do not want stolen.
  - Docs: For running agents hard, the docs advise a device with nothing too sensitive on it or very strong network rules.
  - The docs give a checklist for after a compromise: wipe the computer, rotate secrets, turn on two-factor, check the router and set canaries.
- Other
  - Docs: The threat model says that what an agent program does on its provider's servers is outside what Silo can see or show.

## From the old code

The old code had it built and how much can be reused is not known.

Lessons:
- Restoring a checkpoint replaces the disk, not what the agent already sent.
- One maintainer, so fixes can take longer than the acknowledgement.

Where it lived, at the tag legacy-2026-11-16:
- `docs/THREAT-MODEL.md#L1516`
- `SECURITY.md`
- `README.md`
