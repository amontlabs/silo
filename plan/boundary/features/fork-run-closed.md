# A fork run with its network closed

## The choice

An agent's part, 22 November 2026. You fork main to run the benchmark of a library, with the fork's
network closed. The run ends, and the fork's record shows that no connection left it from start to
end.

The options:

- Build it (recommended). You know the result did not depend on anything fetched during the run, and
  that nothing left the fork. It costs the runs that need the network: a test that downloads a
  package fails in such a fork.
- Leave it out. You close the fork's network yourself and trust that it stayed so. It costs the
  statement in the record, which is what lets you show the result to someone else.

Recommended: build it. Closing the network is already there, and the record saying so is the part
people cannot get today.
