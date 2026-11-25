# Destructive calls on Railway, Fly.io, Vercel, Supabase and Cloudflare

Research of 25 November 2026, by agents, for polarzero. It extends the destructive-call detector
of #353 (`research/2026-11-24-destructive-calls/`) from the calls #353 picked by hand to every
documented write of five platforms, then measures it on what those platforms' own CLIs send.

## What we found

1. **#353's rules held half of the documented destructive calls of these five APIs.** Every write
   operation in each provider's API was labelled by hand: 1,085 requests, from Railway's and
   Fly's GraphQL schemas (introspected on 25 November), the OpenAPI specs of Vercel, Supabase and
   Cloudflare (Cloudflare limited to Workers, R2, D1, KV, Queues, DNS, zones, members and tokens),
   and Fly's Machines API. 341 should be held, 384 should pass, and 360 depend on context (a
   deploy, or a write that overwrites). #353's rules held 176 of the 341 and 31 of the 384.

   | Outcome | Should be held | #353's rules | New rules |
   |---|---|---|---|
   | gone | 103 | 82 | 103 |
   | outage | 120 | 80 | 120 |
   | exposed | 102 | 8 | 102 |
   | money | 13 | 3 | 13 |
   | recoverable | 3 | 3 | 3 |

   The 31 wrongly held are Railway mutations whose names end in `Delete` or `Remove` but undo
   small things, such as `projectScheduleDeleteCancel` and `featureFlagRemove`. The new rules
   (194 of them) hold all 341 and none of the 384. That is by construction, since they were
   written from these labels. The capture below is the measurement that was not.

2. **On 121 real CLI commands, the new rules held 62 of 66 destructive calls and none of 125
   other requests, blind.** The rules were committed (b026dd5) and the commands labelled
   (ff0ce02) before any CLI ran. #353's rules held 42 of the 66 on the same captures. Three
   commands did not reach their step and are not counted: `flyctl apps move` and `flyctl mpg
   destroy` need what a mock cannot give, and `vercel certs rm` has no flag to skip its prompt.

   | CLI | Should be held | #353's rules | New rules, blind | Tuned after |
   |---|---|---|---|---|
   | flyctl | 13 | 6 | 13 | 13 |
   | Railway | 14 | 11 | 12 | 14 |
   | Supabase | 10 | 6 | 9 | 10 |
   | Vercel | 11 | 6 | 10 | 11 |
   | Wrangler | 18 | 13 | 18 | 18 |
   | Total | 66 | 42 | 62 | 66 |

   Requests that should pass: 125, none held by any of the three. #353's corpus of 372 requests
   gives the same result under all three rule sets: 220 of 226 held, 1 of 146 wrongly held.

3. **The four blind misses are wire forms the documentation does not show.**
   - `railway volume detach` sends `volumeInstanceUpdate(input: { serviceId: null })`. The same
     mutation changes a mount path, so the surface labelled it "either".
   - `railway scale us-west2=0` removes the region from the patch:
     `{"multiRegionConfig": {"us-west2": null}}`. The rules held only patches that set
     `isDeleted`.
   - `supabase storage rm` deletes through the project's own host,
     `DELETE https://<ref>.supabase.co/storage/v1/object/avatars` with `{"prefixes": [...]}`. The
     Storage API is not in the Management API spec.
   - `vercel teams invite` sends `POST /teams/team_1/members` with no version. The spec says
     `/v2/teams/{teamId}/members`.

   Three rules and one change to every Vercel path fix them (`TUNED` in
   `proto/detector/rules_build.py`). `proto/tests/test_tuned.py` shows each miss under the blind
   rules and its neighbours that must still pass: a mount path changed, a volume attached, a
   service scaled up, a storage read, a member list.

4. **14 of the 100 writes the CLIs sent are not in the documented API**
   (`proto/capture/spec_coverage.py`, Railway's telemetry left out). Most are Vercel paths: the
   CLI deletes an environment variable at `/v10/...` where the spec has `/v9/...`, and removes an
   alias at `/now/aliases/...` and a project at `/projects/...` with no version. 10 of the 14
   should be held. The new rules match any version (`/v*`) and held 8 of the 10 blind; a set
   written with the spec's exact paths would have treated all 14 as unknown operations. This is
   the evidence for the note added to `operation-of-a-request`.

5. **Exposure is a third of what should be held, and about half of it is not "readable by
   anyone".** 102 of
   the 341: tokens created (`flyctl tokens create`, `vercel tokens add`), members invited, a
   Vercel firewall bypass, a log drain or webhook sending data out, a Supabase database opened to
   `0.0.0.0/0`, SSL enforcement off, an R2 bucket given a public URL. 41 of the 93 operations
   behind them give access to someone named: a token, an invitation, a member or role, a drain or
   webhook sending data out. #353's rules held 8 of the 102. The word "exposed" is already in
   `outcome-of-a-call`, defined as "readable by anyone".

6. **Two CLIs already know they are driven by agents.** The Vercel CLI 66.6.0 deprecates `--yes`
   on destructive commands: "--yes is deprecated for this command. Use --confirm-name my-app."
   It asks for the target's name, or `--amount` for a price, to approve. The Railway CLI reads
   the environment variables of coding agents (`CLAUDECODE`, `CURSOR_AGENT`, `CODEX_SANDBOX` and
   others) and reports the agent in its telemetry. Neither stops a destructive call: `--yes`
   still worked.

## What it changes in the plan

Nothing under `plan/` changes except two notes. The work supports `destructive-calls-held`
(feature) with the measurements above.

- `operation-of-a-request`: a note that 14 of 100 writes the CLIs sent are outside the documented
  API, and that a set should match a path's version segment as any version.
- `outcome-of-a-call`: a note that about half of exposure here is access given to someone named, with a
  proposed wording (below).

## For you to rule

- **Widen "exposed" to access given to someone.** `outcome-of-a-call` (answered) says exposed is
  "it makes something readable by anyone". The case: an agent runs `flyctl tokens create deploy`
  and pastes the token into a log. Nothing became readable by anyone at the moment of the call,
  so under that wording it is not exposed, and no other outcome fits; it passes. The same holds
  for `vercel teams invite`, a firewall bypass and a log drain sending data out, which are
  about half of the exposed calls measured here. Proposed: "Exposed: it gives access or data to someone who did
  not have it, such as a new token, an invitation, a public bucket or an opened network."
- **An outage passes, but a third of these holds are outages.** `outcome-of-a-call` says an
  outage raises an alert and passes. The prototype holds outages, as #353's did, so the tables
  above count them. Under the answer in force, 120 of the 341 would only alert, among them
  `flyctl scale count 0`, `railway down`, a Vercel domain removed and a Cloudflare Workflow
  terminated. This is not a challenge yet: whether an alert is enough for a production domain is
  a question for the default rule set, not for the outcome words.

## What was not verified

- No call went to a real provider. Every answer came from the mock, built from the provider's
  schema. A CLI that would act differently on a real answer (a confirmation that depends on the
  resource, a second call after the first succeeds) is not seen.
- `flyctl apps move`, `flyctl mpg destroy` and `vercel certs rm` did not reach their step.
- Cloudflare's surface covers 301 operations of the products listed above, not its whole API.
- Fly's Machines API spec was not reachable (the URL redirects to a 404); its 55 paths come from
  the `fly-go` client, v0.12.8.
- The labels are one agent's reading of each operation's documentation, not checked by a second
  agent. `proto/surface/labels/` gives the reason for each label that is not obvious from its name.
- The tuned rules are fitted to the four misses. Only their tests and the 125 reads check that
  they do not hold more.

## What is here

| Path | What |
|---|---|
| `details/captures.md` | Each of the 121 commands, the write it sent and what each rule set decided |
| `proto/surface/` | The extracted operations (`ops/`), their labels (`labels/`), the bodies for context-dependent ones (`variants.json`), and `surface.py`, which turns them into requests |
| `proto/detector/` | `rules_build.py` builds `rules-blind.json` (frozen before the captures) and `rules.json` (with the tuned rules); `engine.py` adds two body conditions to #353's engine |
| `proto/capture/` | The capture rig (`run.sh`, `server2.py`, `capture.py`), the commands, the captures, each command's output, and `labels.py` |
| `proto/measure.py` | Scores #353's, the blind and the tuned rules on the surface, the captures and #353's corpus; writes `results/` |
| `proto/tests/` | The tests for the tuned rules |

Run from `proto/`: `python3 measure.py` and `python3 -m unittest discover -s tests`. They need
#353's detector, which is on the integration branch (#375).
