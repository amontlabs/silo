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
- The lasting wording above, applied call by call to #383's set and to #404's (5,546 writes of AWS,
  Google Cloud, Azure, GitHub and Kubernetes), changes 13 calls and 6. Of the 86 calls on #383's
  set that give access to someone named, 73 give lasting access, 9 mint a credential for the
  caller's run (`railway ssh`'s shell token, a Vercel build's OIDC token, R2 temporary
  credentials) and 4 give nobody anything new. On #404's set, 6 of 222 are for the run, such as
  `kubectl create token`. The request seldom says how long a credential lives, so a rule set
  needs a list of the operations that mint run tokens. `flyctl tokens create` is the exception:
  it sends `"expiry": "175200h0m0s"`, 20 years.
  `research/2026-11-25-lasting-access/README.md`.
- The answer does not say what a revocation is: a member or a role removed, a key or a token
  deleted, a webhook or log sink deleted. #383's and #404's labels call most of them outage, and
  both prototypes hold them. Listed by hand, they are 64 operations on #383's set and 102 on
  #404's, and the rules hold 57 and 95 of them. Case: at night an agent deletes an IAM access key
  it found in a log, then removes a member from the GitHub organization. Options:
  - A revocation passes (recommended). It is an outage at most. It removes 57 and 95 asks. It
    also lets an agent remove polarzero from the organization.
  - A revocation is held, as both prototypes hold every outage today.
  - A revocation passes unless it removes the user's own access. That needs to know which account
    on the provider is the user's, which Silo could learn from the credential it holds.
  If taken, the answer would add: "A call that only removes access someone had passes; when what
  used it stops, it is an outage."
