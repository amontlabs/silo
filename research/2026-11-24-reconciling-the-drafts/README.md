# Reconciling the drafts of 24 November 2026

For polarzero, 24 November 2026, by an agent. Fourteen draft pull requests of research and plan
entries were opened in parallel on 24 November. Each was written without seeing most of the
others, so several reach the same entries of the plan from different sides and some disagree. This
page reads all fourteen together. For each entry that more than one draft touches, it says where
they agree, where they conflict, and one position that takes the best of each. It ends with the
merge conflicts the drafts have with each other and an order to merge them in.

Nothing under `plan/` is changed by this page, and no verdict is written. Every position below is a
proposal, and the decisions in force stay in force. Entries are named by id: `plan show <id>`
prints one.

## The drafts

| PR | What it is | Edits `plan/` |
|---|---|---|
| #347 | Notes from six "to find out" checks run on Linux: holds, refusals, secrets, forks, the record's cost, the boundary spike | notes of 34 entries, no answer changed |
| #348 | The smallest first build, its seven missing entries, and its decisions first in the order | 7 new entries, 4 features edited, `order.json` rewritten |
| #350 | Real pains of people who give coding agents full access | no |
| #351 | What the record and budgets need to hold | no |
| #352 | Second opinions on the 56 decisions the first build needs | no |
| #353 | Holding destructive calls through allowed APIs | 6 new entries |
| #354 | The first run and the daily loop, as transcripts and sketches | no |
| #355 | Fixes for the gaps found in #350 and #347, with Linux prototypes | no |
| #356 | Five boundary questions, and answers a survey made untrue | 5 new entries, 4 answers edited |
| #358 | How a hostile agent gets past Silo's precautions | no |
| #359 | A Mac test kit for the questions only a Mac can answer | no |
| #360 | What Silo sends home, simulators for parallel agents, a Mac that stays on | 2 new entries |
| #361 | What Mac and iOS developers need from agents in a macOS computer | 5 new entries, notes of 9 |
| #362 | What a team of 5 to 50 needs, as entries | 18 new entries |

## In short

1. **Holds.** Five drafts pull the length of a hold in three directions: 90 seconds (#347, #350),
   10 minutes (#355), and a length per host below the client's own timeout (#353). They stop
   conflicting once an ask outlives its hold: a retry joins the open ask, and a yes given after the
   hold lets the next exact copy of the request through once. Then the hold only has to end before
   the program that sent the request gives up, which is a number per client, not one number.
2. **Secrets.** #347, #355 and #356 all find that `secret-placeholder`'s "a request that carries the
   placeholder to any other host is denied" ends a conversation after one `env`. #347 and #355
   agree on the fix: swap only in the places the secret names, forward a placeholder seen anywhere
   else, and record it. #356 keeps the deny sentence and adds a question about paths, which #353
   and #355 already answer.
3. **Keys that sign or mint tokens.** Three drafts propose three entries for the same family of
   cases: `signing-keys-re-signed` (#361), `cloud-credentials` and `exchanged-tokens` (#356), and
   notes in #347 that already re-signed AWS, Azure and Google requests and swapped a minted token on
   its way back. #356 rejects re-signing for Google on the grounds that no product does it. #347
   found that Codex and Claude Code already ship it for AWS, and re-signed a Google token request
   itself. The three entries fit together once that is corrected.
4. **What Silo changes in traffic.** Four drafts each need a second change to traffic that
   `what-silo-changes-in-traffic` rules out: re-signing (#347, #361), a placeholder put into a reply
   (#355, #356) and a request made undoable on your yes (#353). One new wording below covers all of
   them, instead of four separate challenges.
5. **Name lookups.** #351, #356 and #347 agree that Silo must be the computer's only resolver, and
   disagree on what a name off the list gets back. Answering it with the boundary's own address,
   without asking any other server, keeps the name in the computer (#356's point) and lets the
   refusal carry Silo's words (#347's point).
6. **gvproxy.** #352 proposes gvproxy as the first network driver with its route to the Mac's
   `127.0.0.1` removed. #358 found that gvproxy also serves an API on the computer's gateway that
   opens ports on every interface of the Mac, with no flag to turn it off; a token is the only
   guard. #348's `network-before-rules` says both are "turned off", and that main never reaches
   your Mac, which no stack tested so far shows. Only a patched gvproxy can be used.
7. **Simulators.** #360 and #361 each add a decision with the id `simulators-for-parallel-agents`,
   in two folders, with opposite answers. Merged together, `plan check` fails. #361's answer is the
   better one, with #360's record of who erased which simulator added to it.
8. **Merge order.** Seven drafts touch `plan/`. The research drafts merge cleanly in any order.
   #347 fails `plan check` on its own (one notes file without an entry). #348 rewrites
   `order.json`, and #360, #361 and #362 conflict with it there. Merge #348 first, then re-add the
   others' ids by hand, as listed at the end.

## Where the drafts disagree

Each section gives the entries, what each draft says, the position proposed, and why.

### 1. How long a hold lasts, and what happens to an ask nobody answered

Entries: `hold-limit-length`, `hold-sentence`, `how-a-hold-ends`, `program-gives-up-first`,
`asks-for-a-burst` (new in #353), `one-queue`. `hold-limit` is taken: a hold has a limit and Silo
ends it.

| Draft | Says |
|---|---|
| #347 | Measured: neither program ends a command at 2 minutes. Claude Code moves it to the background at 120 s and kills it at 720 s. Codex keeps it while the model waits on it. curl, git and Python wait with no limit; pip gives up at 15 s and sends the request again, 6 times. 90 s works for both programs and most tools. Proposes that a retry joins the open ask, and leaves to you whether a yes covers a request sent after the first one closed |
| #350 | Keep 90 s. A yes given after the hold becomes a grant for the same request (same computer, host, method, path and secret) for a short time |
| #353 | One open ask per computer: a held request joins it until you answer. A retry joins the line it repeats when it is the same request: the AWS SDK's request id, or the same method, host, path and body within 120 s. Each rule carries a hold below its client's own timeout: `aws`, `supabase`, `neonctl` and `heroku` give up at 60 s, and the Railway CLI gave up before a 90 s hold ended |
| #354 | An ask whose hold ended vanishes from `one-queue`, so the morning shows nothing. Keep refused-as-held requests on the list. Add "away": while you are away, a request a rule would hold is denied at once, with words that say so |
| #355 | The 90 s rests on a false premise. Hold 10 minutes, measured again for each version of each program. The ask outlives its hold, and a late yes lets that exact request pass once. #350's matcher by method and path let a 9,900.00 charge through on a yes to 40.00, so "the same request" must include the body |
| #358 | A computer may have only so many asks open in an hour; past it, new holds are refused with the reason |

Where they agree: the premise of `hold-limit-length` (Claude Code ends a command at 2 minutes) is
false, measured twice (#347, #355). Retries must join the open ask rather than make new ones
(#347, #353, #355). An unanswered ask must leave a trace you see later (#350, #354, #355).

Where they conflict: the number (90 s, 10 min, or below each client's timeout), and how a retry is
matched (method and path in #347 and #350; with the body in #353 and #355).

**Proposed position.**

- A hold ends before the program that sent the request gives up. The rule for a host carries that
  length when its client is known (`aws` and the Railway CLI: under 60 s; `git push` over HTTPS,
  which waits with no limit: up to 10 minutes). Where nothing is known, 90 s.
- The ask outlives its hold. It stays on the list of what needs you, marked "held, not answered",
  until you answer or set it aside.
- A request is the same request when its computer, host, method, path and a hash of its body match,
  or when its client sends a request id of its own (the AWS SDK's `amz-sdk-invocation-id`). While
  the ask is open, such a copy joins it.
- A yes given after the hold ended lets the next copy of that same request through, once, while the
  ask is open. The refusal the agent read says so: "held; the ask is still open; a retry passes if
  it is allowed".
- Held requests of one computer join one open ask until you answer it (#353's grouping). #358's cap
  on asks per hour and #354's "away" are later additions to the same mechanism, not alternatives to
  it.

Why: the 10 minutes of #355 is right for `git push` and wrong for `aws`, which closes at 60 s; a
hold past that returns Silo's words to a closed connection. The late yes is what makes the length
matter less: a hold no longer has to be long enough for you to wake up. The exact matcher is #355's
case: a coarse one grants a request you never saw.

Edits this implies: `hold-limit-length` (answer), `hold-sentence` ("a yes can no longer be given"
goes), `one-queue` (refused-as-held asks stay), `asks-for-a-burst` (the matcher above, and the
late yes). #355 names the new entry `ask-outlives-hold`; it can be the second half of
`asks-for-a-burst` instead of a third entry.

### 2. Which error a program reads at a refusal or a budget

Entries: `how-a-hold-ends`, `budget-denies-or-holds`.

| Draft | Says |
|---|---|
| #347 | Measured against Claude Code 2.1.331 and Codex 0.172.0, also on Bedrock and over Codex's WebSocket. For a model request: 451 for a hold and 402 for a budget for Claude Code (each sent once, Silo's words shown); 400 with a plain-text body for Codex (sent once, words shown). Codex sends every other 4xx six times; Claude Code takes a 403 for a failed sign-in, and on Bedrock retries a 403 for minutes. For tools: 403 with the sentence as the status line's reason and as a `text/plain` body reaches git, npm and pip. For SSH, PostgreSQL, MySQL and Redis: the refusal in the protocol's own first messages |
| #351 | Read from the programs' code and docs, not run: a 429 with `x-should-retry: false` and `billing_error` for Anthropic; a 429 with `error.code` `budget_reached` or `project_spend_limit_exceeded` for OpenAI formats |
| #353 | For a destructive call on a provider's API: the provider's own error form, carried by the rule. A plain 403 lost Silo's words on `gh`, `railway`, `vercel` and `git` |
| #355 | A held push answers in git's own `remote:` lines. A 403 makes git print a false "Everything up-to-date" |

Conflict: #347 and #351 pick different statuses for the same budget error. #351 also says Codex
retries a plain 429 four times; #347 measured one send and "exceeded retry limit".

**Proposed position.** Take #347's measured statuses as the answer of `budget-denies-or-holds` and
`how-a-hold-ends` for model requests: 451 (hold) and 402 (budget) for Claude Code, 400 with a
plain-text body for Codex, as an `error` event of status 400 on Codex's WebSocket. Per-host error
forms for other APIs come from the rule (#353), and a push is refused in git's report (#347's
notes on `merges-and-deploys-held`, #355). #351's 429 with `x-should-retry: false` is worth one run
in #347's rig before it is ruled out: it is what Anthropic's own gateway sends, so it may survive
future versions of Claude Code better than a 402. For Codex, #351's `project_spend_limit_exceeded`
makes Codex show "Your API project has reached its spend limit", which points you at OpenAI's
billing, the same flaw #347 found with `insufficient_quota`.

Why: #347 ran both programs; #351 read their code. Where they differ, the run wins.

### 3. Where a placeholder is swapped, and what happens to one seen elsewhere

Entries: `secret-placeholder` (answer edited by #356; notes by #347, #356, #361), `secret-limits`,
`limits-on-a-secret`, `secret-values`.

| Draft | Says |
|---|---|
| #347 | One `env` puts every placeholder into the conversation, so "a request that carries the placeholder to any other host is denied" ends that conversation for good and protects nothing. Swapping anywhere in a request writes real keys into the model's context and lets an agent publish a token through an issue body. Swap only where the secret names; forward a placeholder seen elsewhere and record it. Also: Basic authentication hides the placeholder in base64 (git, pip, uv, npm `_auth`, Twilio); Infura and Slack webhooks put the key in the URL |
| #355 | The same fix, built and tested: a secret is swapped only in its slots on its hosts. A slot is a named header, a scheme inside `Authorization`, the password of Basic authentication, a query parameter, a cookie or a path segment. A placeholder has the real value's length and prefix. Replies are scrubbed of the real value and its base64 forms |
| #356 | Keeps the deny sentence and adds "whether it can also be bound to a method and a path is to find out" |
| #361 | Adds a note pointing at `signing-keys-re-signed` |
| #353, #351 | "Only read" is decided by the operation, not the method: GraphQL and AWS send reads as POST (`operation-of-a-request`) |

**Proposed position.** Replace the deny sentence with #355's slots. Proposed answer:

> The computer holds a placeholder with the real value's prefix and length. The boundary swaps the
> real value in only in the places the secret names (a header, the password of Basic
> authentication, a query parameter, a path segment, the form field of a token request) and only
> on the way to the exact hosts you allowed. A placeholder seen anywhere else is forwarded as it is,
> and the record notes it. Keys that sign requests, SSH keys and database passwords are answered in
> `signing-keys-re-signed`, `ssh-keys` and a new `database-secret`.

#356's question about paths is answered by #353 and #355: a secret is limited by operation and by
place (named repositories, packages or buckets), in `secret-limits`, not by path. So #356's added
sentence can go.

Why: the deny sentence fails on the first `env`, the case every draft that tested it hit. #355 ran
the slot design against git, `gh`, boto3, Claude Code and the AWS SDK for JavaScript with only
placeholders in the computer.

### 4. Keys that sign, and tokens minted from a secret

Entries: `signing-keys-re-signed`, `apple-keys-in-a-computer` (new in #361), `cloud-credentials`,
`exchanged-tokens` (new in #356), notes of `secret-placeholder` (#347), `credential-minting-held`
(proposed by #355).

| Draft | Says |
|---|---|
| #347 | Tried: the boundary re-signs AWS SigV4 (CLI and boto3, uploads in parts, presigned links used from inside), Azure Shared Key, and an EKS token. It re-signs a Google service account's token request and puts a placeholder in place of the token in the reply. An MCP server's OAuth tokens are held outside the same way. Codex and Claude Code already ship AWS re-signing in their own proxies |
| #355 | Re-signing AWS works in its prototype. The hole left is minting: a swapped secret can create a new real credential (`sts:GetSessionToken`, a deploy key, `npm token create`), so each host's minting endpoints are held with an ask |
| #356 `cloud-credentials` | Silo answers as the cloud's metadata server at `169.254.169.254` and hands out placeholders for the tokens. Rejects re-signing for Google: "no product was found that does it" |
| #356 `exchanged-tokens` | At known token endpoints, the boundary makes the exchange and gives the computer placeholders for the tokens it gets back |
| #361 `signing-keys-re-signed` | A placeholder key of the same kind; the boundary re-signs. App Store Connect first, then AWS. A placeholder-signed request to another host is refused |

Where they agree: the key never enters the computer; the boundary signs; a token minted from a
secret comes back as a placeholder. #347's trials and #356's `exchanged-tokens` are the same
mechanism.

Where they conflict: #356 rejects re-signing for Google, which #347 did. #361 refuses a
placeholder-signed request to another host, where section 3 forwards a placeholder and records it.

**Proposed position.** Keep three entries, each with one job:

- `signing-keys-re-signed` (#361): the general rule for a key that signs: AWS SigV4, Azure Shared
  Key, App Store Connect, a Google service account's token request. Its last sentence follows
  section 3: a request signed with the placeholder towards another host is forwarded and recorded,
  since the signature is worthless there.
- `exchanged-tokens` (#356): a token minted from a secret comes back as a placeholder, refreshed by
  Silo. #355's minting endpoints (a new deploy key, `npm token create`) belong in its notes as the
  case it does not cover, with the hold #355 proposes.
- `cloud-credentials` (#356): the metadata server is how a computer gets cloud credentials with no
  key file at all, for the tools that look for it. Its option "re-sign at the boundary" should say
  that Silo already does that for a key file, as `signing-keys-re-signed` answers, and drop "no
  product was found".

`apple-keys-in-a-computer` (#361) depends on `signing-keys-re-signed` and does not conflict.

### 5. What Silo changes in traffic

Entry: `what-silo-changes-in-traffic` (open). It says Silo changes model traffic "in one way only":
the swap.

Four drafts need more than that: re-signing a Bedrock request (#347), signing an App Store Connect
token again (#361), a placeholder put into a reply in place of a minted token or of a secret's value
(#355, #356; #355 notes that the reply scrub of `secret-values` already breaks "one way only"), and
a destructive request sent with a final snapshot on your yes (#353's `made-undoable`). Each draft
challenges the entry on its own.

**Proposed position.** One rewording that covers all four, for the traffic Silo reads, not only
model traffic:

> Silo changes traffic it reads in three ways only, each recorded with what it replaced: it puts a
> secret to use, by swapping its value into the place it names or by signing the request again with
> it; it keeps a secret out, by putting a placeholder in a reply in place of a secret's value or of
> a token minted from it; and, on your yes or a rule you set, it sends a request changed so that the
> call can be undone (`made-undoable`). Anything else that a rule or a budget refuses is not changed:
> it is not sent.

### 6. A key Silo did not give

Entries: `secret-placeholder`, `logins`, `control-plane-hosts-read` (new in #353), proposed
entries in #351 and #355.

| Draft | Says |
|---|---|
| #351 | On every host Silo reads, a request whose credential is not one of the computer's own placeholders is denied. It stops a key from `.env`, an attacker's own key used to upload files to `api.anthropic.com`, and a push to an attacker's account |
| #355 | Narrower: a host that gets a secret accepts no other credential. A worm brings its own token |
| #353 | Rejects "deny these hosts unless a secret is held for them", because it breaks every CLI you signed in to inside the computer. Reads those hosts and holds destructive operations instead |
| #358 | The subscription login sits in the computer (`logins`). Remember a hash of it, and treat that token leaving to any other host as a fact |

Conflict: #351's rule, on every read host, denies exactly what #353 keeps working, and would also
deny Claude Code's own subscription login, which `logins` leaves in the computer.

**Proposed position.** Take #355's scope as the default rule: on a host where Silo holds a secret
for this computer, another credential is denied, except the agent program's own sign-in to its
provider (`logins`), which the record notes. #351's wider rule, on every read host, is a rule in a
set you switch on, for a limited computer. On hosts a set reads by operation (#353), a foreign
credential is a fact the ask shows. #358's hash of the login token goes in `logins`' notes.

### 7. Name lookups

Entries: `name-lookups` (new in #356), `network-modes`, `how-a-hold-ends`.

| Draft | Says |
|---|---|
| #356 | Silo answers every lookup. In a limited computer, a name not on the list gets "no such name", and no other server is asked, so the name never leaves. A connection goes only to an address Silo gave, and its handshake must name that host. ECH keys are left out of answers |
| #351 | Silo is the only resolver. An allowed name gets a private placeholder address mapped to it (as NVIDIA OpenShell does); the real name is resolved when a program connects. No UDP leaves but Silo's own lookups; QUIC is refused |
| #347 | A refused or "no such name" lookup reads to every tool as a network fault, often a temporary one that invites a retry, and carries no words. Answer with the boundary's address and refuse the request, with words. Also: Silo resolves the name itself and connects to what it found, whatever address the computer asked for |

They agree on Silo as the only resolver, the handshake check and leaving ECH keys out.

**Proposed position.** Every name, listed or not, is answered with an address of the boundary,
without asking any other server for a name off the list. That is #351's placeholder address, and
it keeps #356's property that the name never leaves. A connection to that address is then allowed
or refused with Silo's words, as #347 asks. The boundary resolves a listed name itself when it
connects. "No such name" is kept for a lookup that a rule holds as such, such as a name that looks
like encoded data. A closed computer gets no answer at all, as #356 says.

### 8. The network stack in the first build, and gvproxy

Entries: `network-stack`, `network-before-rules` (new in #348), `reaching-a-computer`,
`device-out-of-reach`.

| Draft | Says |
|---|---|
| #352 | gvproxy as the first driver, with its route from `192.168.127.254` to the Mac's `127.0.0.1` removed. Apple's NAT, shown as no boundary, only until the Mac test passes |
| #358 | Tested: from inside, with no token, one `curl` to `192.168.127.1/services/forwarder/expose` made gvproxy listen on `0.0.0.0:2299` of the host. The gateway API starts unconditionally and has no flag to turn it off. Remove it in code. Proposed property `nothing-inside-reaches-the-stack` |
| #351 | gvproxy and passt forward unknown names to the Mac's resolver, so neither can be used as it is |
| #348 | `network-before-rules`: open with your yes, or closed. "Open reaches the internet but never your Mac". Its notes say gvproxy's loopback route and API are "turned off before main runs on it"; its `first-build.md` says the API gets a token |
| #347 | macOS 27's per-machine `vmnet` networks still give Silo no way to read or refuse a packet |
| #354 | If the first build runs main on Apple's NAT, main reaches everything, and the first run has to say so |
| #359 | Test 07 runs gvproxy with a macOS guest against Apple's NAT, and checks what of the Mac the computer reaches. Its header notes that the Mac's own network address stays reachable through any stack that dials out |

Conflicts: `network-stack` (open) says "never Apple's NAT"; #352 and #348 allow it in the first
build. gvproxy's API cannot be turned off without a code change (#358); a token protects it, which
is #348's and #352's fix, and #358 asks for the API to be removed. "Never your Mac" is not shown
either: the Mac's own network address is in reach through any stack that dials out, so a service of
the Mac that listens on every interface is in reach of main.

**Proposed position.** gvproxy is used only as a patched copy: no gateway HTTP API (a token is the
stopgap until the patch), an empty NAT table with loopback off, its API on a Unix socket of Silo's
user, and name lookups answered by Silo (section 7). Until that copy passes #359's test 07, the
first build runs main on Apple's NAT, and `network-before-rules` says so in its own words: open,
not checked, not recorded, and in reach of services on your Mac that listen on every interface.
`network-stack` then reads "never Apple's NAT once rules exist", which is the case it was written
for. #358's property `nothing-inside-reaches-the-stack` goes beside it.

### 9. Going back on a shared main: one path

Entries: `putting-back-a-path` and `a-path-from-a-checkpoint` (new in #348), `fork-from-the-moment`
(taken), `one-run-grain` and `putting-back-a-runs-paths` (taken).

#354 proposes `silo look` and `silo put-back` for the first build and calls them a challenge to
`fork-from-the-moment`, which says "on a main shared by several agents you do not rewind; you fork
from the moment". #348 writes the same thing as an entry and argues it is no challenge, because
`putting-back-a-runs-paths` (taken) already puts paths back into a running main. #351's morning
screen has a "put back" button on a deleted folder.

The two taken decisions read against each other: one says you do not rewind a shared main, the
other puts a run's paths back into it. Agents cannot settle that by `--by agents`, since both were
ruled by you.

**Proposed position.** Keep #348's entries. Add, for you to take or drop, a decision that replaces
`fork-from-the-moment` with both sentences: "On a main shared by several agents you do not restore
the whole computer. You put back the paths you name, after a checkpoint of now, or you fork from the
moment." Without it, every later draft will raise the same question again.

### 10. How often checkpoints are taken, and which ones thinning may remove

Entries: `thinning-checkpoints`, `checkpoint-at-each-turn`, `checkpoint-per-system`,
`restore-saves-first`, `fork-shares-disk-blocks`.

| Draft | Says |
|---|---|
| #351 | The checkpoint before a deletion at 02:47 is thinned at 04:47. Keep a checkpoint an open alert points to. Take a checkpoint at each turn, so a deletion falls inside one tool call |
| #352 | A checkpoint makes every write in main wait for the whole clone, and the wait grows with fragmentation (11 s for one 4 KiB write on XFS at 500,000 pieces). Frequent checkpoints keep the image at its worst. Keep every checkpoint for 24 hours, never remove the save before a restore or a checkpoint not yet compared. A minimum gap between checkpoints |
| #354 | `restore-saves-first` lets the save before a restore be thinned two hours later. Keep it until set aside |
| #347 | Same fragmentation measures. macOS 27's DiskImageKit could make a checkpoint "a new empty top layer" |
| #352 | Read UTM's source: the layers of an attached disk cannot change while it runs, and there is no public API to merge one back. A layer per boot is possible, a layer per checkpoint is not. #348's `first-build.md` already follows this |

Conflicts: a checkpoint at each turn (#351) against a minimum gap (#352). DiskImageKit per
checkpoint: #347 proposes it, #352 shows it cannot be done today.

**Proposed position.**

- `thinning-checkpoints` keeps, whatever their age: the save before a restore until you set it
  aside, a checkpoint an open alert points to, and a checkpoint whose comparison is not written yet.
  All three drafts ask for one of these, and none conflicts with another.
- Whether the full window is 2 hours or 24 hours is for you. 24 hours costs only the blocks changed
  in a day, and it is what keeps the night's checkpoints whole until the morning.
- `checkpoint-at-each-turn` waits for #359's tests 03 and 15 on APFS. If a clone stalls main's
  writes as it does on XFS, a checkpoint at each turn needs #352's minimum gap.
- DiskImageKit layers per checkpoint are dropped from `checkpoint-per-system`'s notes; #352's reading
  replaces #347's.

### 11. Simulators for several agents in one macOS computer

Entries: `simulators-for-parallel-agents`, added twice: `plan/computers/decisions/` by #360 and
`plan/recipes/decisions/` by #361. `sizing-target`.

| Draft | Says |
|---|---|
| #360 | Silo does not choose. A recipe can create several named simulators, and the record shows which process booted, erased or installed into each, so an agent that wiped another's is seen. Giving each conversation its own simulator is rejected: no supported way was found to force the device an agent program picks |
| #361 | Each agent gets its own simulator and build folder, through a piece Silo ships that uses the agent programs' documented settings (hooks, the environment), with a maximum booted at once that fits main's memory. Each booted simulator holds about 3.4 GB |
| #361 on `sizing-target` | 16 agents in main on a 32 GB Mac cannot each have a booted simulator. For an iOS developer, the load is the number of simulators |
| #350 | Measure main plus one and two Linux forks on a 16 GB Mac before `sizing-target` is promised |

Merged together, the two files share an id and `plan check` fails.

**Proposed position.** One entry, #361's, in `recipes`, since it is a piece Silo ships. Add #360's
sentence to its answer: the record shows which process booted, erased or installed into each
simulator. That is what you read in the morning if the piece fails. Delete #360's file and its line
in `order.json`. #360's doubt (no way to force the device) becomes the first thing #361's Mac test 25
checks. `sizing-target` gains a third load from #361: a few booted simulators shared by many agents,
with the maximum shown.

### 12. What a new macOS computer starts from

Entries: `computer-image`, `xcode-in-a-computer` (new in #361), `mac-silo-needs` (new in #348),
`fork-identity`.

#352 and #348 make each new macOS computer from the installed system kept before its first start,
so that Apple's guest provisioning gives it its user and SSH on that start. Apple honours
provisioning "only if the guest has not already been set up" (#347's notes on `computer-image`).
#361 proposes keeping the installed state with Xcode, "so later macOS computers start from a clone
that already has Xcode". Installing Xcode needs a set-up, logged-in system, so a clone of that state
is already set up: provisioning will not run on it, and it carries the first computer's user,
machine identity and SSH host keys.

**Proposed position.** Two kinds of start, said apart in `computer-image`. A new computer starts
from the never-started system and is provisioned. A computer started from a set-up state, Xcode
included, is treated as a fork of that state: `fork-identity` gives it a new identity before
anything in it runs. Or Xcode is copied in on each new computer's first run, from the store outside
computers that `shared-cache` allows for Apple-signed files, which #361 already proposes for
simulator runtimes. #359's test 13 (ten fresh computers, one with a new machine identifier) and
#361's test 23 tell which costs less. #354's reuse of a restore image already on the Mac is an
independent edit and fits both.

### 13. A push to your phone, and nothing sent home

Entries: `phone-app`, `forwarding-to-a-phone`, `what-silo-sends-home` (new in #360),
`reaching-a-host` (new in #362).

#354 designs the lock-screen ask on Apple's push service (APNs). #360 answers that Silo sends
nothing to Amont Labs unless you turn it on, and #362 that Silo runs no relay of its own. A push to
an iPhone app is sent with that app's push key, which only its developer holds, so a push from Silo
needs a sender that holds Amont Labs' key. None of the three drafts says who sends it.

**Proposed position.** Not settled by the drafts. One way to look at first: the Mac writes the ask
into your own private iCloud database, and the phone app subscribes to it, so that Apple, not Amont
Labs, sends the push. Not checked. Until it is, `what-silo-sends-home` should name the phone's
push as the one exception it would make, or `phone-app` should say it works only while the phone
reaches the Mac (`reaching-a-host`).

### 14. Budgets in a subscription's window

Entries: `budgets-on-rates`, `budget-rule-or-precaution`, `word-budget`.

#351 proposes "main uses at most 30 percent of this week's limit", read from the provider's headers.
#355 measured the same idea and splits it: a reserve ("stop this computer's calls once the week is
70 percent used") is a rule, exact to about 0.3 points, because the provider reports the account's
use. A share for one computer cannot be read from any reply and has to be estimated, which drifts by
a factor of 2 to 4 once your laptop also spends; so it can only be a precaution. #347 found the
headers both programs read.

**Proposed position.** #355's split. #351's 30 percent is a share, so it is a precaution, and the
rule offered first is the reserve.

### 15. Smaller factual differences

- **Download size.** #352, #354 and #348 give 25.6 GB for macOS 27.2.1's restore image; #359's kit
  says "about 18 GB". One of them read a different build; the kit should print the size it downloads.
- **Mac test numbers.** #352 numbers its Mac tests 1 to 21, and #359's kit uses the same numbers
  (01 to 22). #348's `research/2026-11-24-first-build.md` has its own list, where gvproxy is test 6,
  and #354 quotes an earlier version where it was 5. #361 adds tests 23 to 27 in its own folder. Use the kit's numbers
  everywhere, and move #361's tests into `research/mac-test-kit/tests/` once both are merged.
- **SSH keys.** #347 (notes) and #356 (`ssh-keys`) agree: an SSH agent outside the computer, bound to
  the hosts allowed. #356 adds the rewrite of Git remotes to HTTPS and the check that a signature is
  for a connection Silo saw. #347's trial used OpenSSH's own destination limits (`ssh-add -h`). Both
  find some clients cannot use such a key (#347: libssh; #356: Paramiko, JGit, OpenSSH before 8.9).
  No conflict; #347's note belongs under `ssh-keys` once it exists.
- **Duplicate notes.** #347 writes notes for `fork-and-sign-in` in two places. One is
  `plan/going-back/decisions/fork-and-sign-in.md`, beside no entry, so #347 fails `plan check` on its
  own. Its text belongs at the end of `plan/agent-programs/decisions/fork-and-sign-in.md`.

## Where the drafts agree

These need no ruling between drafts. Each is named so it is said in one place.

| Point | Drafts | Where it goes |
|---|---|---|
| "Only read" is decided by the operation (GraphQL query or mutation, AWS action), not the method | #347, #351, #353, #355 | `secret-limits`, with `operation-of-a-request` (#353) |
| A tool the provider runs is refused in a limited computer, by what the request declares | #351, #356, #358; #347 found the probe cannot make such a tool fail | `tools-the-provider-runs` (#356). #358 adds that it holds only if the boundary reads every request whole (its proposed `boundary-reads-whole`) |
| Destructive calls to named APIs are held by operation | #351, #353, #355 | `destructive-calls-held` (#353). #355's GitHub classifier and database proxy are its data |
| A client that refuses Silo's certificate fails closed and says why; Silo sets a fixed list of trust settings when it makes a computer | #347, #353, #361 (simulators keep their own store) | `ending-the-encrypted-connection` |
| macOS 27 on the Mac and in the computer, with Apple's guest provisioning | #347, #348, #352, #354, #359 | `mac-silo-needs` (#348) |
| Main set up for agents at the first run, runs that outlive the terminal, the Mac kept awake | #348, #352, #354, #360 | #348's three entries |
| Main's network is open in the first build, and Silo says so | #348, #352, #354 | `network-before-rules` (#348), with section 8 |
| A conversation belongs to one computer: a fork starts its own | #347 | `recognising-a-conversation`; no other draft disagrees |
| A hash chain in the record, and the person in each control fact, from the record's first version | #362 | `record-edits-show`, `person-in-a-control-fact`. Both are ground work: they belong in #348's first-build block of `order.json`, beside `record-file-format` |
| The disk reader is attack surface: a confined helper, fuzzed, never a mount | #352, #358 | `file-changes-from-outside` |

## Merge conflicts and an order to merge in

Every draft was merged with every other on a copy of `main` (`1b8f191`), and `plan check` was run
on the result. The research-only drafts (#350, #351, #352, #354, #355, #358, #359) conflict with
nothing.

| Pair | Conflicting files | How to resolve |
|---|---|---|
| #347 and #356 | `secret-placeholder.md` | Keep both sections; then section 3 |
| #347 and #361 | `package-age-rule.md`, `registry-traffic-read.md`, `secret-placeholder.md`, `computer-image.md` | Keep both sections in each; all are added notes |
| #356 and #361 | `secret-placeholder.md`, `order.json` | Keep both; in `order.json`, `ssh-keys`, `exchanged-tokens`, `cloud-credentials`, `signing-keys-re-signed` and `apple-keys-in-a-computer` all follow `secret-placeholder` |
| #360 and #361 | `main-on-macos-first.md` | Keep both sections. They also add the same id in two folders, which `plan check` refuses: section 11 |
| #348 and #360, #361, #362 | `order.json` | #348 moves the entries they insert next to. Re-add their ids as below |

What fails `plan check` after a clean merge:

- #347 alone: `plan/going-back/decisions/fork-and-sign-in.md` is notes without an entry (section 15).
- #360 with #361: the id `simulators-for-parallel-agents` twice (section 11).

**The order proposed.**

1. Before anything merges: #347 moves its second `fork-and-sign-in` notes into the agent-programs
   file; #352 renames `research/first-build-decisions/` to `research/2026-11-24-first-build-decisions/`,
   the name #354 and #359 already link to and the project's files use; #360 drops its
   `simulators-for-parallel-agents` in favour of #361's (section 11).
2. The research drafts, in any order: #350, #351, #352, #354, #355, #358, #359.
3. #348. It holds the first build, which is what you rule on first, and it rewrites `order.json`.
4. #347, then #353, then #356. #356 conflicts only with #347's notes.
5. #360, #361 and #362, each re-adding its ids to `order.json` after #348:
   - #360: `what-silo-sends-home` after `nothing-to-set-up`.
   - #361: `simulators-for-parallel-agents` after `silo-ships-recipes`; `xcode-in-a-computer`
     beside it; `builds-on-your-iphone` after `asks-are-native`; `signing-keys-re-signed` and
     `apple-keys-in-a-computer` after `secret-placeholder`.
   - #362: `record-edits-show` after `record-file-format` and `person-in-a-control-fact` after
     `who-made-each-change`, both now in #348's first-build block; its fourteen team entries after
     `forwarding-tool`.

This was run on a copy: all fourteen merged in this order, with the fixes of step 1 and the ids
placed as above, pass `plan check`.

## Why this pull request adds no notes to the plan's entries

Every entry in conflict above already has its notes edited by at least one of the open drafts. A
note added here would be one more conflict to resolve in each of them. Once the drafts are merged,
each section's "Proposed position" can be written into the notes of the entries it names, or into
the replacing decision it proposes, in one pass.
