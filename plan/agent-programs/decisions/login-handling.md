# Do agent logins need special handling

Decided on 10 November 2026.

How the handling works is part of `logins`, which is open.

## Found by an audit, 22 November 2026

An agent's part. This says a login does not end up in forks by accident, but a fork copies the disk,
and a subscription login is a file on it. Claude Code is signed in on main and you fork main for
something risky: the agent in the fork holds the token, though `what-a-fork-is-given` says a fork
gets no secrets. The audit proposes that `what-a-fork-is-given` says: "A login that an agent program
keeps on the disk is copied with the disk: Silo shows it before the fork is made."

The question is put to polarzero in `fork-and-sign-in`, under "The choice".
