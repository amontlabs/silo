# What does a set of rules say a destructive call does

## The choice

An agent's part, 24 November 2026. Four calls in one night. Neon `DELETE /projects/{id}`: Neon
keeps a deleted project 7 days. A Stripe refund with a live key: the money leaves. Vercel removes
a deployment: the site is down until the next deploy. GitHub `PATCH /repos/{o}/{r}` with
`"private": false`: nothing is lost, but anyone can copy the code from that moment.

The options:

- Five outcomes, and hold all but an outage (recommended). Each operation in a set says gone,
  recoverable, money, exposed or outage. The first four are held, and so is any call that turns
  off a protection. An outage alerts and passes. The ask says the outcome, and for a recoverable
  one what the provider says brings it back and for how long. It costs asks for calls the
  provider could undo, such as the Neon delete above.
- Hold only what cannot be undone: gone, money and exposed. It costs the cases where the
  provider's undo is real but out of your reach in the moment: Railway's 48 hours of a deleted
  volume can only be used from a link in the owner's email.
- Four outcomes, without exposed. A repository made public is counted as an outage or not held.
  It costs the one case that loses nothing and still cannot be taken back.

Recommended: five outcomes, and hold all but an outage. The ask is where the provider's undo is
worth saying, and a repository made public cannot be made private again for whoever copied it.

## An agent's notes

- The outcomes of about 400 operations, with sources, are in the catalogues of
  `research/2026-11-24-destructive-calls/details/` (paas, data, clouds, saas).
- What a provider says it can bring back is a report of the provider, not a fact Silo saw, so the
  ask shows it as the provider's word (`facts-before-reports`).
- The calls that turn a protection off include RDS `ModifyDBInstance` with
  `DeletionProtection=false`, S3 `PutBucketVersioning` suspended, Atlas termination protection
  off, Turso delete protection off, and `SkipFinalSnapshot=true` on an RDS delete.
- Thirteen exposing operations, with their wire forms, are in
  `research/2026-11-24-destructive-calls/details/undo-paths.md`: a GitHub repository or gist made
  public, S3 public access block removed or a public ACL, a GCS `allUsers` binding, a public
  Supabase or R2 bucket, Vercel deployment protection off, Firebase rules, Cloudflare Access app
  deleted, a Netlify password removed, an npm publish, a public Docker Hub repository, a Drive file
  shared with anyone who has the link.
- npm has its own hold for a publish: staged publishing keeps a version hidden until a maintainer
  approves it with a second factor. A token that may only stage, held by Silo, makes every publish
  wait for that approval.
- On the five APIs measured on 25 November 2026 (Railway, Fly.io, Vercel, Supabase, Cloudflare),
  102 of the 341 documented writes that should be held are exposed, and about half of them give
  access to someone named rather than to anyone: `flyctl tokens create`, `vercel teams invite`, a Vercel
  firewall bypass, a Supabase database opened to `0.0.0.0/0`, SSL enforcement turned off. #353's
  rules held 8 of the 102. The answer's "readable by anyone" leaves out a token made for an
  agent's own use. Proposed wording: "Exposed: it gives access or data to someone who did not
  have it, such as a new token, an invitation, a public bucket or an opened network."
  `research/2026-11-25-destructive-calls-platforms/README.md`.
- A second, blind labelling of the same 1,085 writes, under the wording in force, called 11 of
  those 102 exposed and 74 of them access given to someone named. Applied as written, the
  proposed wording above also holds tokens an agent mints for its own run: `railway ssh` mints one
  through `generateShellToken`, a Vercel build asks for its project's OIDC token. That is 86
  operations more than today. Proposed instead: "Exposed: it makes something readable by anyone,
  or gives lasting access to someone who did not have it: a new long-lived token or key, an
  invitation or a role, the project moved to another account, data sent to an outside address."
  Removing 2FA, a passkey or changing the account's email fit no outcome; they are a protection
  turned off. Paid resources created (a Postgres cluster, a disk grown, a read replica, static
  IPs) are the outcome the rules miss most: 17 pass. `research/2026-11-25-second-labeller/README.md`.
