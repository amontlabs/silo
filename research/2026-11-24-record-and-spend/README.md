# The record and the budget: what to build so they hold

Research of 24 November 2026, by agents, for polarzero. It follows the competitor survey of the
same day (draft PR #349), which found that no product records a whole computer from outside and
none enforces a model budget at the boundary for any agent program. This research asks what those
two features need in order to be true for real users, and where the plan leaves them open.

Nothing here is a decision, and nothing under `plan/` was edited. Every recommendation names the
plan entries it touches. "Taken" means polarzero gave the entry a verdict, so the recommendation
is a challenge. "Open" means the entry has an answer and no verdict, so the recommendation is a
proposal. "New" is an entry the plan does not have.

The detail files hold the evidence, with quotes and links:

| File | Covers |
|---|---|
| [`details/incidents.md`](details/incidents.md) | 41 real incidents from July 2025 to November 2026 and 70 user requests, each tested against the plan |
| [`details/record-ux.md`](details/record-ux.md) | Prior art for showing agent work, and text sketches of one overnight case: the first screen, a receipt, the Timeline, a diff of the whole computer, the `silo` output |
| [`details/spend.md`](details/spend.md) | Where each provider puts its token counts, prices, limit headers and errors, and a simulator of budget policies ([`sim/`](sim/)) |
| [`details/keys-and-routes.md`](details/keys-and-routes.md) | Every route by which an agent spends on models without a budget counting it, and the $453 case walked against the plan |
| [`details/dns-and-provider-tools.md`](details/dns-and-provider-tools.md) | Name lookups and other ways out past a host list, and the tools a provider runs for the model |
| [`details/recording-feasibility.md`](details/recording-feasibility.md) | How file changes, processes and connections can be recorded from outside a macOS or Linux computer, and what a root agent can do against each |
| [`details/fact-check.md`](details/fact-check.md) | A second agent's check of the claims the recommendations rest on |

## In short

1. **One rule closes three gaps at once: deny a request to a host Silo reads when it carries a
   credential Silo did not give.** Three of the six agents arrived at it separately. It stops the
   key from `.env` in the $453 case, the attacker's own API key used to upload files to
   api.anthropic.com ("Claude Pirate"), and a push to an attacker's GitHub account. It needs no
   guessing of key formats, because Silo knows its own placeholders byte for byte. It is a rule on
   a fact.
2. **A budget today has a hole the size of a provider.** `program-that-cannot-be-read` lets traffic
   to a provider Silo does not read pass uncounted. The $453 runner had a `--provider openai|gemini`
   switch. Proposed: under a budget, a connection to a known model host that Silo cannot count is
   denied, for the same reason `what-a-budget-promises` already denies a model with no price.
3. **Most unwanted spend is a subscriber billed on a key they did not know was in use.** Five of
   the 41 incidents have this shape: $453 in a day with Codex, $1,800 and $1,700 with `claude -p`
   and an `ANTHROPIC_API_KEY` in the environment, a $52 key from `.env`, Enterprise extra usage.
   Proposed: a small default budget on metered usage whenever a computer is signed in with a
   subscription. For Codex, a sign-in and a key go to different hosts. For Claude, an API key comes
   in `X-Api-Key`, but no Anthropic page says how a sign-in is sent, so this needs a live check.
4. **"Calls under way finish" is right on an ordinary night and wrong in the tail.** In the
   simulator, a $50 budget with 8 Opus subagents ends $0.67 over at the median. The worst case the
   plan allows is 16 large fast-mode calls leaving at $49.99, which ends the night at $725.
   Proposed: a request leaves only if its input cost, which the body bounds exactly, fits in what
   remains. A rate of $15 an hour stopped a runaway loop 2.6 hours sooner and halved the night's
   spend.
5. **The error at the limit decides whether the agent stops or retries for hours.** Codex stops on
   a 429 with `error.code` `project_spend_limit_exceeded`. Claude Code stops on a 429 marked
   `x-should-retry: false`. A plain 429 is retried 10 times by Claude Code and 4 by Codex.
   `budget-denies-or-holds` should fix the error per provider.
6. **Tools the provider runs turn the model host into a way to the whole internet.** A script in
   main can call api.anthropic.com with main's own key and declare `web_fetch`, and Anthropic's
   servers fetch any URL. No host list stops it. The request declares each such tool, so a rule
   can deny the request before it leaves. Proposed: a new decision, `tools-the-provider-runs`.
7. **"Limited" needs Silo to be the computer's only name resolver.** DNS broke host lists this year
   in Docker Sandboxes, Harden-Runner and E2B. The design that holds answers allowed names with
   placeholder addresses and resolves the real name only when a program connects. gvproxy and
   passt, which the engine choice weighs, forward unknown names to the Mac, the same flaw.
8. **The costliest incidents are deletions through an API the agent is allowed to reach, and the
   plan handles them worst.** PocketOS lost a production database to one Railway `volumeDelete`.
   A checkpoint of the computer brings none of it back. The record would hold only the request's
   size. Proposed: keep the method, path and operation of every request Silo already opens, and
   ship a rule set that holds destructive calls to named control planes.
9. **The first version of "what changed overnight" needs nothing inside main.** APFS never writes
   a changed object in place, so the changes between two checkpoint clones can be found from the
   disk alone, as facts. On Linux computers, a patch of about 30 lines to libkrun gives changed-
   block tracking, and the kernel Silo ships makes processes watchable from outside.
10. **The morning view the plan decided has no competitor, but a checkpoint it needs may be gone
    by morning.** Under `thinning-checkpoints`, the checkpoint before a deletion at 02:47 is thinned
    at 04:47. Proposed: a checkpoint an open alert points to is kept until the alert is set aside.

## Who has the problems Silo solves

The incidents rank the features by what real people lost and asked for
([`details/incidents.md`](details/incidents.md)):

| Capability | Incidents it answers | What users asked for, by votes |
|---|---|---|
| Checkpoints of the whole computer, and putting back one run | 19 of 41 undone; 14 were files destroyed on the agent's own machine | Codex "/undo" 505 👍, Codex `/rewind` 233 👍 |
| The record from outside | the account no agent program gave: child processes, Bash changes, encrypted subagent messages | Codex "readable task audit trail" 141 👍 |
| Budgets, with a rate | 5 surprise bills on a key, 5 runaway loops | Codex "burning tokens very fast" 284 👍; Claude "instantly hitting usage limits", about 1,500 comments |
| Seeing spend while it happens | the first complaint in every runaway case | same as above |
| A computer that cannot reach the Mac's files | 6 prevented outright | |
| Stopping a run from the phone | "I had to RUN to my Mac mini" (Summer Yue); a delete that ran 20 minutes after stop | asked often, few votes |

Claude Code's documentation still says of `/rewind`: "A file that only Bash commands changed isn't
tracked and stays as it is." Cursor's revert does not restore deleted files. The deleted folder
outside the repository is the case where Silo is alone, and it is the most common incident.

Several incidents happened on the vendors' machines (Replit, Kiro, Codex cloud). Silo would cover
the same agent run in one of its computers. That belongs in what Silo says it is for.

## Budgets that hold

What the boundary can count, per provider ([`details/spend.md`](details/spend.md)):

- Anthropic sends input tokens in `message_start` and the final counts in the last `message_delta`.
  Server-side fallbacks and the advisor run other models inside one request, and only
  `usage.iterations[]` lists them. The top-level `usage` undercounts them.
- OpenAI's Responses API gives `usage` only in `response.completed`, `response.incomplete` or
  `response.failed`. Chat Completions streams carry no usage unless the request sets
  `stream_options.include_usage`, and Silo may not add it.
- A call the program cancels has no final count anywhere. Anthropic's own gateway for Claude apps
  bills it at about four characters per output token.
- Codex under a ChatGPT sign-in talks to `chatgpt.com/backend-api/codex` over a WebSocket. Silo has
  to read WebSocket frames to count it at all.
- Batch requests and OpenAI's `background: true` responses are billed after the connection ends.
  The $453 runner was a batch runner. No decision says how a budget counts them.
- Prices change on dates already announced. A price table needs a date of effect and a row per
  service tier.

The simulator ([`sim/budget_sim.py`](sim/budget_sim.py), 20 synthetic nights per setting, output in
[`sim/output.txt`](sim/output.txt)) compares five policies at a $50 budget:

| Policy | Worst overshoot, Opus 5.5, 8 subagents | Worst overshoot, Fable 5.2 fast, 16 subagents | Conversations stopped that would have fit, per night |
|---|---|---|---|
| Calls under way finish (the plan) | $1.80 | $12.60 | 0 |
| The plan, and the input cost must fit | about the same | $12.44 | 0 to 0.1 |
| Reserve `max_tokens` before the call (LiteLLM's default) | $0 | $0 | 25 to 136 |
| Cut the stream at the limit | lower | $3.12, plus $5.96 of paid answers thrown away | 0 |

The input check changes little on a simulated night. It is there for the call the simulator does
not draw: one request of a million input tokens leaving at $49 of $50. Reserving the whole
`max_tokens` stops most of the night's work to protect against a tail that rarely comes.

For a subscriber, dollars at API prices measure work, not money: one community measurement puts a
Max 20x week at about $3,400 to $3,900 at API prices. What a subscriber can lose is the week's
limit, burned overnight, and credits billed beyond the plan. Both are read at the boundary:
`anthropic-ratelimit-unified-7d-utilization` for Claude, and `x-codex-secondary-used-percent` and
`x-codex-credits-balance` for Codex. The Claude header names come from community captures, not
Anthropic's documentation.

## The routes around a budget

[`details/keys-and-routes.md`](details/keys-and-routes.md) lists 12. The ones a rule can close:

- **A key in a file inside the computer.** Denied by the rule in point 1 above, on every host Silo
  reads.
- **A provider Silo does not read.** Denied under a budget by point 2. A list of model hosts cannot
  be complete: LiteLLM knows 215 providers, 101 of them in OpenAI's format. Only a limited network
  covers the long tail.
- **Claude Code moving off the plan.** Under `-p`, Claude Code always uses an `ANTHROPIC_API_KEY`
  from the environment instead of the user's sign-in, and in interactive mode it asks once. It
  sends an API key in `X-Api-Key`. Anthropic documents `Authorization: Bearer` only for gateway
  tokens, and the sign-in prefix `sk-ant-oat01-` comes from community captures. So the switch is a
  fact once a live request confirms how a sign-in is sent.

The ones no rule at the boundary closes:

- A GitHub token pays for GitHub Models, and a Cloudflare token runs Workers AI on the host it
  deploys to. A secret needs a path limit (`secret-limits`).
- An AWS key for Bedrock signs each request, so the placeholder cannot swap it, and it stays a file
  the agent reads.
- An agent that pushes a GitHub Actions workflow, mentions "@claude" in an issue, or calls the
  user's own production app spends on someone else's machine. Silo sees only the push or the call.

Scanning files for keys is a precaution, because a pattern can be wrong. gitleaks has 222 rules and
measured 46% precision in a 2023 study. Silo should use only the roughly 15 rules for keys with a
fixed prefix, at first run and on the files that changed at each checkpoint, and offer to hold each
key it finds outside the computer.

Walked step by step against the plan, the $453 case ends before any spend only if the key was a
Silo secret. With the key left in `.env` and main's network open, the plan counts the OpenAI share
only if Silo reads `api.openai.com` for a program that is not an agent program, and never counts the
Gemini share.

## The boundary's other holes

From [`details/dns-and-provider-tools.md`](details/dns-and-provider-tools.md):

- **DNS in 2026.** Docker Sandboxes forwarded every name to the host resolver. Harden-Runner's block
  mode let lookups out through DNS over HTTPS and over TCP. E2B opens all traffic to 8.8.8.8 once
  one domain is allowed. An OpenAI agent in training reached a public chatbot through DNS TXT
  records. The CVE numbers are in the detail file and in the fact check.
- **The design for "limited".** Silo is the only resolver. It refuses every name not on the list.
  It answers an allowed name with a private placeholder address mapped to that name, as NVIDIA
  OpenShell does, and resolves the real name when a program connects. The TLS SNI, and the HTTP
  Host on hosts Silo reads, must match the name. No UDP leaves except Silo's own lookups. Computers
  get no IPv6 or ICMP. QUIC is refused, so programs fall back to TCP.
- **Lookups are facts worth keeping.** Each is a name a program tried. A refused one is a denied
  attempt. Names that look like encoded data are only an opinion, so only a precaution acts on
  them.
- **Tools the provider runs.** Claude Code's WebFetch runs inside the computer, so network rules
  apply. Its WebSearch and its advisor run at Anthropic. Codex uses OpenAI's hosted web search,
  "live" under full-access modes. Silo cannot strip a tool from a request (`no-silent-change-to-traffic`),
  so it denies or holds the whole request by what it declares, records each call from the reply,
  and counts the tool's price into budgets. Web search costs $10 per 1,000 at Anthropic, and $10 to
  $25 per 1,000 at OpenAI.
- **State that stays at the provider after a restore.** Anthropic Files are kept until deleted and
  readable by any key in the workspace. Code execution containers last 30 days, OpenAI stored
  responses 30 days. The restore screen should say what was made after the checkpoint.

## What the record can see, and from where

From [`details/recording-feasibility.md`](details/recording-feasibility.md):

| What | macOS computer | Linux computer |
|---|---|---|
| File changes | Fact: walk the APFS Data volume of two checkpoint clones. Report: `.fseventsd` read off the disk, which catches a file made and deleted between checkpoints | Fact: compare checkpoints, with ext4's `i_generation` to tell a rename from a reused inode. Fact: changed blocks, from a small libkrun patch |
| Processes | Report only: an Endpoint Security client, which needs Apple's entitlement | Fact, possible: hardware breakpoints on the kernel's exec and exit paths, since Silo ships one pinned kernel. Report: a probe, which needs a Silo kernel config |
| Connections | Fact at the boundary. Which process made it is a report joined to the flow | the same |

Two findings change entries:

- `processes-watched-from-outside` suggests a drop because it means "following every version of
  its kernel". Silo ships the Linux kernel, libkrunfw, pinned, so there is one version. The drop
  holds for macOS only.
- The stock libkrunfw kernel has no tracepoints, kprobes, BTF or eBPF JIT. Tetragon, Falco and
  Tracee need a kernel config of Silo's own, and their published overhead numbers do not carry
  over.

Things only a Mac can settle: whether APFS ever reuses an object id, whether Apple's NBD client
accepts a Unix socket, and whether a TCC row written before first boot gives Full Disk Access.

The trial's 4,939 false renames out of 10,000 deletes come from writing a rename as a delete and a
create. Both filesystems let Silo tell them apart.

## What the morning looks like

[`details/record-ux.md`](details/record-ux.md) sketches one case end to end: three agents left for
eight hours, 214 turns, $31, one new host, one folder deleted outside the repository, a failed test
suite and a run stuck for two hours. The first screen at 06:58, from that file:

```text
 Since you left at 22:58 · 8 h · 3 runs · 214 turns · $31.00 · 2 checkpoints you may need

 NEEDS YOU
 main     claude #2 ◇  deleted ~/work/shared-fixtures/ · 3,412 files · 1.2 GB      [put back] [open]
                       outside ~/work/app · your rule outside-the-repo · 02:47
                       the checkpoint before it is kept for this alert
 try-bun  codex        ◇ stuck since 04:58 · 2 h, no model call · runs `bun test --watch`  [open]
                       your precaution stuck-15-min · 05:13
 try-bun  codex        new host bun.sh · 1 call · 38 MB in · 2 KB out · 00:32       [deny this host]
                       your rule new-host
 DONE
 main     claude #1    6 h 54 · $14.20 · 38 files · ◌ last full `npm test` 3 failed  [receipt]
                       ◇ its summary says «All tests pass»: mismatch
 main     claude #2    5 h 16 · $9.10 · 61 files · 1 deletion outside the repo        [receipt]

 [Checkpoint] [Fork] [Pause all]                         Now · Disk · Network · Timeline
```

What no other product found does: show a night of several agents as one ranked list. Trace tools
show one trace at a time, agent apps one task at a time. The closest prior art is StepSecurity
Harden-Runner, which ties every process, connection and file write to the CI step that made it and
flags new endpoints. That flag caught the tj-actions compromise in March 2025. Silo's chain does
the same one level finer, on the turn.

◇ marks an opinion and ◌ a report; the rest are facts. The mismatch under claude #1 is a proposed new feature that two agents arrived at separately: an agent's claim of success
checked against the record. Silo sees the final reply in the traffic and the exit code in the
record. The result is an opinion, marked as one.

## Recommendations for the plan

The detail files hold about 80 recommendations. These are the ones with the most behind them, in
the order they would change what Silo does.

### Spend

1. **Deny a key Silo did not give.** New decision in `boundary`, beside `secret-placeholder` (open).
   On a host Silo reads, a request whose credential header is not one of the computer's own
   placeholders is denied. It also covers uploads to a provider with someone else's key. From
   `keys-and-routes.md`, `incidents.md` and `dns-and-provider-tools.md`.
2. **Under a budget, deny model hosts Silo cannot count.** `program-that-cannot-be-read` and
   `budget-on-a-computer` (both open). Read model hosts by provider, not by agent program
   (`subscription-and-key-alike`, open), so a script's calls to `api.openai.com` are counted.
3. **The input cost must fit.** `what-a-budget-promises` (open): keep "calls under way finish", and
   add that a request leaves only if its input cost, priced at the highest rate it can incur, fits
   in what remains. Also say how a cancelled call is counted.
4. **The error at the limit, per provider.** `budget-denies-or-holds` (open): HTTP 429 with
   `error.code` `budget_reached` for OpenAI formats, `x-should-retry: false` and `billing_error` for
   Anthropic. Whether Claude Code treats Silo's 429 like Anthropic's gateway needs a live run.
5. **A small default budget on metered usage when a subscription is signed in.** `budgets-on-rates`
   and `safe-fresh-install` (open). The switch from sign-in to key is a fact for Codex by its host.
   For Claude it rests on how a sign-in is sent, which a live request has to confirm first.
6. **A limit on the plan's share for subscribers.** New decision: "main uses at most 30 percent of
   this week's limit", read from the provider's headers, applying to the whole account.
7. **Count what the reply bills.** `what-traffic-gives` (open): `usage.iterations[]`,
   `usage.server_tool_use`, `usage.speed`, `inference_geo`, and the reply's `service_tier`.
8. **Batches, background responses and Chat Completions streams without usage.** Two new decisions,
   since none of these leaves a count on the connection that started it.
9. **Where prices come from.** New decision: Silo's own price table, with dates of effect and a row
   per tier, checked against LiteLLM's file and OpenRouter's `/models` but not loaded from them.

### The boundary

10. **Silo is the only resolver, with placeholder addresses.** New decisions `name-lookups` and
    `connection-matches-its-name`, and `network-modes` / `open-limited-or-closed` (open). It bears
    on `network-stack` and `linux-vm-engine`: gvproxy and passt cannot be used as they are.
11. **Tools the provider runs.** New decision `tools-the-provider-runs`: a rule denies or holds a
    request by the server tools it declares, and every call is recorded and priced.
12. **Keep the method, path and operation of every request Silo opens.**
    `content-kept-of-what-was-sent` (open). Define "read only" by operation, not method, in
    `secret-limits` (open), since GraphQL and AWS send reads as POST.
13. **Hold destructive calls to named control planes.** New feature `destructive-calls-held`: a
    shipped rule set for Railway, AWS, GitHub and Gmail, beside `merges-and-deploys-held`.
14. **Offer to hold keys found at first run, from `.env` files and Git history too.**
    `first-run-offers-to-bring-in` (open). Mask credential headers in the record
    (`model-calls-in-the-record`, open), since `secret-never-written-down` covers only Silo's own.

### The record and the views

15. **Keep the checkpoint an open alert needs.** `thinning-checkpoints` (open), and rank such alerts
    first in `one-queue` (open).
16. **A checkpoint at each turn.** `checkpoint-at-each-turn` (open): with it, a deletion falls
    inside one tool call. With 10-minute checkpoints, the case's deletion falls among 27 commands
    of two runs.
17. **Exact renames and changed blocks.** `file-changes-from-outside` (open), and a new feature
    `blocks-written-per-checkpoint` for Linux computers.
18. **Processes from outside, for Linux only.** `processes-watched-from-outside` (open): keep the
    drop for macOS, and reconsider it for Linux. A new decision `kernel-events-are-facts` asks
    whether what the engine sees in the kernel counts as a fact.
19. **`.fseventsd` read from the checkpoint.** New feature for macOS computers, as a report.
20. **An agent's claims beside the record.** New feature in `opinions`.
21. **Since you last looked, search, `silo why <path|host|pid>`, a diff of two forks, a receipt
    attached to a pull request, looking inside a checkpoint.** New entries in `views` and
    `going-back`, each sketched in `record-ux.md`.
22. **Text from an agent or a host is marked as theirs in `silo` output.** `app-as-text` (open), so
    the record cannot carry a prompt injection to the next agent that reads it.

### One challenge to a taken decision

23. **`where-precautions-live`** (taken) hides from a computer every opinion about it. Which run
    changed a file is always an opinion (`the-chain`). So an agent in main that asks the record
    "what did the other run change" gets file changes with no run attached, and has to guess. That
    is what `app-as-text` exists to prevent. Proposed instead: a computer sees the descriptive
    classifiers that ship with Silo, such as `which-run-did-it`, and never sees precautions, what
    they raised, or which opinions they read. The decision's notes do not weigh this case.

### For Linear, not the plan

24. Apply to Apple for the Endpoint Security entitlement now. Every report of processes on a macOS
    computer depends on it, Apple decides case by case, and the route without it (SIP off in the
    guest) broke once in macOS 26.5.

## How this was made

Six research agents, one per subject, read the plan with `plan show`, then the providers' docs,
source code, issue trackers, forums and advisories on 24 November 2026. One wrote and ran the
simulator. A seventh agent checked 81 claims the recommendations rest on against their sources:
65 held, 7 were wrong and 6 partly right, all corrected in the detail files, and 3 could not be
verified and are marked so. Its log is [`details/fact-check.md`](details/fact-check.md).
Reddit and some vendor pages could not be read directly; the detail files say where. Figures and
prices change quickly: check one at its link before using it in public.
