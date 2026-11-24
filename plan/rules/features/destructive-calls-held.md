# Destructive calls held for your yes

## The choice

An agent's part, 24 November 2026. You switch the set on for main. In the night an agent finds a
Railway token in a file and sends `mutation { volumeDelete(volumeId: "…") }`. The request waits.
The ask says: main wants to delete the Railway volume `pg-data` of project `shop-prod`; Railway
says it keeps a deleted volume 48 hours, restorable only from a link in the owner's email. You do
not answer before the hold ends, and the agent reads Railway's own error form with Silo's sentence
in it.

The options:

- Build it (recommended). One set covers the APIs agents reach most, holds what deletes data,
  moves money or makes something public, and leaves the rest alone. It costs a list per provider
  that has to follow their changes, a certificate the computer trusts for those hosts, and asks
  at night for calls the provider could have undone. It holds what the boundary can see, so it
  says what it does not catch: a delete run from inside the provider, SQL whose `WHERE` matches
  every row, a function created earlier that drops tables.
- Leave it to you. You write the rules for each host yourself. It costs knowing that Railway
  deletes through `POST /graphql/v2`, RDS through `POST /` and Git through a line of zeros in a
  push.
- Leave it to the providers. Railway now keeps deleted volumes 48 hours, Stripe's agent keys ask
  for approval, and some tokens can be limited. It costs every provider that has none of this,
  and every token the agent finds that was made for something else.

Recommended: build it. The costliest incidents with agents in 2025 and 2026 are deletions through
an allowed API (PocketOS, DataTalks.Club, the OpenClaw inbox), and a checkpoint of the computer
brings none of them back.

## An agent's notes

- The set is ordinary rules, off until you switch it on (`rule-sets`, `security-framing`).
- Each rule of the set carries the hold's length for its host, below the time its usual client
  waits. Measured: `aws`, `supabase`, `neonctl` and `heroku` give up after 60 s; the Railway CLI
  after 90 s, and in the run it gave up before Silo's 90 s hold ended (`hold-limit-length`).
- Each rule carries the host's error form for a refusal. A plain 403 lost Silo's sentence on
  `gh`, `railway`, `vercel` and `git`; the provider's own form showed it on all eleven CLIs
  (`how-a-hold-ends`). `stripe` exits 0 on any refusal.
- A Git force push can be told from a fast-forward: walk the pack the push sends, then ask the
  host one question about the commit where the walk leaves it. A prototype classified 281 of 281
  ref updates right and added 0.1 to 5 ms to a push. A backup ref at the old commit survived
  `git gc`, and Silo has to refuse any push or API call that changes those refs, since `git push
  --mirror` deletes them. `research/2026-11-24-destructive-calls/details/git-guard.md`.
- It sits beside `merges-and-deploys-held`, which holds a merge and a deploy. Holding deploys on
  these hosts also closes the case of a service that deletes from inside the provider.
- Everything behind this feature: `research/2026-11-24-destructive-calls/`.
