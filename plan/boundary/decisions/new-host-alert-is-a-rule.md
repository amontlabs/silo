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

## From the first morning's hosts, measured

Agent, 25 November 2026, from `research/2026-11-25-first-morning-hosts/` (#382's gap 4). That
page rebuilt #382's first night from connections that were measured: the hosts the first run's
installers reach (fetched here), #350's `npm install` and `pip install` logs, #350's 20
documentation pages with two links followed from each, Codex's four hosts at start (#371) and
Claude Code's documented hosts. The paste upload is the scenario's and was not sent.

- The first morning holds about 57 hosts seen for the first time (54 to 60 over 20 runs of the
  link-following), not the "about twenty" #382 estimated. Most come from following links in
  documentation. A second night of the same work, with the first night's hosts seen, holds about
  11.
- Counting the installers' hosts as seen removes 6 of them. Adding the hosts each agent program's
  vendor documents removes 9 more. Only #350's shipped sets of registries, download hosts and
  documentation sites bring the morning to about 21.
- In every run, and in every way of counting hosts as seen, the paste site comes first when the
  first-seen hosts are ordered by the bytes sent to each. In alphabetical order it is 9th to 35th.
- Each of the 49 hosts measured sent back more than three times what it was sent. paste.rs
  was sent 1,750 times what it sent back.

What this suggests for the rule, if polarzero takes it: the alert for one night's first-seen hosts
is one line with its count, ordered by the bytes sent to each host. A first-seen host that was sent
more than it sent back gets a line of its own. Each host still raises its alert, so
`new-host-raises-an-alert` holds as it reads. Shipping the rule already narrowed by the hosts an
agent program's vendor documents is a change to the rule's text ("switched on until you narrow
it"), so it is polarzero's call; the page recommends it, and recommends against shipping #350's
documentation set as seen, since that set was chosen from popular sites and includes shared ones.

A host counted as seen still carries data out. After the first run, github.com, api.github.com,
ghcr.io and storage.googleapis.com are seen, and anyone with an account can upload to each of
them. A log sent to a gist raises no first-seen alert in any of these ways; only the record's bytes
per host (`record-shows-what-was-sent`) show it.

An agent's part, 25 November 2026, from `research/2026-11-25-morning-screen`. On #371's night with
a fifth run, 11 first-seen hosts over two computers: 8 are hosts an agent program reaches at its
start (Codex: github.com, api.github.com, chatgpt.com, ab.chatgpt.com), listed on the hosts view
and not in the queue; the other 3 are one line, ordered by bytes sent as #406 recommends. A host
counted as seen still needs a run beside it: api.github.com was both Codex's start host and the
host Claude Code's `curl` tried to open a pull request on, and traffic alone cannot tell the two
apart.
