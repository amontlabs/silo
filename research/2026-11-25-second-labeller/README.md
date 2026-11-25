# A second, blind labelling of #383's 1,085 writes

Research of 25 November 2026, by agents, for polarzero. #383 labelled every documented write of
Railway, Fly.io, Vercel, Supabase and Cloudflare by hand (1,085 requests) and wrote rules that hold
all 341 it called destructive and none of the 384 it called harmless. It said itself that this is
by construction, and that the labels were one agent's reading. Here fresh agents labelled the same
1,085 requests without seeing #383's labels, every disagreement was settled, and #383's rules were
scored again on the settled labels. No rule was changed.

## What we found

1. **The two readings agree on whether to hold a call 84% of the time (κ 0.60), and on the
   exact label 66% (κ 0.54).** 175 of the 1,085 requests are held under one reading and not the
   other. 89 of the 175 are one question of wording, below; the other 86 are 8% of the surface.

2. **Half of what #383 held as "exposed" is not exposed under the wording in force.** The second
   labelling used `outcome-of-a-call` as it stands ("readable by anyone"). Of #383's 102 exposed
   requests it called 11 exposed; it marked 74 of them as giving access to someone named (a token,
   an invitation, a role, a project transfer, a log drain). #383 labelled under the wider wording
   it proposes but which the owner has not ruled on. So #383's tables measure that wording, not the
   one in force.

3. **On the settled labels #383's rules hold 90% of what should be held and wrongly hold 22% of
   the rest.** Not 100% and 0%.

   | Label set | Should be held | Held by #383's rules | Should pass | Wrongly held |
   |---|---|---|---|---|
   | #383's labels | 341 | 341 (100%) | 384 | 0 |
   | Blind labels | 264 | 236 (89%) | 580 | 130 (22%) |
   | Settled, wording in force | 267 | 240 (90%) | 575 | 125 (22%) |
   | Settled, #383's proposed wording | 353 | 299 (85%) | 489 | 66 (13%) |

   #353's rules on the settled labels with the wording in force: 182 of 267 held, 95 of 575
   wrongly held. The blind and the tuned rules of #383 decide every one of these 1,085 requests
   alike.

4. **The rules hold the safe direction: 66 of the wrong holds are revocations.** They hold
   nearly every `DELETE`, so they hold removing a team member, deleting a webhook or a log drain,
   removing a firewall bypass, deleting an SSH key, leaving a project, and restarts and cache
   purges. #383 had labelled 47 of these "either" (context decides), which its tables do not
   score, and 17 "outage". Removing someone's access is what an owner wants an agent to be able to
   do at night; an ask for it is noise.

5. **The 54 the rules miss (proposed wording) are money, public addresses and tokens.**
   - 17 cost money and pass: a Fly Postgres cluster, fork or restore, a volume extended (it
     cannot shrink), a Fly add-on and a static egress IP, a Supabase disk grown and a read
     replica, Vercel Static IPs, Secure Compute, PrivateLink and Observability Plus, and the five
     marketplace billing calls that charge or refund an integration's customers.
   - 5 make something readable by anyone and pass: Railway's `serviceDomainCreate`,
     `customDomainCreate` and `tcpProxyCreate` (a database port on the internet), `templatePublish`,
     and a Cloudflare shareable link to Workers logs. These are exposed under the wording in force,
     not only the proposed one.
   - 27 give access to someone named: mostly tokens the caller mints for its own run (Railway's
     `generateShellToken` and `cloudAgentHarnessToken`, Vercel's project OIDC token, Supabase's
     CLI login role), plus Slack and webhook targets and an outside mail forward.
   - 5 stop something serving: vanity name servers removed, an r2.dev URL turned off, a private
     DNS name renamed, a cloud agent put to sleep, a Vectorize metadata index deleted.

6. **#383 missed two undo endpoints in the very APIs it labelled.** Cloudflare's D1 has
   `POST .../d1/database/{id}/undelete` and Supabase has `POST /v1/branches/{id}/restore`, so a
   deleted D1 database and a deleted Supabase branch are recoverable, not gone; Edge Config keeps
   restorable backups of its items. The ask says what the provider can bring back
   (`destructive-calls-held`), so these change its sentence, not whether it holds.

7. **44 operations were held at their worst case though their body decides.** Role and
   token updates, DNS record and route updates, Supabase network restrictions and SSL enforcement, Vercel's firewall `PUT`, a
   DNS zone replaced, R2 lifecycle rules, Railway's usage limits. #383 gave variants with bodies to
   20 such operations; these 44 have none, so their label is a guess either way. Each needs a
   harmless and a harmful body, as #383's `variants.json` does for the others.

8. **Two kinds of call fit no outcome.** Railway's `twoFactorInfoDelete`, `passkeyDelete` and
   `emailChange*` weaken or move the account itself. The blind labeller found no label and passed
   them; they are settled as exposed under "a protection turned off" (the notes of
   `outcome-of-a-call`). A call that kills running work (Cloudflare Workflow instances terminated,
   Railway's cloud agent put to sleep) loses state that is not data; it is settled as allow and
   outage, by whether something stops serving.

## What it changes in the plan

Two notes, no list or verdict changed.

- `outcome-of-a-call`: the proposed wording of "exposed", applied as written, would hold the
  tokens agents mint for their own runs; a narrower wording is proposed below. Paid resources
  created are the outcome the rules miss most.
- `destructive-calls-held`: a revocation passes; the measured recall and wrong holds on a second
  reading; the undo endpoints #383 missed.

## For you to rule

- **#383's proposed wording of "exposed" holds an agent's own short-lived tokens.** Case: an
  agent runs `railway ssh`, which mints a shell token through `generateShellToken`, or a Vercel
  build asks for its project's OIDC token. Under "gives access or data to someone who did not
  have it", each is held and asks you at night, 86 operations more than today, most of them
  routine. Proposed instead: "Exposed: it makes something readable by anyone, or gives lasting
  access to someone who did not have it: a new long-lived token or key, an invitation or a role,
  the project moved to another account, data sent to an outside address." A token that expires
  within the run passes. This replaces #383's proposal; the wording in force stays until you rule.

## What was not verified

- **The settling is not independent.** The agent that settled the disagreements also wrote the
  second labellers' guide (`proto/GUIDE.md`), and it sided with the blind label in 348 of 368
  cases (18 overrides, 2 for #383 by kind). A third blind labelling of the 175 held-or-not
  disagreements would test it.
- The labellers read only the operation's name, path, arguments and the provider's one-line
  summary, as #383's labeller did, plus what they knew of each provider. No provider was called.
- 120 of the 368 disagreements are #383's "either" against "allow" on calls that matter little
  (most are small deletes). They change no score of #383's, since "either" is not scored, but
  they are the wrong holds of finding 4 once settled.
- No rule was changed or tuned on these labels, so the numbers above are #383's rules as merged.

## What is here

| Path | What |
|---|---|
| `details/disagreements.md` | The 368 disagreements: both labels with their reasons, the settled label and why |
| `proto/GUIDE.md` | The guide the blind labellers had, and nothing else |
| `proto/inputs/` | What they labelled: #383's operations and variant bodies with labels and reasons removed, and their SHA-256 |
| `proto/labels-b/` | The blind labels, with a reason and a `named` flag (access given to someone named) |
| `proto/settle.py` | Settles each disagreement: a rule per kind, and named overrides with reasons |
| `proto/settled.jsonl`, `proto/settled-proposed.jsonl` | The settled labels under the wording in force and under #383's proposed wording |
| `proto/compare.py` | Agreement, κ, and #353's and #383's rules scored on every label set; writes `proto/results/` |

Run from `proto/`: `python3 settle.py`, then `python3 compare.py`. They read #383's surface and
rules (`../../2026-11-25-destructive-calls-platforms/proto/`, on this branch) and #353's detector,
which needs `brotli`, `zstandard` and `graphql-core`.
