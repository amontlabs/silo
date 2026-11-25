# How a fork is kept from acting as its parent

## The choice

An agent's part, 25 November 2026, from the challenge of #355 (page 3).

The case: you fork main to try a database migration. The agent in the fork runs
`prisma migrate reset` against `DATABASE_URL`, which points to your production database outside the
computer. The earlier answer says "the database an agent needs starts as it does in the parent",
which is right for a database inside the computer and says nothing of a remote one; every case in
#350's evidence used a remote one.

The options:

- A remote database is reached through the fork's own secret: a branch where the provider offers one
  (Neon, PlanetScale, Supabase), otherwise a credential that only reads (recommended). It costs a
  database secret that knows the provider, which #355 proposes as `database-secret`, and a fork that
  cannot write to a database with no branches.
- The fork gets the parent's credential, as the earlier answer allows by saying nothing. It costs
  the case above.

Recommended: the fork's own secret. A fork exists to try things, and a migration tried against
production is the case it should stop.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

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
