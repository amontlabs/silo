# Revocations and "lasting access", rescored on #383's and #404's sets

Research of 25 November 2026, by agents, for polarzero. #405 proposed a narrower wording of
"exposed" for `outcome-of-a-call`: "it makes something readable by anyone, or gives lasting access
to someone who did not have it". It also said that 66 of the calls #383's rules hold wrongly are
revocations. Here that wording is applied, call by call, to #383's 1,085 documented writes (with
#405's settled labels) and to #404's 5,546, revocations are listed by hand in both, and both rule
sets are scored again on the surfaces and on the CLI captures. No rule of #383 or #404 was changed.

## What we found

1. **The narrower wording changes little: 13 calls of #383's set and 6 of #404's.** Of the 86
   calls of #383's set that give access to someone named, 73 give lasting access: a token with no
   expiry, an invitation, a role, a project transfer, logs or mail sent to an outside address. 9
   mint a credential for the caller's own run and 4 give nobody anything new (the caller logs in
   or accepts an invitation). On #404's set, 6 of the 222 calls that give access to someone named
   are for the run: a Kubernetes TokenRequest, a GitHub App installation token, a narrowed GitHub
   token, a just-in-time runner (two paths) and a JWT signed as a Google service account.

   | Set | Wording in force | #383's wide wording | Lasting access |
   |---|---|---|---|
   | #383's surface: should be held | 267 | 353 | 340 |
   | held by #383's rules | 240 (90%) | 299 (85%) | 298 (88%) |
   | should pass, held wrongly | 125 of 575 (22%) | 66 of 489 (13%) | 67 of 502 (13%) |
   | #404's surface: should be held | 1,272 | 1,494 | 1,488 |
   | held by #404's rules | 1,272 | 1,494 | 1,488 |
   | should pass, held wrongly | 241 of 2,449 (10%) | 19 of 2,227 (1%) | 25 of 2,233 (1%) |

   #404's rules were built from its own labels, which follow the wide wording, so they hold every
   call that gives access. Under the narrower wording they hold the 6 run tokens wrongly; under
   the wording in force they hold all 222 wrongly.

2. **On the CLI captures the narrower wording changes one command.** `kubectl create token default`
   is held under the wide wording and passes under the lasting one. The 4 commands of #383's
   captures that give access (`flyctl tokens create`, `flyctl orgs invite`, `vercel teams invite`,
   `vercel tokens add`) give lasting access, so they are held under both. `railway ssh` and a
   Vercel build's OIDC token, #405's cases, were not among the captured commands.

3. **The request rarely says how long a credential lives.** Of the 13 run-scoped or self calls on
   #383's set, 4 carry the lifetime in the body (`ttlSeconds`, `expiration_minutes`,
   `validHours`, an expiry). Of the 73 lasting ones, 5 accept an optional expiry. Among the
   captures, `flyctl tokens create` sends one (`"expiry": "175200h0m0s"`, 20 years) and
   `vercel tokens add` sends none. So a rule set tells "run" from "lasting" mostly by the operation.
   Where the body carries an expiry, as flyctl's does, a rule can read it: with `--expiry 1h` the
   same call mints a run token.

4. **Revocations are 57 of #383's wrong holds and 95 of #404's, once a revocation passes.** A
   revocation removes access someone had and gives nobody anything: a member or a role removed, a
   key or a token deleted, a webhook, drain or log sink deleted. Listed by hand: 64 operations on
   #383's set and 102 on #404's (`proto/access.py`). The labels disagree about them. #383 and #404
   label most of them "outage", so their tables count them as held correctly. #405's settling
   labels a member removed as "allow" and a token deleted as "outage".

   | Set | Revocations | Labelled outage | Labelled allow | Held by the rules |
   |---|---|---|---|---|
   | #383's surface | 64 | 23 | 41 | 57 |
   | #404's surface | 102 | 95 | 7 | 95 |

   When every revocation passes, the wrong holds under the lasting wording go from 67 to 90 on
   #383's set and from 25 to 120 on #404's. On #404's captures, `gh api -X DELETE
   orgs/acme/members/someone` and `gh repo deploy-key delete` become wrong holds.

5. **#405 overcounted the revocations.** Of the 67 calls #383's rules hold wrongly under the
   lasting wording, 34 are revocations by the list here, not 66. 32 are restarts, build and
   deploy cancels, cache purges, preview deletes and small configuration deletes. The last one is
   an R2 temporary credential, a run token.

6. **A rule set for the lasting wording needs one list more: operations that pass.** With the
   rules unchanged and an overlay that passes the operations classed run, self or revocation
   (`Overlay` in `proto/rescore.py`), the wrong holds fall to 32 on #383's set and to 19 on #404's
   (the GitHub GraphQL list #404 left for polarzero to rule on). No call that should be held is
   lost on either surface or on either set of captures. On #404's captures the overlay passes
   `kubectl create token`, the member removal and the deploy key deletion, and still misses the
   same 5 requests #404 reported.

   | Set | Rules | Should be held, held | Should pass, held wrongly |
   |---|---|---|---|
   | #383's surface | #383's | 275 of 317 | 90 of 525 |
   | | with the overlay | 275 of 317 | 32 of 525 |
   | #404's surface | #404's | 1,393 of 1,393 | 120 of 2,328 |
   | | with the overlay | 1,393 of 1,393 | 19 of 2,328 |
   | #404's captures | #404's | 167 of 172 | 3 of 213 |
   | | with the overlay | 167 of 172 | 0 of 213 |

   These rows use the lasting wording with every revocation passing. The overlay is built from the
   same lists it is scored with, so the surface rows show only that it encodes them. The captures
   are the test, and they hold 3 such commands.

## What it changes in the plan

Notes only, on `outcome-of-a-call`: the options for what "exposed" covers and for revocations,
with the numbers above. No answer, list or verdict changes.

## For you to rule

- **What "exposed" covers.** Case: an agent runs `kubectl create token default -n prod` to call
  the cluster for an hour, and later `flyctl tokens create deploy`, which mints a token that lives
  20 years. Options, with their cost on the two surfaces:
  - Readable by anyone, as in force. It passes all 73 + 216 lasting grants, `flyctl tokens create`
    among them.
  - #383's wide wording. It holds the 13 + 6 run or self calls too, `kubectl create token` among
    them.
  - Lasting access, #405's wording (recommended). It holds the lasting grants and passes the run
    tokens. It costs a list of run-scoped operations in each rule set, since the request rarely
    says the lifetime.
- **Whether a revocation passes.** Case: at night an agent deletes an IAM access key it found in a
  log, and removes a member from the GitHub organization. Options:
  - A revocation passes (recommended). It is an outage at most, which alerts and passes under the
    answer in force. It removes 57 asks on #383's set and 95 on #404's. It also passes an agent
    that removes polarzero from the organization.
  - A revocation is held, as both prototypes do today, since they hold every outage.
  - A revocation passes, except one that removes the user's own access. It needs a fact Silo does
    not have yet: which account on the provider is the user's. Silo could learn it from the
    credential it holds (`GET /user` on GitHub, `GetCallerIdentity` on AWS).

## What was not verified

- One agent classed every call, from the operation's name, path and summary, the reason #404 or
  #405 gave, and what it knew of each provider. No second agent checked the classes, and no
  provider was called.
- The lifetimes in `proto/access.py` come from that knowledge of the providers' documentation, not
  from reading it again today. Three are marked as not documented: Cloudflare's tunnel management
  token, Railway's clearance token and Vercel Connect's token. They are classed lasting.
- On #404's set only the calls #404 labelled exposed were classed. A call #404 labelled "allow"
  that the wide wording would hold was not looked for. Two seen in passing: Google's
  `serviceAccounts.keys.enable` (allow) while AWS `UpdateAccessKey` to Active is exposed.
- The overlay matches REST paths and GraphQL mutation names. AWS requests on the captures were not
  matched by it; none of them is a revocation or a run token.
- Outages are counted as held, as #383, #404 and #405 count them, though the answer in force says
  an outage alerts and passes.

## What is here

| Path | What |
|---|---|
| `proto/access.py` | The class of each call that gives access (anyone, protection, lasting, run, self), with a reason and where the request carries the lifetime; the revocations of both sets |
| `proto/rescore.py` | Builds the labels under each wording, with revocations as labelled or passing, scores #404's tuned rules and the overlay on both surfaces and both sets of captures |
| `proto/results/tables.md` | Every combination of wording, revocations and rules, per set |
| `proto/results/rescore.json` | The counts, and each call held wrongly or missed in each combination |

Run from `proto/`: `python3 -B rescore.py`. It reads #383's, #404's and #405's folders on this
branch and #353's detector, which needs `botocore`, `sqlglot`, `brotli`, `zstandard` and
`graphql-core`.
