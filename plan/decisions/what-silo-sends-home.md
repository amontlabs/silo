# What Silo sends home

## The choice

An agent's part, 24 November 2026. You install Silo and run three agents for a week. Silo has seen
every process, file change, connection and model call in main. Nothing in the plan says whether any
of that, or a count of it, leaves your Mac for Amont Labs. `nothing-to-set-up` already says Silo
needs no account, no sign-up and no key. It says nothing about what Silo sends.

The options:

- Nothing unless you turn it on (recommended). No usage data and no crash report leave the Mac by
  themselves. The one request Silo makes on its own is a daily fetch of one public signed file
  that names the latest version of Silo and of its price table. It carries nothing, not even the
  version you run, and can be turned off. A crash report is offered when Silo crashes:
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

## The price table rides on the same request

An agent's part, 25 November 2026, from `research/2026-11-25-price-channel`.

`where-prices-come-from` recommends that the app fetch Silo's signed price table, so that a budget
counts Gemini 4 Carbon at its new price from 1 December. That is a second request home, which the
answer above, as first written, did not allow. The research joins the two: one file, the index,
names the latest version of Silo and the latest table with its hash, and the app fetches it once a
day at a random minute. A new table is a second fetch, of a file every Mac fetches alike.

The version check needs no version either. The app reads the latest version in the index and
compares it with its own. Measured on a local server: the request for the index is the path, a
fixed `User-Agent: Silo`, and three fixed headers. Two installs at different versions, with
different tables and histories, sent byte-identical requests. Common clients send more by default:
curl and Python name their own version, and Node sends `sec-fetch-mode` and keep-alive. On a Mac,
`URLSession` names the app's build and the Darwin version unless told not to (not measured here:
no Mac). What is left is what any connection gives: the address, the time, and the TLS handshake.

Where the file lives decides who sees even that. On GitHub Pages or a raw file of the public
repository, the requests reach GitHub, and Amont Labs gets no log of them. On a server of Amont
Labs, or a CDN it rents, it gets a log by address and time, which counts installs. A GitHub release
asset shows a public download count, which also counts them. Recommended: a static file on GitHub
Pages, with any other address only as a mirror, since every file is checked by its signature
wherever it comes from. That Pages gives the owner no per-request log was not verified beyond
GitHub's documentation.

What this answer does not cover, and should name once decided: the recipes Silo ships update
(`recipe-audit`) and would ride on the same index; the Ubuntu image (`computer-image`) and Claude
Code (`claude-code-fetched-by-recipe`) are downloaded from Ubuntu and Anthropic, not from Amont
Labs, when you make a computer that needs them.

A property for polarzero to word, if the answer is taken: "A request Silo makes on its own carries
nothing that tells one Mac from another." The test `test_two_macs_send_the_same_bytes` in the
research checks it.

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
