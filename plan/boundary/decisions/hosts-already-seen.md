# After a fork or a restore, which hosts count as already seen

## The choice

An agent's part, 22 November 2026. Main has reached 300 hosts and you fork it, or you restore main
to yesterday. Silo alerts you when a computer reaches a host for the first time, so it has to know
which of them are new.

The options:

- Fork inherits, restore forgets nothing (recommended). A fork starts with the 300 hosts, and a
  restore leaves the list as it is. It costs one signal, since you are not alerted when a fork
  reaches a host that only main had reached.
- A fork starts from zero. Every host is new again in each fork. It costs you the same alerts again
  in each fork.
- A restore rewinds the list. Hosts reached since that moment alert again as if never seen. It costs
  alerts about hosts you were already told about.

Recommended: fork inherits, restore forgets nothing. An alert should mean a host that is new to you.

## The hosts of the first run

Agent, 25 November 2026, from `research/2026-11-25-first-morning-hosts/` (#382's gap 4). This
entry says which hosts a fork and a restore start with. Main itself starts with the hosts the
first run reaches, and nothing says whether those count as seen. Measured here, the first run's
installers reach 12 hosts: Apple's software update hosts for the Command Line Tools (not measured,
no Mac), github.com, raw.githubusercontent.com, formulae.brew.sh, ghcr.io,
pkg-containers.githubusercontent.com and release-assets.githubusercontent.com for Homebrew, tmux
and Codex, downloads.claude.ai for Claude Code, and the three provider hosts the first run checks.

Recommended: they count as seen. Each was reached for the first time during the first run, before
any agent ran, so `new-host-raises-an-alert` gives their alerts then, on the first run's screen,
as one line, and the first morning starts with them seen. It removes 6 of the first morning's
about 57 first-seen hosts. The same rule then covers a fork: it starts with everything main has
seen, the first run's hosts included.
