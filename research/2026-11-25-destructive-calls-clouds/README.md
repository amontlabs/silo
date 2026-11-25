# Destructive calls on AWS, Google Cloud, Azure, GitHub and Kubernetes

Research of 25 November 2026, by agents, for polarzero. It extends the destructive-call detector of
#353 (`research/2026-11-24-destructive-calls/`), as #383 did for five platforms
(`research/2026-11-25-destructive-calls-platforms/`), to the five APIs agents reach most through
their own CLIs, and studies a generic fallback for writes no rule lists. Everything was scored
blind first: the fallback, the rules and the command labels were each committed before anything
was run against them.

## What we found

1. **#353's rules held 22% of the documented destructive calls of these five APIs.** Every
   documented write was labelled by hand: 5,546 requests, from botocore's models for 20 AWS
   services, gcloud's generated clients for 16 Google APIs, the Azure CLI's ARM operations for 14
   providers, GitHub's REST description and GraphQL schema, and the Kubernetes v1.37.2 OpenAPI
   document. 1,494 should be held, 2,227 should pass, and 1,825 depend on context (a deploy, an
   overwrite, a pod a controller recreates).

   | API | Should be held | #353's rules | Should pass, held by #353 |
   |---|---|---|---|
   | AWS | 420 | 266 | 149 |
   | Google Cloud | 401 | 0 | 0 |
   | Azure | 318 | 0 | 0 |
   | GitHub REST | 163 | 38 | 35 |
   | GitHub GraphQL | 42 | 26 | 19 |
   | Kubernetes | 150 | 0 | 0 |

   The new rules hold all 1,494 by construction (they are built from the labels), with 19 wrongly
   held: #353's GitHub GraphQL list, kept as it is (see "For you to rule"). Nothing changes on
   #383's and #353's own sets: the new rule set scores exactly as #383's there.

2. **On what the CLIs send, the blind rules held 165 of 177 destructive requests and nothing that
   should pass.** 218 commands of `aws`, `gcloud` and `bq`, `az`, `gh` and `kubectl` were labelled,
   then run against a mock of each API; 216 reached their step and sent 413 requests.

   | Rule set | Held | Should pass, held |
   |---|---|---|
   | #353's rules | 61 of 175 | 2 of 210 |
   | Fallback alone | 134 of 175 | 0 of 210 |
   | Blind rules | 165 of 175 | 0 of 210 |
   | Blind rules, fallback behind | 168 of 175 | 0 of 210 |
   | Tuned rules, fallback behind | 170 of 175 | 0 of 210 |

   The first blind score was 165 of 177: two requests counted as a step were gcloud polling an
   operation (`POST .../operations/<id>/wait`). `capture/labels.py` now counts those as reads,
   which leaves 175. Per command: [`details/captures.md`](details/captures.md).

3. **Only 7 of 226 writes the CLIs sent were outside the documented surface read here, against 14
   of 100 for #383's platforms.** All seven are older versions or providers the extraction did not
   read: gcloud still calls Resource Manager v1 and Cloud Functions v1, `az webapp stop` is in
   `Microsoft.Web`, and the Key Vault and Cloud Storage upload calls are data planes. These APIs
   publish machine-readable descriptions, and their CLIs are generated from them, so a rule set
   built from the descriptions covers almost everything the CLIs send.

4. **Bodies a rule cannot read were counted as "field absent", and let through.** Two real cases:
   - `az deployment group create --mode Complete` sends `{"properties": {"mode": "Complete",
     template:{...}}}`: the key `template` is not quoted, so the body is not JSON. Complete mode
     deletes every resource of the group the template does not list. The blind rules let it pass.
   - `kubectl` 1.37 sends creates, evictions and token requests as protobuf
     (`application/vnd.kubernetes.protobuf`), not JSON. A rule on a create's body (a Service of
     type LoadBalancer) cannot read it.
   `operation-of-a-request` already says a body Silo cannot parse is held. #353's engine did not
   do that for body conditions. The tuned rules do: a condition on a body that is neither JSON nor
   a form is unknown, so the request is held. It costs one deploy (Incremental mode, same invalid
   body) held as unknown.

5. **The fallback is right where rules are missing, but too loose to stand alone.** It reads the
   method, the verb at the end of the path or in the operation name, and a few body markers
   (`fallback/fallback.py`, frozen before any score).

   | Set | Destructive held | Should pass, held |
   |---|---|---|
   | This surface | 1,026 of 1,494 (69%) | 348 of 2,227 (16%) |
   | #383's surface | 230 of 341 (67%) | 34 of 384 |
   | #383's captures | 51 of 66 | 0 of 125 |
   | #353's corpus | 140 of 226 (62%) | 6 of 146 |
   | These captures | 134 of 175 (77%) | 0 of 210 |

   Behind the rules it decided the 7 writes no rule listed: it held 3 of the 4 that should be held
   (a project deleted, a function deleted, a web app stopped) and passed the 3 that should pass.
   It missed `gcloud projects add-iam-policy-binding --role roles/owner`. Holding every unlisted
   operation, as `operation-of-a-request` says, would have held all 4 and the 3 others too. Its
   cost on the whole surface is 16% of harmless writes held, so the rule in force is the better
   default. The fallback's guess is still useful in the ask, to say what the call probably does.

6. **Five misses remain, and none is a missing rule.**
   - `kubectl delete pods --all` sends one DELETE per pod; one pod deleted is "either" (a
     controller recreates it). Only the number of requests says "all pods". That is
     `asks-for-a-burst`, not a rule.
   - `gcloud projects add-iam-policy-binding --member user:someone --role roles/owner`: gcloud
     sends the whole policy, existing owners included, so the new grant cannot be told from the
     body alone (see "For you to rule").
   - Three are labels landing on a request that is not the step: `az deployment` validates first
     (a POST that changes nothing), `gh repo sync --force` merged upstream without forcing, and
     `gcloud storage cp` deleted its own upload after the mock's answer failed its hash check.

## What it changes in the plan

Nothing under `plan/` changes. The work supports `destructive-calls-held` (feature) with the
measurements above, and these would be notes:

- `operation-of-a-request`: two real bodies the boundary cannot parse (az's deployment body,
  kubectl's protobuf), and a measure of unlisted operations on generated APIs (7 of 226 writes).
  For the hold of an unlisted operation to work, a set must list the reads too: the sets here list
  writes only, apart from AWS.
- `asks-for-a-burst`: `kubectl delete --all`, `aws s3 rm --recursive` and `gcloud storage rm -r`
  fan out on the client, one request per object.

## For you to rule

- **#353's GitHub GraphQL list or the new labels.** #353 holds every mutation whose name says
  delete or remove (`github.gql-list`). Every mutation now has its own label, and 19 of the ones
  #353 holds are small: `removeStar`, `deleteLabel`, `removeReaction`, `deleteProjectV2Item`,
  `disablePullRequestAutoMerge`. Recommended: replace the name list with the labels. The case:
  an agent tidying a project board is held at each `deleteProjectV2Item`. Kept as #353 had it
  until you rule.
- **Granting a role to someone named, through setIamPolicy.** Google sends the whole policy on
  every change, so the body cannot show which binding is new. Options: hold every policy that
  holds `roles/owner` or `roles/editor` (every change on most projects); have Silo read the
  current policy first (one GET, and the computer's credential must allow it); or hold only
  `allUsers` and `allAuthenticatedUsers` (what the rules do now). Recommended: read the current
  policy when the computer's credential can, else hold on owner and editor.

## What was not verified

- No call went to a real provider. Every answer came from the mock. A CLI that would act
  differently on a real answer is not seen. While the rig was being built, a few aws commands
  reached the real RDS and SQS endpoints with made-up keys and were refused
  there; the rig now sends every host it does not map to a dead proxy.
- `kubectl apply --server-side` did not reach its step (kubectl gave up on the mock's OpenAPI
  document), nor did `gh project delete` (the mock cannot say whether `acme` is a user or an
  organization).
- The surfaces cover the services listed above, not each provider's whole API. AWS reads are
  listed; for the four others only writes are.
- The labels are one agent's reading of each operation's documentation, not checked by a second
  agent. `proto/surface/labels/` gives the reason for each label that is not obvious.
- The rules are built from the same labels the surface is scored with, so the surface score shows
  only that the rules encode the labels. The captures are the test.
- Whether Azure Resource Manager accepts az's unquoted `template` key was not checked.

## What is here

| Path | What |
|---|---|
| `details/captures.md` | Each of the 218 commands, the writes it sent and what each rule set decided |
| `proto/fallback/fallback.py` | The generic fallback, frozen before any score |
| `proto/surface/` | `extract.py` (the operations, `ops/`), the labellers and their labels (`labels/`), `variants.py` (bodies for operations whose body decides), `surface.py` (requests) |
| `proto/detector/` | `rules_build.py` builds `rules-blind.json` (frozen before the captures), `rules.json` (with `TUNED`) and `aws_labelled.json`; `engine.py` adds to #383's engine |
| `proto/capture/` | The rig (`run.sh`, `prepare.sh`, `server3.py`, `capture.py`), `commands.tsv`, the captures and `labels.py` |
| `proto/measure.py`, `proto/score.py` | Score the rule sets and the fallback; write `results/` |
| `proto/tests/` | The tests of the tuned rules; `RULES=blind` shows them failing on the blind rules |

Run from `proto/`: `python3 -B measure.py` and `python3 -B -m pytest -q tests/`. They need #353's
and #383's folders beside this one.
