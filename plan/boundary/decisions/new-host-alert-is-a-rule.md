# What happens when a computer reaches hundreds of hosts for the first time

## The choice

An agent's part, 22 November 2026. You let main run open for a while to learn which hosts it needs,
and one package install reaches 200 hosts for the first time. You decided that each of them raises
an alert, and the plan also proposes that Silo never sends you a stream of alerts.

The options:

- Grouped, and yours to switch (recommended). The alert is a rule that ships switched on. You can
  narrow it or switch it off, and the 200 reach you as one alert with a count, as
  `alert-only-on-a-change` says of every alert. It costs the promise as it stands, since a new host
  stops alerting in a computer where you switched the rule off.
- Quiet while learning. A new host always alerts, except in a computer you set to run open to learn
  its rules, where new hosts are only marked in the record. It costs hearing of a host you would
  have refused during that time.
- One alert per host. Every new host alerts by itself, always. It costs you 200 alerts for that
  install.

Recommended: grouped, and yours to switch. You still hear about every new host, without the 200
alerts that would teach you to ignore them.

The audit's finding is in the notes of `first-seen-host`, which this decision replaces.

An agent's part, 25 November 2026, from `research/2026-11-25-morning-screen`. On #371's night with
a fifth run, 11 first-seen hosts over two computers: 8 are hosts an agent program reaches at its
start (Codex: github.com, api.github.com, chatgpt.com, ab.chatgpt.com), listed on the hosts view
and not in the queue; the other 3 are one line, ordered by bytes sent as #406 recommends. A host
counted as seen still needs a run beside it: api.github.com was both Codex's start host and the
host Claude Code's `curl` tried to open a pull request on, and traffic alone cannot tell the two
apart.
