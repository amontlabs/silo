# Reconciling the drafts of 24 November 2026

For polarzero, 24 November 2026, by an agent. Twenty draft pull requests of research and plan
entries were opened in parallel on 24 November. Each was written without seeing most of the
others, so several reach the same entries of the plan from different sides and some disagree. This
page reads all twenty together, at the heads they had in the evening of 24 November. For each entry that more than one draft touches, it says where
they agree, where they conflict, and one position that takes the best of each. It ends with the
merge conflicts the drafts have with each other and an order to merge them in.

Nothing under `plan/` is changed by this page, and no verdict is written. Every position below is a
proposal, and the decisions in force stay in force. Entries are named by id: `plan show <id>`
prints one.

## The drafts

| PR | What it is | Edits `plan/` |
|---|---|---|
| #347 | Notes from six "to find out" checks run on Linux: holds, refusals, secrets, forks, the record's cost, the boundary spike. Since updated: uploads held and boto3, the hosts installs reach, sign-ins held outside a fork, vendor terms read, HTTP/3 refused | notes of about 40 entries, no answer changed |
| #348 | The smallest first build, its seven missing entries, and its decisions first in the order | 7 new entries, 4 features edited, `order.json` rewritten |
| #350 | Real pains of people who give coding agents full access | no |
| #351 | What the record and budgets need to hold | no |
| #352 | Second opinions on the 56 decisions the first build needs | no |
| #353 | Holding destructive calls through allowed APIs | 6 new entries |
| #354 | The first run and the daily loop, as transcripts and sketches | no |
| #355 | Fixes for the gaps found in #350 and #347, with Linux prototypes | no |
| #356 | Five boundary questions, and answers a survey made untrue | 5 new entries, 4 answers edited |
| #358 | How a hostile agent gets past Silo's precautions | no |
| #359 | A Mac test kit for the questions only a Mac can answer. Since updated with tests 23 to 33: Mac and iOS developers, the checkpoint engine on APFS, a boot from NBD, TRIM | no |
| #360 | What Silo sends home, simulators for parallel agents, a Mac that stays on | 2 new entries |
| #361 | What Mac and iOS developers need from agents in a macOS computer | 5 new entries, notes of 9 |
| #362 | What a team of 5 to 50 needs, as entries | 18 new entries |
| #363 | Fixes for #358's attacks, built and measured: a login vault, gvproxy sealed, previews against a hostile page, a strict reader | notes of 6 entries |
| #365 | A checkpoint engine prototype: a block bitmap instead of a clone, crash-safe folders, thinning over 40 days, putting back | notes of 6 entries |
| #366 | What a Linux host needs: KVM, file systems, neighbours, the cost of a user-space stack | notes of 5 entries |
| #367 | Every other agent program through the boundary, on Linux | 2 new entries, notes of 8. All of it is also in #369 |
| #368 | Every provider's terms read against Silo | 2 new entries, notes of 1 |
| #369 | #367, plus its follow-ups: a refusal every program shows, holds the programs wait for, a probe that lets a program's own work through | 2 new entries, notes of 9 |

## In short

1. **Holds.** Five drafts pull the length of a hold in three directions: 90 seconds (#347, #350),
   10 minutes (#355), and a length per host below the client's own timeout (#353). They stop
   conflicting once an ask outlives its hold: a retry joins the open ask, and a yes given after the
   hold lets the next exact copy of the request through once. Then the hold only has to end before
   the program that sent the request gives up, which is a number per client, not one number. #369
   adds the case of a model reply: once Silo has sent the reply's headers and a keep-alive the
   format documents (an event, for Codex), every agent program tried waited 300 s or more, so a
   held model reply is limited by you, not by the program.
2. **Refusals.** #369 found the one status every agent program tried stops on and shows: 400, with
   Silo's sentence in the provider's own error shape. It replaces #347's 451 and 402, and #367's
   finding that a 403 is retried, which came from refusing the program's sign-in too.
3. **Secrets.** #347, #355 and #356 all find that `secret-placeholder`'s "a request that carries the
   placeholder to any other host is denied" ends a conversation after one `env`. #347 and #355
   agree on the fix: swap only in the places the secret names, forward a placeholder seen anywhere
   else, and record it. #356 keeps the deny sentence and adds a question about paths, which #353
   and #355 already answer.
4. **Keys that sign or mint tokens.** Three drafts propose three entries for the same family of
   cases: `signing-keys-re-signed` (#361), `cloud-credentials` and `exchanged-tokens` (#356), and
   notes in #347 that already re-signed AWS, Azure and Google requests and swapped a minted token on
   its way back. #356 rejects re-signing for Google on the grounds that no product does it. #347
   found that Codex and Claude Code already ship it for AWS, and re-signed a Google token request
   itself. The three entries fit together once that is corrected.
5. **What Silo changes in traffic.** Four drafts each need a second change to traffic that
   `what-silo-changes-in-traffic` rules out: re-signing (#347, #361), a placeholder put into a reply
   (#355, #356) and a request made undoable on your yes (#353). One new wording below covers all of
   them, instead of four separate challenges.
6. **Name lookups.** #351, #356 and #347 agree that Silo must be the computer's only resolver, and
   disagree on what a name off the list gets back. Answering it with the boundary's own address,
   without asking any other server, keeps the name in the computer (#356's point) and lets the
   refusal carry Silo's words (#347's point).
7. **gvproxy.** #352 proposes gvproxy as the first network driver with its route to the Mac's
   `127.0.0.1` removed. #358 found that gvproxy also serves an API on the computer's gateway that
   opens ports on every interface of the Mac, with no flag to turn it off; a token is the only
   guard. #348's `network-before-rules` says both are "turned off", and that main never reaches
   your Mac. #363 built the patch: 64 lines, after which a computer reaches neither the Mac's
   loopback nor its LAN address. Only that patched gvproxy can be used.
8. **A program's sign-in held outside the computer.** #363 built a login vault that makes a copied
   login useless, and #347 showed that a sign-in held outside is what lets a fork keep it without
   signing main out. #368 read the terms: Anthropic and Google forbid holding a plan's sign-in, so
   the vault fits API keys and MCP servers' sign-ins, not a plan's.
9. **Checkpoints.** #365 proposes taking a checkpoint from Silo's own record of the blocks written,
   which makes the checkpoint's moment under 1 µs and the longest write 0.3 s, where a clone made
   every write wait 11 s, and #366 measured the same
   question on a Linux host. Both agree; #359's new tests 31 and 32 are the Mac checks it waits on.
10. **Simulators.** #360 and #361 each add a decision with the id `simulators-for-parallel-agents`,
   in two folders, with opposite answers. Merged together, `plan check` fails. #361's answer is the
   better one, with #360's record of who erased which simulator added to it.
11. **Merge order.** The research drafts merge cleanly in any order. #367 is contained in #369
   and can be closed. #348 rewrites `order.json`, and #360, #361, #362 and #369 conflict with it
   there. Merge #348 first, then re-add the others' ids by hand, as listed at the end. All the
   drafts merged in that order pass `plan check`.

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
| #347, update | An upload is held before its body: curl and Python `requests` waited out 150 s, boto3 closed at 60 s and sent the whole request again, three times |
| #369 (with #367) | A model host that sends nothing: Codex gives up at 15 s, Gemini CLI at 60 s, Qwen Code at 120 s, Goose and Aider at 600 s, Claude Code and OpenCode never close. Once the status and headers are sent, Claude Code gives up at 180 s and Codex and OpenCode at 300 s. With a byte every 5 s that the format documents (`ping` on Anthropic's Messages, a comment on Gemini's stream), every program waited the whole 300 s and took the reply; Codex needs an event on Responses and waited 420 s on one |

Where they agree: the premise of `hold-limit-length` (Claude Code ends a command at 2 minutes) is
false, measured twice (#347, #355). Retries must join the open ask rather than make new ones
(#347, #353, #355). An unanswered ask must leave a trace you see later (#350, #354, #355).

Where they conflict: the number (90 s, 10 min, or below each client's timeout), and how a retry is
matched (method and path in #347 and #350; with the body in #353 and #355).

What the new evidence changes: the two kinds of hold have different limits. A held tool request
(`aws`, boto3, pip, the Railway CLI) is limited by the tool, which closes at 15 to 60 s and sends it
again. A held model reply is not limited by the program at all, as long as Silo sends the headers
and a documented keep-alive while it holds; #369's numbers to the first byte (Codex 15 s) apply only
if Silo holds before any byte. In a real hold of a tool call, the provider's reply has already
started, so the headers have already gone.

**Proposed position.**

- A hold ends before the program that sent the request gives up. The rule for a host carries that
  length when its client is known (`aws`, boto3 and the Railway CLI: under 60 s; `git push` over
  HTTPS, which waits with no limit: up to 10 minutes). Where nothing is known, 90 s.
- A held model reply sends its headers at once and a keep-alive the format documents every few
  seconds: `ping` on Anthropic's Messages, a comment on Gemini's stream, and on OpenAI's Responses
  an event Codex ignores. #369 found the Responses format documents no harmless event and an
  unknown event broke OpenCode and Cline on Messages, so the event is chosen per format, never one
  for all. Such a hold lasts as long as `classifier-wait-limit` or you allow, not a client's limit.
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
hold past that returns Silo's words to a closed connection. For a model reply, #369 measured that
none of the programs is the limit once the keep-alive flows, so a fixed 90 s there would cut short a
hold no program needed cut. The late yes is what makes the length
matter less: a hold no longer has to be long enough for you to wake up. The exact matcher is #355's
case: a coarse one grants a request you never saw.

Edits this implies: `hold-limit-length` (answer, with the two kinds of hold), `how-a-hold-ends` (the keep-alive per format), `hold-sentence` ("a yes can no longer be given"
goes), `one-queue` (refused-as-held asks stay), `asks-for-a-burst` (the matcher above, and the
late yes). #355 names the new entry `ask-outlives-hold`; it can be the second half of
`asks-for-a-burst` instead of a third entry.

### 2. Which error a program reads at a refusal or a budget

Entries: `how-a-hold-ends`, `budget-denies-or-holds`, `denial-per-program`.

| Draft | Says |
|---|---|
| #347 | Measured against Claude Code 2.1.331 and Codex 0.172.0, also on Bedrock and over Codex's WebSocket. For a model request: 451 for a hold and 402 for a budget for Claude Code (each sent once, Silo's words shown); 400 with a plain-text body for Codex (sent once, words shown). Codex sends every other 4xx six times; Claude Code takes a 403 for a failed sign-in, and on Bedrock retries a 403 for minutes. For tools: 403 with the sentence as the status line's reason and as a `text/plain` body reaches git, npm and pip. For SSH, PostgreSQL, MySQL and Redis: the refusal in the protocol's own first messages |
| #351 | Read from the programs' code and docs, not run: a 429 with `x-should-retry: false` and `billing_error` for Anthropic; a 429 with `error.code` `budget_reached` or `project_spend_limit_exceeded` for OpenAI formats |
| #353 | For a destructive call on a provider's API: the provider's own error form, carried by the rule. A plain 403 lost Silo's words on `gh`, `railway`, `vercel` and `git` |
| #355 | A held push answers in git's own `remote:` lines. A 403 makes git print a false "Everything up-to-date" |
| #367 | Eight programs, every request to the model host refused with 403, then 429. Claude Code, Qwen Code, OpenCode and Aider retried the 403 for the whole minute; only Cline and Goose stopped at the first. Codex did not show the body of a 429. "No status tried made every program stop and show Silo's words" |
| #369 | Eight programs, Copilot CLI in place of Qwen Code, refusing only the model requests: 400 in the provider's error shape (`invalid_request_error`, or `{"error": {"code": 400}}` for Google) stopped all eight within 9 s and each printed Silo's sentence. With 403, Aider retried and Copilot showed its own words. On 402 Goose tells you to add credits with your provider; on 404 Claude Code says the model may not exist; on 401 Claude Code retries silently |

Conflicts: #347 and #351 pick different statuses for the same budget error. #367 says a 403 is
retried by Claude Code; #347 and #369 saw Claude Code stop on it. #369 explains why: #367's stand-in
refused the program's sign-in and settings calls too, so Claude Code was retrying a failed sign-in,
which is #347's own finding about 403.

**Proposed position.** For a model request Silo refuses, by a budget, a rule or a hold that ended:
400 in the provider's own error shape, with Silo's sentence as the message, for every program. It is
the only status every program tried stops on and shows. #347's per-program statuses (451 and 402
for Claude Code) are dropped: one status is simpler to keep, and 402 sends Goose's users to their
provider's billing. #351's 429 is ruled out by #367 (Codex hides the body of a 429). A refusal
always refuses only the model request, never the program's sign-in or settings calls on the same
host, or Claude Code retries. The record expects the same refused request more than once (Goose
sent it 8 times), and the alert in Silo's app stays what tells you for sure.

Other APIs get the provider's own error form from the rule (#353), and a push is refused in git's
report (#347's notes on `merges-and-deploys-held`, #355).

The cost, which #369 names: a 400 means "your request is wrong", which a budget is not, and a
program may one day answer it by changing the request. `denial-per-program` already checks each
program's version; the status belongs in that check.

Why: #369 ran eight programs where #347 ran two and #351 read code. Where they differ, the widest run
wins.

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
| #363 | Built the patch, `-sealed`, 64 lines: no API on the gateway, no address mapped to the host, and no dial to loopback, private, link-local or the host's own addresses. Measured: stock and token-guarded gvproxy reach 6 ports of the Mac's loopback and a service on its LAN address; sealed reaches neither, and the internet still works |
| #366 | Measured on Linux: gvproxy carries 1.4 Gbit/s at MTU 1500 and 4.1 Gbit/s at 65520, against 14.7 for the kernel. An `npm install` is not slowed; two computers exchanging gigabytes are. Silo's stack should take large frames from the start |

Conflicts: `network-stack` (open) says "never Apple's NAT"; #352 and #348 allow it in the first
build. gvproxy's API cannot be turned off without a code change (#358); a token protects it, which
is #348's and #352's fix, and #358 asks for the API to be removed. "Never your Mac" is not shown
either with stock gvproxy: a service of the Mac on its LAN address is in reach of main. #363's
sealed gvproxy closes that too, by refusing every private address, which #359's header thought no
stack that dials out could do.

**Proposed position.** gvproxy is used only as #363's sealed copy: no gateway HTTP API, no address
mapped to the host, no dial to loopback, private, link-local or the Mac's own addresses, its API on
a Unix socket of Silo's user, large frames (#366), and name lookups answered by Silo (section 7).
A token on the API is a stopgap only while the patch is not in. A private address you do want, such
as a database on your LAN, is then a rule that names it. Until that copy passes #359's test 07 with
a macOS guest, the first build runs main on Apple's NAT, and `network-before-rules` says so in its own words: open,
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
`checkpoint-storage`, `restore-saves-first`, `fork-shares-disk-blocks`, `putting-back-one-run`.

| Draft | Says |
|---|---|
| #351 | The checkpoint before a deletion at 02:47 is thinned at 04:47. Keep a checkpoint an open alert points to. Take a checkpoint at each turn, so a deletion falls inside one tool call |
| #352 | A checkpoint makes every write in main wait for the whole clone, and the wait grows with fragmentation (11 s for one 4 KiB write on XFS at 500,000 pieces). Frequent checkpoints keep the image at its worst. Keep every checkpoint for 24 hours, never remove the save before a restore or a checkpoint not yet compared. A minimum gap between checkpoints |
| #354 | `restore-saves-first` lets the save before a restore be thinned two hours later. Keep it until set aside |
| #347 | Same fragmentation measures. macOS 27's DiskImageKit could make a checkpoint "a new empty top layer" |
| #352 | Read UTM's source: the layers of an attached disk cannot change while it runs, and there is no public API to merge one back. A layer per boot is possible, a layer per checkpoint is not. #348's `first-build.md` already follows this |
| #365 | A prototype engine. Silo marks each block written in a bitmap where the writes pass (libkrun's virtio-blk for Linux, a disk Silo serves over NBD for macOS); a checkpoint's moment is a swap of two bitmaps, under 1 µs, and the blocks are copied afterwards. Longest write during a checkpoint 0.16 to 0.31 s, against 9.2 to 11.8 s for a clone. Challenges `checkpoint-per-system`'s "by the same means as a fork's disk". Also: one checkpoint at a time, 12 to 16 s each, so at three turns a minute some turns get none; 30 days of dailies cost 247 to 413 GiB under its model of an agent's writes; a full volume breaks the computer's own writes, so a floor of free space; thinning keeps the first of each hour and of each day |
| #366 | On a Linux host: other computers on the same file system keep most of their writes during one's copy; a restore takes as long as a checkpoint (0.8 s from a Btrfs subvolume snapshot); a restored disk reads 3 to 8 times slower, so a host rewrites a stopped computer's disk in one piece from time to time. XFS is the better host if #365's engine is built, Btrfs if not |
| #359, update | Test 31 runs #365's engine on APFS, test 32 boots a macOS computer from an NBD disk, test 33 checks that TRIM reaches the image |

Conflicts: a checkpoint at each turn (#351) against a minimum gap (#352). DiskImageKit per
checkpoint: #347 proposes it, #352 shows it cannot be done today. #365 and #366 agree with each
other, and #365's engine dissolves the first conflict: with the bitmap, a checkpoint at each turn
costs the turn under 1 µs, and the minimum gap becomes "one checkpoint at a time".

**Proposed position.**

- `thinning-checkpoints` keeps, whatever their age: the save before a restore until you set it
  aside, a checkpoint an open alert points to, and a checkpoint whose comparison is not written yet.
  All three drafts ask for one of these, and none conflicts with another.
- Whether the full window is 2 hours or 24 hours is for you. 24 hours costs only the blocks changed
  in a day, and it is what keeps the night's checkpoints whole until the morning.
- `checkpoint-per-system` gets a replacing decision, for you: a checkpoint is taken from Silo's own
  record of the blocks written where Silo sees them, and by a clone where it does not (#365). It
  waits on #359's tests 31 and 32, since nobody has booted macOS from an NBD disk yet. Until they
  pass, `checkpoint-at-each-turn` waits for tests 03 and 15, and if a clone stalls main's writes on
  APFS as it does on XFS, a checkpoint at each turn needs #352's minimum gap.
- `thinning-checkpoints` names which checkpoint stays for an hour or a day: the first (#365, so the
  result does not depend on when thinning runs). It also faces #365's disk case: 30 days of dailies
  can outgrow the default limit within a week or two, so the Timeline shows how far back the kept
  checkpoints reach, and an alert says when the limit removes one younger than 30 days.
- `checkpoint-storage` adds #365's floor of free space and its reserve file.
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
in `order.json`. #360's doubt (no way to force the device) becomes the first thing Mac test 25
checks; #361 names the test and #359's update writes it, so #359 merges before #361. `sizing-target` gains a third load from #361: a few booted simulators shared by many agents,
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
  and #354 quotes an earlier version where it was 5. #361 names Mac tests 23 to 30, and #359's
  update writes them under those numbers in the kit, with 31 to 33 for #365. Use the kit's numbers
  everywhere.
- **SSH keys.** #347 (notes) and #356 (`ssh-keys`) agree: an SSH agent outside the computer, bound to
  the hosts allowed. #356 adds the rewrite of Git remotes to HTTPS and the check that a signature is
  for a connection Silo saw. #347's trial used OpenSSH's own destination limits (`ssh-add -h`). Both
  find some clients cannot use such a key (#347: libssh; #356: Paramiko, JGit, OpenSSH before 8.9).
  No conflict; #347's note belongs under `ssh-keys` once it exists.
- **Duplicate notes.** #347 wrote notes for `fork-and-sign-in` in two places, one beside no entry.
  Its update moved them into `plan/agent-programs/decisions/fork-and-sign-in.md`; #347 now passes
  `plan check` on its own.
- **Anthropic's Usage Policy.** #347 and #368 quote the same clause of 12 November 2026 ("Resell,
  proxy, or otherwise provide access to Claude through unauthorized means") and draw the same
  conclusion: it is the clause to ask Anthropic about in `anthropic-reads-intermediate`. #347's
  notes there and in `terms-on-changing-a-reply`, and #368's in `provider-terms-line`, say it three
  times. Once merged, the quote stays in `terms-on-changing-a-reply` and the others point to it.

### 16. A program's sign-in held outside the computer

Entries: `logins`, `fork-and-sign-in`, `provider-terms-line`, `secret-placeholder`.

| Draft | Says |
|---|---|
| #363 | Built a login vault in the boundary. The program signs in its own way; the vault keeps the real tokens from the provider's reply and gives the program placeholders of the same shape, swaps them back on the way out and answers refreshes itself. Against fake providers that rotate refresh tokens, with the real Claude Code and Codex: a copied login file got 401, and two computers shared one login through three expiries. Passed through, as `logins` says today, a copied file worked from anywhere, and a fork signed both computers out at its first refresh. It notes that the vault does what Anthropic's terms forbid for a plan, and offers a partial guard: refuse a request to any other read host that carries the login's value |
| #347, update | The same for an MCP server's sign-in: copied into a fork, the fork's renewal signed main out; held outside, both stayed connected. A sign-in held outside must renew once for every computer that uses it, or two renewals at once sign one computer out |
| #368 | Holding a plan's sign-in outside the computer and swapping it in is forbidden by Anthropic's words ("collect, store, or intermediate") and by Google's. OpenAI's Codex page tells users to copy `~/.codex/auth.json`, and its terms allow storage "local and under the user's control". So `logins` stays as it is until OpenAI says yes in writing |
| #358 | Ranks the stolen login seventh of 21 attacks, and proposes a hash of it in the record |
| #369 | Where every other agent program keeps its login, in `logins`' notes |

They agree on the facts. They differ on what to build: #363 argues for the vault, #368 for
`logins` as it is.

**Proposed position.** Build #363's vault, and use it where the terms allow: an MCP server's
sign-in (#347's case, which a fork needs), an API key, and a provider whose terms allow it in
writing. A plan's sign-in from Anthropic or Google stays in the computer, as `logins` says, with
#363's partial guard and #358's hash: the boundary knows the login's value from the requests it
reads, and refuses it, as a fact you see, on any other host it reads. Every sign-in the vault holds
renews once for all the computers that use it (#347).

Why: the vault is the only fix measured for a stolen login and for a fork that signs main out, and
#368 shows where it stops. Using it on a plan's sign-in would trade a stolen login for a suspended
account.

### 17. Programs whose traffic Silo does not read

Entries: `program-that-cannot-be-read`, `vendor-runs-the-model-calls` (new in #367 and #369),
`google-sign-in-unread` and `silos-own-model-calls` (new in #368), `subscription-and-key-alike`.

#369 leaves unread the model traffic of a program whose vendor's server calls the model in a
protocol of its own (Cursor, Windsurf, Amp, Kiro), and reads Copilot, whose server speaks the
providers' formats. #368 leaves unread Gemini CLI and Antigravity signed in with a Google account,
for the risk to the account. Both send their case to `program-that-cannot-be-read`: allowed or
denied by the rules, never read, left out of spend and budgets, and said in the app. They agree.
One gap between them: #369 says "under a budget, its traffic is denied", and #368 does not say. **Proposed position:** #369's sentence
holds for both, written once in `program-that-cannot-be-read` instead of in each new entry.

#368's `silos-own-model-calls` (a call Silo or an extension makes itself uses a key you give for
it, never a plan's sign-in) agrees with #347's reading of the terms and conflicts with no draft.


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
| A request the boundary cannot read whole is refused, not forwarded | #358 (attack A3), #363 (a strict reader in Zig refused 46 of 46 hostile requests and read all 49 real ones) | `forwarded-byte-for-byte`, with #358's `boundary-reads-whole` |
| Each request on a connection Silo ends names the host of its handshake | #363, #356 (the handshake names the host Silo gave) | `ending-the-encrypted-connection` |
| A preview of a computer's port is a path out through your browser; each port gets its own origin and opens in a page of Silo's | #363 (26 ways out at localhost, 0 in a browser Silo starts) | `ports-reached-from-your-mac`, `paths-out-are-shown` |
| A program's own processes fail in a denial's window too; a program that stops on it fails its check | #369 | `own-processes-in-a-denial` |
| A Linux host needs `/dev/kvm`, Btrfs or XFS, and memory snapshots for running forks | #366, #365 | `linux-server-host`, `linux-vm-engine` |

## Merge conflicts and an order to merge in

Every draft was merged with every other on a copy of `main` (`1b8f191`), at the heads they had in
the evening of 24 November, and `plan check` was run on the result. The research-only drafts (#350,
#351, #352, #354, #355, #358, #359, #363, #365, #366) conflict with nothing: where several add notes
to one entry (`network-stack`, `checkpoint-per-system`, `logins`, `hold-limit-length`,
`how-a-hold-ends`), each adds its own section at the end, and the sections stack.

| Pair | Conflicting files | How to resolve |
|---|---|---|
| #347 and #356 | `secret-placeholder.md` | Keep both sections; then section 3 |
| #347 and #361 | `package-age-rule.md`, `registry-traffic-read.md`, `secret-placeholder.md`, `computer-image.md` | Keep both sections in each; all are added notes |
| #356 and #361 | `secret-placeholder.md`, `order.json` | Keep both; in `order.json`, `ssh-keys`, `exchanged-tokens`, `cloud-credentials`, `signing-keys-re-signed` and `apple-keys-in-a-computer` all follow `secret-placeholder` |
| #360 and #361 | `main-on-macos-first.md` | Keep both sections. They also add the same id in two folders, which `plan check` refuses: section 11 |
| #367 and #369 | none | #369 contains #367's one commit. Merging #369 merges #367 |
| #368 and #369 | `order.json` | Both insert into the traffic block a few lines apart. Keep both |
| #348 and #360, #361, #362, #369 | `order.json` | #348 moves the entries they insert next to. Re-add their ids as below |

What fails `plan check` after a clean merge: only #360 with #361, the id
`simulators-for-parallel-agents` twice (section 11). #347's orphan notes are fixed in its update.

**The order proposed.**

1. Before anything merges: #352 renames `research/first-build-decisions/` to
   `research/2026-11-24-first-build-decisions/`, the name #354 and #359 already link to and the
   project's files use; #360 drops its `simulators-for-parallel-agents` in favour of #361's (section
   11); #367 is closed, since #369 holds all of it.
2. The research drafts, in any order: #350, #351, #352, #354, #355, #358, #359, #363, #365, #366.
   #359 before #361, whose notes name its tests 23 to 30.
3. #348. It holds the first build, which is what you rule on first, and it rewrites `order.json`.
4. #347, then #353, then #356. #356 conflicts only with #347's notes.
5. #368, then #369, each re-adding its ids to `order.json` after #348:
   - #368: `google-sign-in-unread` and `silos-own-model-calls` after `subscription-and-key-alike`.
   - #369: `vendor-runs-the-model-calls` after `program-that-cannot-be-read`;
     `own-processes-in-a-denial` after `denial-per-program`.
6. #360, #361 and #362, each re-adding its ids to `order.json` after #348:
   - #360: `what-silo-sends-home` after `nothing-to-set-up`.
   - #361: `simulators-for-parallel-agents` after `silo-ships-recipes`; `xcode-in-a-computer`
     beside it; `builds-on-your-iphone` after `asks-are-native`; `signing-keys-re-signed` and
     `apple-keys-in-a-computer` after `secret-placeholder`.
   - #362: `record-edits-show` after `record-file-format` and `person-in-a-control-fact` after
     `who-made-each-change`, both now in #348's first-build block; its fourteen team entries after
     `forwarding-tool`.

This was run on a copy: all the drafts merged in this order, with the fixes of step 1 and the ids
placed as above, pass `plan check`.

## Why this pull request adds no notes to the plan's entries

Every entry in conflict above already has its notes edited by at least one of the open drafts. A
note added here would be one more conflict to resolve in each of them. Once the drafts are merged,
each section's "Proposed position" can be written into the notes of the entries it names, or into
the replacing decision it proposes, in one pass.
