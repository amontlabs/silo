# Fixes for the gaps in the plan

Research of 24 November 2026, by agents, for polarzero. Draft PR #350 found ten real pains the plan
misses (G1 to G10) and challenged three decisions (C1 to C3). PR #347 found a flaw in
`secret-placeholder`. This folder designs a fix for each one. Every fix has a mechanism, the plan
entries it touches, prior art, bad paths, and a prototype run on Linux against real programs:
Claude Code 2.1.331, Codex 0.172.0, `git`, `gh`, `npm`, boto3, Postgres 16 and Redis. These are
recommendations only. Nothing under `plan/` is changed. Entries are named by id, and `plan show <id>`
prints one.

C2 (dollars or the window first, on screen) and C3 (the writing guide) are about how things are
shown. Another thread covers that, so they appear here only where they change what Silo measures.

## The pages

| Page | Answers | Prototype |
|---|---|---|
| [`1-credential-slots.md`](1-credential-slots.md) | #347: where a placeholder is swapped | [`credential-slots/`](prototypes/credential-slots/): a forward proxy that ends TLS, AWS re-signing, 61 checks |
| [`2-secret-resource-scopes.md`](2-secret-resource-scopes.md) | G2: a GitHub token used to publish loot | [`resource-scopes/`](prototypes/resource-scopes/): a classifier for every GitHub REST operation and GraphQL mutation, plus traffic from real clients |
| [`3-database-secrets.md`](3-database-secrets.md) | G6: `DATABASE_URL` | [`database-proxy/`](prototypes/database-proxy/): a Postgres boundary with SCRAM on both legs and statement holds, plus Redis |
| [`4-files-that-run-on-the-mac.md`](4-files-that-run-on-the-mac.md) | G3: hooks and tasks that run on the Mac. G4: package age beyond npm and PyPI | [`autorun-paths/`](prototypes/autorun-paths/): reads a push and holds it on a worm commit; tested on the history of 8 repositories |
| [`5-secrets-silo-never-held.md`](5-secrets-silo-never-held.md) | G5: secrets pasted into a chat or already committed | [`leaked-secrets/`](prototypes/leaked-secrets/): a detector with GitHub's checksum, run on 750 MB of real text |
| [`6-window-budgets-and-traffic-alerts.md`](6-window-budgets-and-traffic-alerts.md) | G7: budgets in the 5-hour and weekly windows. G8: alerts from model traffic | [`window-budgets/`](prototypes/window-budgets/): a window simulator with four estimators, and detectors |
| [`7-receipt-facts.md`](7-receipt-facts.md) | G10: false "tests pass" claims. G9: what a restore cannot bring back | [`receipt-facts/`](prototypes/receipt-facts/): a bpftrace collector on the real Claude Code, five scenarios |
| [`8-holds-and-new-hosts.md`](8-holds-and-new-hosts.md) | C1: the 90-second hold. G1: asking for new hosts | [`holds/`](prototypes/holds/): hold servers measured against Claude Code and Codex, and host counts from real installs |

Each page opens with a summary table that gives each recommendation and how sure it is. Each
prototype has its run command on its page.

## What each page recommends

**#347, credential slots.** A secret is swapped only in its slots on its allowed hosts. A slot is a
named header, a scheme inside `Authorization`, the password of Basic auth, a query parameter, a
cookie or a path segment. A placeholder seen anywhere else is forwarded as it is and recorded as a
fact, and nothing is denied for it. A placeholder has the real value's length and prefix, so a swap
never changes a length. Replies are scrubbed of the real value, including its base64 forms and runs
of 12 or more of its characters. AWS keys work too, because the boundary re-signs the request. With
only placeholders in the computer, a real `git clone` and `git push`, Claude Code, `gh`, boto3 and
the JavaScript AWS SDK all worked. The hole that remains is minting: a swapped secret can create a
new real credential (`sts:GetSessionToken`, a deploy key, `npm token create`). The proposal is a
per-host list of minting endpoints, each held with an ask.

**G2, places on a host.** A secret can be limited to named repositories, packages or buckets. The
boundary reads each request's effect (read, change, create, expose, runs, destroy, unknown) from its
method, path and parsed GraphQL operation. It uses data built from GitHub's own OpenAPI file and
GraphQL schema. Creating, exposing, running and unknown effects are held; anything outside the named
places is denied. `gh repo create` sends a GraphQL mutation, so a check of paths alone misses it.
The finding that matters most: limiting your secret is not enough, because a worm can bring its own
token. The page proposes that a host which gets a secret accepts no other credential.

**G6, database passwords.** The boundary can swap them. It ends the Postgres connection, checks
the placeholder in the client's SCRAM exchange, and logs in to the real server with the real
password. psql, node-postgres, psycopg and Prisma all worked. A parse of the statements
cannot be the guarantee: `DELETE ... WHERE total > $1 OR true` got through. A DO block also turned
off a read-only session before a SECURITY DEFINER function deleted rows. So the guarantee comes from
Postgres itself, through a read-only credential that the secret holds next to the write one. Holding
`DROP`, `TRUNCATE`, `DELETE` without `WHERE` and DO blocks is a rule triggered by a fact, and it
catches every reset in the evidence. Prisma's reset sends `DROP SCHEMA "public" CASCADE`. A fork
should get a database branch, not the parent's write credential. PocketOS was a GraphQL API token,
not a database password, so G2's limits cover it.

**G3, files that run on the Mac.** Hold a push that changes a file some tool on the Mac runs by
itself, and list every such change in the receipt. The boundary reads the push it already decrypts:
the ref updates, then the pack indexed against a mirror kept outside the computer. Which paths a
tool runs is shipped data, so the hold is a rule. The hold reads which keys changed, not only which
files. On the last 1,000 commits of 8 popular repositories it would have stopped 0 to 1.8% of
commits; a check on paths alone would have stopped up to 29%. Measured on Claude Code 2.1.331: a
folder trusted once runs a hook changed by a later `git pull` without asking again. `claude -p` ran
the hooks of a folder it had never seen.

**G4, package age.** The rule can cover the VS Code Marketplace, Open VSX, crates.io, RubyGems,
Docker Hub and Quay, which state publish times themselves. It cannot cover GHCR and other OCI
registries, Go modules, Homebrew or skills from a Git URL. New proposal: a version's age is the
older of what its registry states and when Silo first saw that exact digest or commit. This is a
lower bound, so it extends the rule to images pulled by digest and to Git commits. `npm install`
fetches metadata without publish times, so for npm the boundary must ask the registry separately.

**G5, secrets Silo never held.** Recognise credentials by exact formats only: GitHub's built-in
checksum, keys that parse, fixed documented prefixes. Entropy only marks a line. On 606 MB of
`node_modules` and 140 MB of old Git history, the exact rules matched only vendor examples and test
values. Entropy raised 58 wrong matches. Scan only the messages that are new in each model request:
2.7 ms for a whole session. Hold the first request that carries a match, with three answers: send
as is, make it a Silo secret, or don't send. Never keep the value in the record. Claude Code itself
redacts nothing: its transcript held 3 of 3 planted values.

**G7, window budgets.** Two budgets. A **reserve** is a rule: "stop this computer's calls once the
week is 70% used". It reads the provider's figure on each reply, and in the simulator it stopped at
70.0 to 70.3% of the true week. A **share** ("this computer may use 30% of the week") can only be a
precaution. Neither provider reports a share per call, so it has to be estimated. The obvious
estimator is off by a factor of 2 to 4 as soon as your laptop also spends. The best one stays
within about a point when the figure is precise and drifts to 24 to 45% when it comes as a whole
percent.

**G8, traffic alerts.** Alerts that ship switched on, each only an alert, in the pattern of
`new-host-alert-is-a-rule`:

- another model answered than the one asked;
- the program changed model in a conversation;
- the cache lifetime asked changed;
- a live cache was not read;
- more than half of an hour's input was uncached;
- the week is running ahead of pace.

All but one are facts. Measured: Claude Code asks a 1-hour cache lifetime with a subscription and
5 minutes with an API key. It drops to 5 minutes the moment calls draw on extra usage, and says
nothing when another model answers.

**G10, false "tests pass" claims.** Each test run in the receipt gets four items:

- the runner's own exit code, not the pipeline's;
- stale: a file in the checkout changed after the pass;
- narrowed: a selector in the argv;
- changes that weaken checks, listed first.

The agent's claim is then marked backed, stale, narrowed, no run or contradicted, as an opinion. A
bpftrace collector on the real Claude Code caught every false claim in five scenarios and raised
no false alarm on the honest one. Without the probe, from traffic and checkpoints alone, 4 of 5
verdicts were the same. Pipes are joined by pipe inodes, not process groups. Also measured: a
passing pytest piped into `head` exits 1.

**G9, what a restore cannot undo.** Before a restore, list everything that left the computer since
the checkpoint and could have changed something elsewhere: requests that do not only read, pushes
with their refs, database writes, copies to the Mac, and connections Silo did not read. The list
comes with three plain sentences on what Silo cannot know.

**C1, the 90-second hold.** Its premise is measured false. Claude Code 2.1.331 does not end a
command at 2 minutes. It moves it to the background and stops it at 12 minutes. Codex kept a held
push alive for 20 minutes. A yes at 4 minutes let the original push through with no retry. The page
recommends three changes:

- a 10-minute hold, measured again for each version of each program;
- an ask that outlives its hold, so a retry joins the open ask, and a late yes lets that exact
  request pass once;
- "the same request" defined exactly. #350's coarse matcher let a 9,900.00 charge through on a yes
  to 40.00.

A held push should answer in Git's own `remote:` lines. A 403 makes git print a false "Everything
up-to-date", and a closed connection makes it send the push twice.

**G1, asking for new hosts.** Ship "limited, ask for new hosts" as a rule set. It is a list, a rule
that allows the list, and a rule that holds any host not on it; a yes adds the host. Installs are
cheap: a big `npm install` reached 9 hosts (not the 200 that a note of `first-seen-host`
assumes) and `pip install` reached 7. With a shipped list of download hosts, they raised 0 and 1
asks. The asks pile up when the agent reads documentation and follows links: 35 for one 20-page
session. So the page also proposes that the agent can ask for a list of hosts in advance.

## Decisions these pages challenge

Each challenge has its case on the page named. The decisions stay in force until polarzero changes
them.

| Decision | Why | Page |
|---|---|---|
| `secret-placeholder` | Its deny sentence ends a conversation after one `env` and protects nothing. A database password can be swapped after all | 1, 3 |
| `hold-limit-length` | Its premise, that Claude Code ends a command at 2 minutes, is false for 2.1.331 | 8 |
| `hold-sentence` | "A yes can no longer be given" after the hold: the ask should outlive it | 8 |
| `how-a-hold-ends` | A 403 hides Silo's words from git, and Claude Code retried a 403 on a model request twice | 5, 8 |
| `ending-the-encrypted-connection` | With github.com allowed, a worm's own token passes unread | 2 |
| `limits-on-a-secret` | Says "only GET", against the answer of `secret-limits`, and loses GraphQL reads sent as POST | 2 |
| `what-silo-changes-in-traffic` | "One way only" is already broken by the reply scrub of `secret-values`, and re-signing adds a third change | 1, 5 |
| `forwarded-byte-for-byte` | Page 5 proposes swapping a value you just made a secret in later model requests. Measured: the prompt cache keeps every prefix | 5 |
| `secret-values` | The scrub has no defined reach, so no test can check it | 1 |
| `content-kept-of-what-was-sent` | With sizes only, a push cannot be told from a fetch, and the PocketOS call is "233 B to backboard.railway.app" | 7 |
| `what-a-receipt-holds` | As worded, the shell's exit code of `npm test \| tail` reproduces the false claim | 7 |
| `recognising-a-conversation` and the word "conversation" | "With one model": a fallback switch starts a new conversation and resets its budget | 6 |
| `budget-rule-or-precaution` | "Exact" holds for dollars, not for a share of a window | 6 |
| `word-budget` | Counts only in dollars. A weekly point went from $4.45 to $2.31 in a month | 6 |
| `store-messages-once` | A plain hash of a message lets anyone holding the record test guesses of a short password | 5 |
| `fork-identity` | Says nothing of a remote database, which every case in the evidence used | 3 |

## Where the pages meet

- **Reading a push.** Pages 2 and 4 both read `git-receive-pack`. Page 2 has the boundary change GitHub's
  advertisement so that the client sends a full pack, not a thin one. Page 4 fills a thin pack from a
  mirror of the remote kept outside the computer. Both work. One trial should choose one, since G2
  (workflow files), G3 (files that run on the Mac) and G9 (refs moved) all need it.
- **One definition of "only read".** Pages 2, 3 and 7 all lean on `secret-limits`' "only read" and
  extend it the same way: GraphQL queries, Postgres through a read-only role, and the restore list.
  One entry could define it once for HTTP, GraphQL, Git and databases.
- **The boundary must read more than model traffic.** G2, G3, G6 and G9 all need the boundary to
  read some traffic that is not to a model provider. `ending-the-encrypted-connection` and
  `content-kept-of-what-was-sent` are the two decisions to look at together.
- **Holds.** Pages 3, 5 and 8 each hold something other than an HTTP request: a statement on a
  database connection, a model request, a push. Page 8's "ask that outlives its hold" and its exact
  matcher apply to all three.

## New entries proposed

`credential-minting-held`, `secret-signers`, `database-secret`, `database-branch-per-fork`,
`credential-found-in-text` (setting `found-value-not-kept`), `leaked-credential-held`,
`claims-against-the-record`, `what-a-restore-cannot-undo`, `ask-outlives-hold`,
`agent-requests-hosts`, `alerts-on-model-traffic`, `budget-in-a-window`, and a decision on
host API descriptions shipped as data (the counterpart of `ecosystem-data`). Each page gives the
kind, the part and the sentences.

## What went wrong while measuring

- During page 2's first recording, `npm` used this machine's own proxy setting instead of the
  recording proxy. It sent one `PUT` with a made-up token to the real registry.npmjs.org, which
  answered 404. No real credential was involved, and every client now starts from an empty
  environment.
- Page 8's agent ran `pkill -f` on broad patterns before it knew other agents shared the machine.
  That may have stopped another agent's `npm install` or fake server. No agent reported a run of its
  own that was cut.
- No credential was used or found. Every token in this folder is made up or is a vendor's published
  example, such as `AKIAIOSFODNN7EXAMPLE`. One recording of page 6 held the test machine's account
  id; it was deleted and recorded again from an empty environment.
