# What Silo sends home

## The choice

An agent's part, 24 November 2026. You install Silo and run three agents for a week. Silo has seen
every process, file change, connection and model call in main. Nothing in the plan says whether any
of that, or a count of it, leaves your Mac for Amont Labs. `nothing-to-set-up` already says Silo
needs no account, no sign-up and no key. It says nothing about what Silo sends.

The options:

- Nothing unless you turn it on (recommended). No usage data and no crash report leave the Mac by
  themselves. The one request Silo makes on its own is the check for a new version, which carries
  the version and nothing else, and can be turned off. A crash report is offered when Silo crashes:
  you see what it holds, and you send it or not. It costs knowing how many people use Silo and which
  features they use. Downloads, stars and what people tell us are the only counts left.
- Anonymous counts, on by default, with a setting to turn them off. Silo sends counts such as the
  number of computers started and the version, with no content. It costs the first impression: a
  tool that records what agents do and also reports on its user will be asked why, and "anonymous"
  has to be proven each time.
- Counts on, with a question at the first run. Silo asks once whether to send counts. It costs one
  more question at the first run, which `nothing-to-set-up` keeps to the network rules.

Recommended: nothing unless you turn it on. Silo asks people to trust it with a record of
everything their agents do. A record that stays on the Mac is part of that promise, and telemetry
switched on by default is one of the complaints that comes back at the launch of every developer
tool of this kind. In the launch threads of comparable developer tools, read on 24 November 2026,
it comes up beside sign-in walls and closed source.

## What it does not cover

- What you send yourself: a record you export, a report you share, an ask that reaches your phone
  through a relay you set up (`forwarding-to-a-phone`).
- What the agent programs send. Claude Code and Codex send their own telemetry to their vendors
  (`reading-each-agent-program`). That is traffic of the computer, which the boundary sees and your
  rules allow or refuse.

## The phone app's pushes (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-ask-reaches-a-phone/`. To rule. Apple sends a push to an iPhone app only
from a server that holds the publisher's push key. Even a self-hosted ntfy server sends its iOS
wake-ups through ntfy.sh for this reason, and without that, messages can wait hours. So
`phone-app` needs a relay that Amont Labs runs. With the ask sealed to the phone, the relay learns
that a paired Mac sent something to its phone at a given time, and its size.

The case: an agent pushes at 23:10 while you are out, and your phone buzzes. The options:

- Amont Labs' relay, used only once you pair a phone (recommended). Pairing is the act that turns
  it on, as a crash report is sent only when you choose. It costs a service Amont Labs keeps
  running, and timing that leaves the Mac.
- Your own relay. It still reaches the iPhone through a service holding a push key, so it moves
  the timing to another party rather than keeping it home.
- No push. The phone app checks when you open it. It costs every ask you do not look for, which
  then waits until you open the app.

Recommended: Amont Labs' relay, used only once you pair a phone. "What it does not cover" above
would then name it beside the asks you forward yourself.
