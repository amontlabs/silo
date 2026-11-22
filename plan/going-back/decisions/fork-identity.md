# How a fork is kept from acting as its parent

## For the owner

An agent's part, 22 November 2026. Main runs a job every night at 02:00 that emails your team a
report, and a Tailscale node that puts main on your private network. At 18:00 you fork main to let
an agent try something risky.

The options:

- A new identity, scheduled jobs off (recommended). The fork gets its own name and address. The
  nightly job does not run in it and its Tailscale node is off, so your team gets one email and main
  stays the one on your network. Services the agent needs, such as its database, run. It costs a
  step when you want a job to run in the fork, and a service Silo does not know still runs as main's
  twin.
- Every service off too. Only what the agent starts runs. It costs the try itself: the database and
  Docker are down in the fork until the agent starts them, so the fork does not behave as main does.
- An exact copy, as copying a virtual machine by hand gives. It costs the nightly email twice, and
  two machines that claim to be main on your network.

Recommended: a new identity, scheduled jobs off. A fork is for trying something, not for doing
main's work twice, and the services an agent tests against keep running. The promise becomes "A fork
starts with an identity of its own, and runs none of its parent's scheduled jobs until you turn them
on", since "never acts as its parent" cannot be kept for a service Silo does not know.

## Notes

Git versions files; Silo versions something alive. A file does nothing when copied. A computer
has an identity, scheduled jobs, open sessions and logins. That is why a fork must not act as its
parent.
