# The boundary

The edge of a computer: its channels, and what happens to a request that waits there.

## For

To let agents work with full access inside a computer (`full-access-inside`) while what can
leave it, or reach it, is decided outside it, by the rules the user sets (`security-framing`,
`network-policy-by-the-user`). Rules an agent program sets for itself do not count
(`rules-inside-are-no-boundary`).

## Why it is so

These reasons come from the notes. They explain the part and decide nothing.

- **Ambient authority** is the name of the first problem: on your own machine every program you
  run can do everything you can.
- **A boundary is as strong as it is narrow.** A sandbox around a process shares your whole
  system. A container shares the kernel. A virtual machine shares a handful of virtual devices.
  With a virtual machine the boundary is rarely what fails. The channels opened on purpose are.
- **Why "hostile" is the right assumption.** An agent is steered by what it reads: a web page, an
  issue, a dependency. It becomes dangerous when it has three things at once: private data,
  untrusted content, and a way to send data out. Untrusted content cannot be removed.
- **Attention is scarce.** People who are asked often click yes, and the answer stops meaning
  anything.

## One example

A program in a computer connects to a host the computer never reached before. You are alerted
(`new-host-raises-an-alert`). No rule allows the host, so the connection is denied, and the record
keeps the attempt (`denied-attempts-are-recorded`). Had the host been allowed, the record would
show what was sent to it, or its size (`record-shows-what-was-sent`).

## What it holds

- In force: `security-framing`, `full-access-inside`, `rules-inside-are-no-boundary`,
  `isolation-for-speed`, `network-policy-by-the-user`, `what-was-sent-to-a-host`,
  `first-seen-host`, `denied-attempts`, `network-rules-from-use`, `asks-are-native`,
  `hold-limit`, `words-of-a-refusal`.
- A request at the boundary, to decide: `refusal-says-so`, `hold-sentence`,
  `classifier-wait-limit`, `what-an-ask-shows`, `what-an-ask-holds`, `opinion-on-an-ask`,
  `ask-reaches-you`, `program-gives-up-first`, `no-best-effort-wait`,
  `actions-that-never-wait`.
- What a computer can reach, to decide: `device-out-of-reach`, `reach-of-a-computer`,
  `paths-out-are-shown`, `guarantees-outside-the-computer`,
  `safe-fresh-install`, `channel-list`.
- Secrets, to decide: `secret-values`, `secret-placeholder`, `secret-entry`, `secret-limits`,
  `secret-and-prompt-cache`.
- The network, to decide: `network-modes`, `main-network-rules`,
  `package-age-rule`, `package-age-at-the-boundary`, `fork-with-network-closed`, `outside-actions-listed`,
  `no-read-freely-preset`.
- Answered on 22 November 2026, to decide: `hold-limit-length`, `how-a-hold-ends`,
  `reason-in-a-refusal`, `registry-traffic-read`.
- Later: the feature `reach-on-your-mac-today`.
