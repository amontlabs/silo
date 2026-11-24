# Does the boundary read the traffic to the hosts a set of rules names

## The choice

An agent's part, 24 November 2026. An agent working in main finds a Railway token in a file of a
repository and sends one request with `curl` to `backboard.railway.app`, which deletes the
production volume. This is what happened to PocketOS in April 2026. Silo holds no secret for
Railway, so today it does not end that connection: it sees a few hundred bytes go to an allowed
host, and the record keeps their size.

The options:

- Read every host a set names (recommended). When you switch on a set of rules that names Railway's
  API, the boundary ends the encrypted connection to its hosts and reads each request, as it does
  for package registries. It costs a certificate the computer has to trust for these hosts too,
  and a program that refuses Silo's certificate cannot reach them at all.
- Read only where Silo swaps a secret. The set acts only on requests made with a secret you gave
  Silo. It costs the PocketOS case: the token was already in the computer, so the delete passes
  unread.
- Deny these hosts unless a secret is held for them. Nothing reaches Railway except through a
  secret Silo holds. It costs every program that brings its own login, such as a CLI you signed
  in to inside the computer.

Recommended: read every host a set names. It is the way `registry-traffic-read` already reads
registries, and it is the only option that catches a token the agent found by itself.

## An agent's notes

- Measured on 24 November 2026: eleven real CLIs (`aws`, `gh`, `git`, `railway`, `vercel`, `fly`,
  `wrangler`, `stripe`, `supabase`, `neonctl`, `heroku`) all accepted a test authority from the
  system store or from their usual variable (`NODE_EXTRA_CA_CERTS`, `AWS_CA_BUNDLE`), and all
  worked over HTTP/1.1 when only HTTP/1.1 was offered. `research/2026-11-24-destructive-calls/details/cli-capture.md`.
- Railway also answers on `backboard.railway.com` and on `/graphql/internal`, so a set names
  every host of an API, not one. `details/detector.md`.
- The research behind this entry is in `research/2026-11-24-destructive-calls/`.
