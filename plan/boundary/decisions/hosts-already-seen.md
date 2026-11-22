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
