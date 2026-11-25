# G5. Secrets Silo never held

For polarzero. An agent's research, 24 November 2026. It answers gap G5 of draft PR #350
(`research/2026-11-24-real-pains/README.md`): a token you paste into a chat, type into a `curl`
command, find already committed in a repository, or see printed by a tool. Silo never held it as a
secret, so `secret-never-written-down` says nothing about it, and today it would end up in three
places: the computer, Silo's record (`conversation-text-kept`), and the model provider.

Code is under `research/2026-11-24-gap-fixes/prototypes/leaked-secrets/`. Every number below was
measured in this container and says what ran.

## Summary

| Question | Recommendation | How sure |
|---|---|---|
| How Silo recognises a credential | By exact formats only for anything that acts: a checksum the issuer built in (GitHub, npm), a private key that parses, a JWT whose parts decode, or a fixed documented prefix (`sk_live_`, `sk-ant-api03-`, `AKIA`, `xoxb-`). Entropy only marks a line and never acts. | Sure. On 606 MB of `node_modules` and 140 MB of this repository's old Git history, the exact rules raised 4 and 5 distinct matches, all vendor examples or test values. The entropy check raised 45 and 13, all wrong. |
| Where | Inline at the boundary, on each model request, but only on the messages that are new in it. Silo already splits requests into messages by hash (`store-messages-once`). | Sure. In a real Claude Code session of 16 requests, the conversation text was sent 9.5 times over. Scanning only the new messages took 2.7 ms in Python for the whole session. Scanning every body took 41.8 ms. |
| Before the provider gets it | The first model request that carries a credential matching an exact rule is held, with an ask. The ask has three answers: send it as it is, send it with a placeholder and make it a Silo secret, or do not send it. | Fairly sure. Claude Code 2.1.331 waited through a 150-second hold and went on normally. Two decisions in force have to change for the placeholder answer, so that part is a challenge below. |
| The record | The value is not kept in the stored message text, on any exact rule. The record keeps a marker and a keyed fingerprint instead. An entropy match only marks the line. | Sure. Not keeping a value is destructive (`kinds-of-other-actions`), so only a rule or you may do it. A checksum is exact enough to be a rule's condition, and entropy is not. |
| Rotation | An alert with the provider's page for revoking the key. For GitHub tokens, a button that calls GitHub's `POST /credentials/revoke`, pressed only by you. | Sure about the alert. The button is cheap, but it cannot be undone, so it is yours to press. |
| Committed secrets | The first run scans each repository it brings in, with its whole history, using the exact rules only. It lists what it found and changes nothing. | Sure, and cheap: 140 MB of history took 3.3 s in Python. |

Decisions this research challenges, each with its case further down:

- `what-silo-changes-in-traffic` and `forwarded-byte-for-byte`, for the placeholder answer.
- `how-a-hold-ends`, which says a 403 is not retried. Claude Code 2.1.331 retried one twice.
- `store-messages-once`, whose plain hash of a message lets anyone with the record test guesses
  of a short password.

## The case

In November 2026 Apiiro found real secrets in 1,122 of 104,185 public `.claude/settings.local.json`
files ([HN, 629 points](https://news.ycombinator.com/item?id=50407882); figures from
[InfoWorld](https://www.infoworld.com/article/4247344/claude-codes-dont-ask-again-button-has-a-secrets-spilling-problem.html)
and [RuntimeWire](https://runtimewire.com/article/claude-code-dont-ask-again-secrets-public-github-settings-files),
since Apiiro's own page did not render for the earlier research). How it happens: the model writes
`curl -H "Authorization: Bearer sk-..."`, you press "Don't Ask Again", and Claude Code saves the
command, token included, as an allow rule in a file that gets committed and pushed.

A second case shows how much: one developer found about 600 secrets in his own Claude Code
transcripts ([dev.to](https://dev.to/rentierdigital/i-spent-25-years-avoiding-malware-claude-code-stored-600-of-my-secrets-anyway-3e2j)).
He counted 95 GitHub OAuth tokens, 94 GitHub fine-grained tokens, 103 Google API keys, 197
strings shaped like JWTs, 45 AWS access keys and 7 Anthropic sign-in tokens. They came in through
command output, pasted text, file reads, file writes and the model repeating them.

Inside Silo, the token in the first case would sit in the conversation, so it would be in every
model request after that one, in the record's stored message text, and in the file the agent writes.
`secret-entry` already says the plain thing: "A value pasted into a chat or a file inside a
computer is not a secret: the computer can read it." This design does not change that. It decides
what Silo does once it sees such a value.

### What Claude Code does today, measured

`rig/run_cc.sh` runs the real Claude Code 2.1.331 against the fake Anthropic API of
`research/2026-11-24-experiments`, in a clone of this repository. The prompt holds a made-up GitHub
token with a valid checksum. A `.env` holds a made-up Stripe live key and a database URL, and the
scripted model runs 15 commands, including `cat .env` and a `curl` that carries the token.

`rig/analyse.py` printed:

```
credentials found (rule, fingerprint, first request that carried it):
  github-token         c5c9bf7a936a  request 0
  stripe-live-key      3d0feedea098  request 4
  database-url         d417507f4f0f  request 4

Claude Code's files under its config dir that hold a found value verbatim:
  home/.claude/projects/-work-g5repo/7cfcf5a0-….jsonl: 3 of 3 values
```

Claude Code redacts nothing. The token is in the very first request body, and every later one.
Its own transcript on the computer's disk holds all three values. A member of the Claude Code team
said as much: "Historically Claude Code has not done secret scrubbing" (quoted in the evidence of
PR #350).

## Where Silo sees the text

| Where | What Silo has there | Timing | Used for |
|---|---|---|---|
| Model requests at the boundary | Every message, decoded from the provider's JSON, already split by hash (`store-messages-once`, `what-traffic-gives`) | Before the request leaves: the request is still waiting | The hold, the record, the alert |
| Model replies | The model's text and tool calls, as they stream | After the provider already has the value | The record and the alert only. Replies pass unchanged (`what-silo-changes-in-traffic`) |
| The disk | The files that changed between two checkpoints, read from outside | After the write, before a push leaves when the push comes later | An alert on a file Git tracks, such as `.claude/settings.local.json` |
| Other requests Silo opens | Headers and bodies toward hosts Silo already opens for a secret (`ending-the-encrypted-connection`) | Before the request leaves | A hint that a value is live: it was sent as a credential |
| Repositories on your Mac at first run | Working tree and history (`first-run-offers-to-bring-in`) | Before anything is copied into main | A list for you |

Command lines from the probe are left out on purpose. A command is a report (`what-a-report-carries`),
and the same command already reaches Silo as a fact: the model's tool call is in the reply, and its
output is in the next request. A rule reads only facts (`rule-reads-only-facts`), and here it loses
nothing by doing so.

Most leaks of this kind pass through the model traffic, because the model is the one that reads
the `.env` and writes the `curl`. That is why the model request is the main place to act. It is
also the one place where Silo can still stop the value before it reaches anyone outside the
computer.

## How a credential is recognised

### Four grades

`detector.py` sorts every rule into one of four grades. The grade says what a match proves.

| Grade | What a match proves | Rules in the prototype |
|---|---|---|
| checked | The bytes pass a check the issuer built into the format. A random string passes with odds of about 1 in 4 billion or less. | GitHub `ghp_` `gho_` `ghu_` `ghs_` `ghr_` (CRC32 of the 30 random characters, in base62, as the last 6), GitHub `github_pat_` (CRC32 in the last 6 characters of the second part), npm `npm_` (same scheme), a private key block that parses as a key, a JWT whose header and payload decode to JSON with `alg` |
| prefix | A fixed prefix, length and alphabet the issuer documents, with no checksum | Anthropic `sk-ant-api03-…AA` and sign-in tokens `sk-ant-oat01-`, OpenAI keys with the `T3BlbkFJ` marker, Stripe `sk_live_` and `sk_test_`, Slack `xoxb-` and webhooks, AWS `AKIA`/`ASIA`, Google `AIza` |
| shape | A URL with a user and a password, which is often a local default | `postgres://`, `mysql://`, `mongodb+srv://`, `redis://` with a password that is not `postgres`, `${VAR}` or a few other defaults |
| entropy | A long, random-looking string near a word such as `token` or `secret` | Shannon entropy at least 4.5 bits per character, 32 characters or more, not plain hex, within 40 characters after a keyword |

The GitHub checksum follows GitHub's own description: "A 32 bit checksum in the last 6 digits of
each token … We start the implementation with a CRC32 algorithm … We then encode the result with a
Base62 implementation, using leading zeros for padding"
([GitHub blog](https://github.blog/engineering/platform-security/behind-githubs-new-authentication-token-formats/)).
npm uses the same scheme: "The last six characters of the tokens consist of CRC32 checksum"
([GitHub blog](https://github.blog/security/announcing-npms-new-access-token-format/)). The
alphabet order (digits, then capitals, then small letters) and the part that is checksummed are
taken from [keyhog](https://docs.rs/crate/keyhog-scanner/latest/source/tests/gap/github_checksum_token_structure.rs).
To check the implementation, 200 tokens made by a separate implementation,
[base62-token](https://www.npmjs.com/package/base62-token) (npm), all passed. The checksum of
`github_pat_` tokens comes from keyhog alone and was not checked against a token GitHub issued,
since that would need a real one. That is open.

### Which grade may act: a rule, a precaution, or neither

`what-a-classifier-is` says a classifier is "whatever gives an opinion: a pattern, a function…".
So the question is whether a match is a rule's condition or a classifier's opinion. The line that
fits the plan is the one `rule-carries-its-data` draws. A rule may carry exact data of its own,
such as the hosts of package registries, and match facts against it. What it may not do is read a
judgement such as "kind of call".

- "This message holds a string with GitHub's token format and a valid checksum" is exact about the
  bytes. It is right every time it fires, including on a test token. So it can be the condition of
  a rule that ships with Silo, carrying the formats and checks as its own data. The same holds for
  "a string that starts with `sk_live_` and has 24 to 99 letters and digits after it". The rule
  decision is recorded with the rule's version (`rule-decisions-are-recorded`).
- "This is a live credential" is not exact. Only the issuer knows that. Silo never claims it. The
  app says "a string with GitHub's token format" and never "your GitHub token".
- "This random-looking string near the word `token` is a secret" is a judgement. It is an opinion,
  kept apart (`opinion-stored-apart`), and acts only through a precaution, so only protectively
  (`precaution-grants-nothing`).

What this changes in practice is whether Silo may keep the value out of the record. Not keeping
it is destructive: what was not kept cannot be brought back. That is how `kinds-of-other-actions`
files "remove content from the record's store" and "leave paths out of the checkpoints to come". So
a rule may do it on an exact match, and a precaution may never do it on entropy.

Is the action right whenever the exact condition holds? For holding and for keeping a value out of
the record, yes. A test key that passes GitHub's checksum costs one ask and a few characters
missing from stored text. That is the test for whether a byte pattern may drive a rule here: the
action must be acceptable on every string the condition matches, not only on real credentials.

### Measured: what each grade raises on text that holds no live credential

`scan_tree.py` reports each grade and never prints a value: a match shows its rule, a masked
context and a fingerprint.

| Corpus | Size | checked | prefix | shape | entropy |
|---|---|---|---|---|---|
| `node_modules` of `next typescript aws-sdk @aws-sdk/client-s3 googleapis stripe @slack/web-api openai @anthropic-ai/sdk firebase-admin` (text files up to 5 MB) | 606 MB, 35,452 files | 3 distinct JWTs, all in `aws-sdk`'s API examples (expired `idToken`s) | 1 AWS key ID from `aws-sdk`'s IAM examples. The documented `AKIAIOSFODNN7EXAMPLE` (1 more) was skipped | 0 | 45 distinct, all wrong: `integrity` hashes in lock files next to package names with "token" in them, badge URLs |
| This repository's old history, `git log -p origin/legacy`, 4,601 commits | 140 MB | 2 distinct private keys, both test keys of the vendored `russh` crate | 1 AWS key ID in a redaction test | 2 distinct, in tests, at `example.test` | 13 distinct, all wrong (lock-file hashes) |
| `git log -p main` | 7.5 MB | 0 | 0 | 0 | 1, wrong |
| The Claude Code session above, every new message | 110 KB | the planted token | the planted Stripe key | the planted URL | 0 |

Two more results from the old history matter:

- gitleaks 8.28.0 reports 14 findings of its GitHub token rules there, in an agent's committed
  run log (`.smithers/…/stream.ndjson`) that quotes GitHub's API documentation. Every GitHub-shaped
  string on those lines fails the checksum: they are GitHub's documentation examples. The checksum
  removes all 14 false alarms.
- The old code had a redactor of its own whose GitHub pattern was `(?:ghp|…)_[A-Za-z0-9]{20,}`,
  with a test value `ghp_abcdefghijklmnopqrstuvwxyz0123`. Without the checksum, a pattern like that
  also fires on documentation examples and on strings a model makes up.

On the public test suites of other scanners, which are full of made-up keys: in gitleaks, 5 of 5
distinct `ghp_`-shaped strings fail the checksum. In trufflehog, 3 of 4 fail. In detect-secrets, its one
distinct `ghp_` string passes, in its own test file. So a "checked" match on a test token is
uncommon, but it happens: one in trufflehog's tests and one in detect-secrets' tests pass the
checksum. This is why the app never says "live".

The synthetic corpus (`corpus.py`, 4,059 samples: every format above in seven contexts, plus
near misses) gives 100% of planted values found and no wrong match. That number only shows that each
rule matches its own format. The near misses are what matter:

| Near miss | What fired |
|---|---|
| `ghp_` + 36 random characters, or a real-shaped token with one checksum character changed | no rule. The entropy check fired on 70%, as an opinion |
| `postgres://postgres:postgres@localhost`, `postgres://app:${DB_PASSWORD}@db` | nothing |
| `AKIAIOSFODNN7EXAMPLE`, GitHub's documentation token | nothing (a short list of documented examples) |
| One of Silo's own placeholders shaped like a `ghp_` token | nothing (see below) |
| A key block that does not parse | nothing |

What it says about the dev.to case: 189 of the roughly 600 secrets (the GitHub tokens) are
"checked" formats. About 160 more are "prefix" formats the prototype has (103 Google, 45 AWS, 7
Anthropic, 3 Stripe). 41 more (OpenRouter, Resend, Telegram, Vercel) have prefixes the prototype
does not have yet. The 197 JWT-shaped strings are the noisy part: he notes many were public Supabase keys,
which are JWTs by design. That is why a JWT match may mark the record and keep the value out of it,
but should not hold a request.

### Cost per MB, and whether it can run inline

| What | Time |
|---|---|
| Python, exact rules behind one prefix pattern (`scan_fast`) | 19.7 ms per MB on `node_modules`, 23.7 ms per MB on the 140 MB history |
| Python, every rule plus entropy | 244 to 344 ms per MB |
| ripgrep, the same prefix pattern (a compiled regex engine, as the boundary would use) | 60 MB in 51 ms, about 0.85 ms per MB |
| gitleaks 8.28.0, `gitleaks git` on the old history | 71.6 MB in 10.0 s, about 140 ms per MB |

Every exact rule starts with a fixed string (`gh`, `sk-ant-`, `AKIA`, `PRIVATE KEY-----`…). So the
boundary runs one prefix search over each new message and the rules only near a hit. That prefix
search is the whole cost on a message that holds no credential.

### The re-send factor

Each request carries the whole conversation so far. In the 16-request session above:

```
all request bodies: 1740 KB; conversation text in them: 1045 KB; of it new: 110 KB
re-send factor (conversation text sent / text new): 9.5
scan time, whole bodies every request: 41.8 ms; new messages only: 2.7 ms
```

The factor grows with the length of the conversation: about half the number of requests. So a
detection is made once per new message, keyed by the message's hash, and remembered. A later request
that carries a message already seen carries the decision already made about it.

## What Silo does, filed by kind

| Action | Kind (`kinds-of-action`) | Triggered by | When |
|---|---|---|---|
| Record that a message holds a string of format F, with its keyed fingerprint | (a rule decision, recorded) | rule, on an exact match | Each new message |
| Mark a line as maybe holding a secret | opinion | entropy classifier | Each new message |
| Hold the model request and ask you | protective | rule, on a "checked" match or a live prefix (`sk_live_`, `rk_live_`, `sk-ant-api03-`, `sk-ant-admin01-`, `AKIA` with a secret key beside it, `xoxb-`/`xoxp-`, private key) | The first request that carries the new message |
| Keep the value out of the stored message text | destructive (it cannot be brought back) | rule, on any exact match, or you | When the message is stored |
| Alert you, with the provider's page for revoking it | protective | rule, or a precaution on the entropy opinion if you set one | After the hold, or at once when there is no hold |
| Make it a Silo secret, with the hosts you choose | granting ("give a secret") | you only, in the ask or the alert | When you answer |
| Swap it for that secret's placeholder in model requests | a change in traffic, see the challenge | your answer, or a rule you set in advance | From the first request that carries it |
| Revoke it at GitHub | destructive (GitHub's revocation cannot be undone, and the owner is notified) | you only, by a button | When you press it |
| Check whether it is live by calling the provider | not offered | | |
| Hold a push to a Git host while a tracked file holds a found value | protective | rule | Where Silo opens that host's traffic |

### Before the provider gets it: the hold

The boundary sees the request that carries your pasted token before it leaves. `what-a-precaution-may-do`
and `kinds-of-action` both make holding a waiting request protective, so a rule may hold it. The
ask (`what-an-ask-shows`) says, in Silo's words:

> main wants to send Anthropic a message that holds a string with GitHub's token format and a
> valid checksum (`ghp_Ab…`, in your message of 14:02). Once sent, the provider keeps a copy that
> Silo cannot remove.
>
> - Send it as it is
> - Make it a Silo secret for api.github.com and send a placeholder instead
> - Do not send it

Measured with `rig/patch_fake.py` (the fake API, with a knob to hold or refuse):

- **A hold of 150 s.** Claude Code 2.1.331 waited with no retry and went on normally when the
  answer came. It sends `X-Stainless-Timeout: 600` on each request, so its own limit is ten
  minutes, well above the 90 s of `hold-limit-length`.
- **A 403 refusal.** Claude Code retried it twice, one second apart. Then it ended the turn and
  showed Silo's text: `API Error: 403 Silo held this request: it carries a string with GitHub's
  token format and checksum. Answer the ask in Silo.` So the refusal at the end of a hold reaches
  you in Claude Code's own window.

What the hold costs: the whole run waits, since a run does nothing without its model. That cost is
why only the strongest grades hold. A "checked" match or a live prefix is rare in normal text. In 606 MB of
`node_modules` the only one was an AWS key ID in `aws-sdk`'s own API examples. A JWT, a test key (`sk_test_`) or a
database URL does not hold. It only marks the record, keeps the value out of it and alerts you.

Retries need care. A retry of a request whose hold already ended joins the open ask and gets its
403 at once, instead of a new 90 s hold. The ask stays open in the app after the hold ends. When
you answer later and type "go on", the next request carries the same message, and Silo applies your
answer by the message's hash.

### "Make it a secret" and the placeholder: what it buys

The value is already inside the computer: in Claude Code's transcript, in shell history, and in the
`.env`. Silo cannot remove it from there without changing the computer's disk, and an agent that is
treated as hostile has had it. So a placeholder does not make the value private again. It buys
three things:

1. **The provider never gets it**, if the swap starts with the first request that carries it.
2. **The model only ever knows the placeholder.** So every command the model writes from then on
   carries the placeholder. The model writes `curl -H "Authorization: Bearer <placeholder>"`. Claude
   Code runs it, Silo swaps the real value in on the way to api.github.com, and the call works. If
   you press "Don't Ask Again", the rule saved in `.claude/settings.local.json` holds the
   placeholder. That closes the Apiiro case for every command the model writes. It does not close
   it for a command you type yourself.
3. **Rotation becomes painless.** You revoke the old token and type the new one in Silo's own
   prompt (`secret-entry`). The placeholder in the computer keeps working, and the new value never
   enters the computer.

`rig/substitute.py` replays the session with each found value swapped for a placeholder in every
request. It printed:

```
2 values swapped; prefix kept between consecutive requests: original 15/15, swapped 15/15; values left in swapped bodies: 0
```

A swap made the same way in every request, from the first one on, keeps each request a byte-for-byte
continuation of the one before. So the prompt cache is not missed because of it. The cache is missed
once only when you choose the swap after the value has already gone out in earlier requests: from
that message on, the history changes.

For a value you choose not to make a secret ("Do not send it"), the run gets the refusal. Silo does
not swap in a placeholder with no secret behind it, because the model would then write commands
that fail with no reason given.

### The record

When an exact rule matches, the stored text of the message (`bulky-content-once`) holds a marker in
place of the value: `⟦github-token 3f9a…⟧`. A line says how many bytes were not kept and of which
format. The marker carries a keyed fingerprint: an HMAC of the value, with a key kept by Silo on
your Mac. A plain SHA-256 of a 20-character database password can be brute-forced from the record,
while a keyed one cannot. The same value always gives the same marker, so the record still shows
that the token in your message is the one in the `curl` five turns later.

This is done before the text is stored, so no line is edited (`changes-to-the-record`). The receipt
shows your request word for word (`what-a-receipt-holds`) with the marker in place of the token.

The message's identity hash, which `store-messages-once` uses to store each message once, must not
be a plain hash of text that holds a value Silo did not keep. With the rest of the message known,
it lets anyone with the record test guesses of a short password. So messages with a marker are
identified by a keyed hash too.

For an entropy opinion the text is kept as it is and the line is marked. If you then say "this is a
secret", you trigger the removal yourself. That takes a new decision, because removing a value from
text already stored is neither a size-limit removal nor a line edit. It is a proposal below.

### Rotation, and GitHub's revoke API

The alert links to the issuer's page for the format: GitHub's token settings, Stripe's API keys,
the AWS IAM console, the Anthropic and OpenAI key pages, the Slack app's tokens.

For GitHub, `POST /credentials/revoke` exists exactly for this. It is "intended to revoke
credentials the caller does not own". It takes no authentication ("Any authenticated requests will
return a 403"), up to 1,000 tokens per request and 60 requests an hour. It covers `ghp_`,
`github_pat_`, `gho_`, `ghu_` and `ghr_`. The owner is notified, and a revoked token cannot be
brought back ([GitHub docs](https://docs.github.com/en/rest/credentials/revoke)).

- **Kind:** destructive. It ends something for good, so `grant-needs-a-fact-or-you` already puts it
  out of a precaution's reach.
- **Outward:** it sends the token to GitHub, which issued it, and to nobody else. It reveals nothing
  new.
- **Who triggers:** you, by a button in the alert. A rule could do it on a "checked" match, but a
  checksum does not say whose token it is. A test token in a fixture could belong to a colleague's
  live CI account.

Silo does not check whether a value is live by calling the provider, as trufflehog's
`--results=verified` does (it calls `api.github.com` with the token). That would be Silo using a
credential you never gave it, with your authority, and it would show up in the owner's security log.

### Files on disk, and pushes

At each checkpoint Silo knows which files changed. The exact rules run on the changed text files
that Git tracks or that are staged, and on the agent programs' own settings files
(`.claude/settings.local.json`, `.codex/`, `.cursor/`). A match raises an alert that names the file
and the format. That is the Apiiro case caught on disk, for a command you typed yourself.

Where Silo already opens traffic to a Git host to swap in a secret, a rule can hold a
`git-receive-pack` request while such a file is unresolved. The pack is zlib-compressed objects, and
reading it inline is a cost to measure, so that comes later. Where Silo does not open that host, the
alert at the checkpoint is all there is. It usually comes before the push, since the agent writes
first and pushes later.

### Committed secrets on the first run

`first-run-offers-to-bring-in` already reads each repository before it copies it into main. Running
the exact rules over its working tree and `git log -p --all` costs little. This repository's old
history (4,601 commits, 140 MB of patches) took 3.3 s in Python, and gitleaks took 10 s on it. The
first run lists what it found: repository, file, commit, format, and whether the repository has a
public remote. It changes nothing. Rewriting history is your call, and rotation is what fixes a
committed secret anyway.

The same scan can read Claude Code's transcripts on your Mac (`~/.claude/projects`), where the dev.to
author found his 600. The first run already reads that program's settings. Reporting what the
transcripts hold, and touching nothing, is cheap. Whether Silo should read files it does not bring
in is for polarzero.

## Silo's own placeholders must not look like leaks

PR #347 showed that one `env` puts every placeholder into the conversation. If a placeholder looked
like a credential, every conversation would hold. Two measures:

- Silo knows every placeholder it issued, and an exact set lookup skips them before any rule runs.
- A placeholder keeps the provider's prefix and length, so programs that check the shape accept it.
  It carries Silo's marker in its body, and a checksum that is wrong for the provider's format on
  purpose (`make_placeholder` in `detector.py`). A scanner that checks GitHub's checksum, which GitHub's
  post gives as its purpose, then does not take a `.env` full of placeholders for a leak. Whether
  GitHub's push protection checks it was not tested. In the corpus, 210 placeholders raised nothing.

This fits PR #347's proposal that a placeholder seen in a body is forwarded as it is.

## Bad paths

| Path | Outcome |
|---|---|
| A test or documentation token passes a checksum (trufflehog's fixture) | One ask. Its value is left out of the stored text. The app says "format", never "your token". The cost is time. |
| A JWT that is public by design (a Supabase anon key, an AWS example `idToken`) | No hold. The value is kept out of the record and marked. If that marker is a nuisance, the JWT rule could keep only JWTs whose `exp` is in the future, which is a choice to make. |
| A credential with no known format (a password, a Postgres password outside a URL, a customer's own token) | Missed by the rules. The entropy check may mark it as an opinion. This design does not cover it. `secret-asked-mid-task` is the way to keep it out of the chat in the first place. |
| An agent splits, base64-encodes or reverses a token to get it out through the model provider | Not caught. This design is for accidental leaks by you or an agent that is not trying to hide. A hostile agent can send anything to the provider in any encoding. The model provider is an open channel that this design does not close. What protects a secret from a hostile agent is that the real value never entered the computer (`secrets-held-outside`). |
| An agent learns the rule and avoids it to get a value into the record | Nothing is gained: the value is in the record only if it was in the conversation, which the agent already had. |
| An agent fills the conversation with fake tokens that pass the checksum, to flood you with asks | One ask per distinct message, and each later one joins it. A flood shows up as a run that keeps getting held, which is itself worth seeing. A rule could cap asks per run. |
| A placeholder is found in a body | Skipped by exact lookup, never held. |
| Silo's scan finds a real value in files Silo shows in its own UI or logs | The detector never prints a value. Silo's logs and its crash reports go through the same rules (`secret-never-written-down` already says "a log or an event"). |
| You are away during the hold | After 90 s the request gets a 403. Claude Code retries twice and ends the turn with Silo's message (measured). The ask waits. Codex and other programs were not measured. |
| The value went out before you chose the swap (you answered "send as it is", or the match was entropy only) | The provider keeps its copy. Silo still keeps it out of its own record, alerts you, and the swap from then on costs one cache miss. |
| A value sits in a Kubernetes `Secret`, which is base64 | Missed. Decoding base64 in tool output before matching is a later rule. trufflehog decodes base64 and escaped Unicode for this reason. |

## Prior art

| Who | What they do | What differs here |
|---|---|---|
| GitHub secret scanning and push protection ([docs](https://docs.github.com/en/code-security/secret-scanning/introduction/about-push-protection)) | Blocks a push that carries a known format. Providers join the [partner program](https://docs.github.com/en/code-security/secret-scanning/secret-scanning-partnership-program/secret-scanning-partner-program) with a regex, "a uniquely defined prefix" and "a 32-bit checksum", and GitHub posts public matches to them to revoke. | It acts at the push, after the provider of the model already has the value. Silo acts at the first model request. Silo has no partner list, so it cannot tell a provider. You revoke. |
| gitleaks ([repo](https://github.com/gitleaks/gitleaks)) | About 200 regex rules, with no checksum. | It raised 14 GitHub false alarms on this repository's history where the checksum raises none. It is slower here: 140 ms per MB, against 24 ms in Python with a prefix filter. |
| trufflehog ([repo](https://github.com/trufflesecurity/trufflehog)) | Regex rules, decoders (base64, escaped Unicode), then verification by calling the provider (`CloudEndpoint() = https://api.github.com`). | Verification is outward and uses your authority. Silo does not do it. |
| detect-secrets (Yelp) | Plugins per format, plus entropy for base64 and hex. | Its entropy plugins are the noisy class measured above. |
| LiteLLM secret detection ([docs](https://docs.litellm.ai/docs/proxy/guardrails/secret_detection)) | A proxy for model requests that runs detect-secrets "pre_call" on user text, tool arguments and tool results, and replaces values with `[REDACTED]`. | The closest analogue to the swap. It replaces silently, with no ask and no secret behind it, so a command the model writes from it fails. Silo asks first and swaps in a placeholder that still works. |
| Cloudflare AI Gateway DLP ([docs](https://developers.cloudflare.com/ai-gateway/features/dlp/)) | Scans prompts and responses. It can flag or block, with no redaction. | Block only. Silo holds and asks. |
| The dev.to author's hooks | A `UserPromptSubmit` hook that blocks prompts holding a secret, a scrubber at `SessionEnd`, and a daily sweep. Prefix regexes only, "deliberately avoids generic high-entropy" patterns. | Same rule choice. But hooks live inside the agent program, which `rules-inside-are-no-boundary` rules out as a boundary. |
| Simon Willison's scan-for-secrets ([post](https://simonwillison.net/2026/Apr/5/scan-for-secrets-3/)) | Searches for values you list, in their common encodings, not for patterns. | Silo can do this exactly for every value it holds as a secret and for every value it has found once. That is a fact-grade search, and it catches the same value JSON-escaped. |
| Claude Code itself | No redaction of transcripts or requests (measured above). Feature requests: [#29434](https://github.com/anthropics/claude-code/issues/29434), [#20966](https://github.com/anthropics/claude-code/issues/20966). | |

Codex was not measured.

## How it fits the plan

Entries it uses as they stand: `conversation-text-kept`, `store-messages-once`, `bulky-content-once`,
`secret-entry`, `secret-asked-mid-task`, `secrets-held-outside`, `secret-values`, `kinds-of-action`,
`kinds-of-other-actions`, `what-a-precaution-may-do`, `what-a-report-may-trigger` (which replaces
`law-of-triggers`), `rule-carries-its-data`, `rule-decisions-are-recorded`, `what-an-ask-shows`,
`hold-limit-length`, `what-you-may-do`, `first-run-offers-to-bring-in`, `changes-to-the-record`,
`no-silent-change-to-traffic`.

### Challenge 1: `what-silo-changes-in-traffic` and `forwarded-byte-for-byte`

They say Silo changes model traffic "in one way only: it swaps a secret's real value in for its
placeholder", and forwards everything else "as the agent program and the provider wrote them". The
case: the model writes the `curl` with your pasted token, and Claude Code saves it into
`.claude/settings.local.json`, which gets pushed. That is how Apiiro's 1,122 files happened. A
second change, swapping a value you just made a secret for its placeholder in model requests, is the
one way Silo can stop it, because the model then never knows the value. Measured above, it costs no
prompt-cache hit when it starts with the first request that carries the value.

What I would put in place of the answer of `what-silo-changes-in-traffic`: "Silo reads model
traffic and changes it in two ways only. It swaps a secret's real value in for its placeholder
where the secret names it. And when you make a value found in the conversation into a secret, it
swaps that value for the secret's placeholder in every request after that. Both changes are
recorded, and the record never holds a secret's real value." `forwarded-byte-for-byte` then points
to it as it does today.

### Challenge 2: `how-a-hold-ends`

It says "a 403 is not retried by the usual programs, where a 5xx is". Claude Code 2.1.331 retried a
403 twice, at one-second intervals (its stream said `"api_retry", "attempt": 1, "max_retries": 300,
"error_status": 403, "error": "authentication_failed"`), then ended the turn showing the body. The
outcome is still good, because Silo's words reach you. But the sentence is wrong for this program.
Each retry would also be a new request to hold, unless Silo ties it to the hold that already ended.
In its place: "a 403 ends the request. Claude Code 2.1.331 retries it twice and then shows its body.
A retry of a request whose hold has ended is answered at once with the same refusal."

### Challenge 3: `store-messages-once`

It identifies each message by a hash of its text. For a message holding a short password that Silo
kept out of the stored text, a plain hash lets anyone with the record and the rest of the message
test guesses offline. In its place, for messages with a marker: identity by an HMAC with a key that
stays on your Mac.

## Recommendations

1. **A new decision in `record`**, `credential-found-in-text`. Asks: "What does Silo do with a
   credential it never held, found in text it reads?" Answer: "Silo runs rules that carry the
   formats of credentials as their own data: a checksum the issuer built in, a key that parses, or
   a documented prefix. A match is recorded as a rule decision, by format and keyed fingerprint,
   never as 'your token'. The value is not kept in the stored text, and a marker takes its place. A
   random-looking string near a word such as 'token' is only an opinion: the line is marked, the
   text is kept, and you can remove the value yourself." Sets the property `found-value-not-kept`:
   "A value that matches one of Silo's credential formats is not written to the record, a log or an
   event", checked by a hostile test.
2. **A new feature in `boundary`**, `leaked-credential-held`. Name: "A credential in a message is
   held before it leaves". What: "The first model request that carries a new message holding a
   string with a checked credential format, or a live key's prefix, is held. The ask offers to send
   it as it is, to make it a secret and send a placeholder instead, or not to send it. The alert
   links to the issuer's page for revoking it, and for GitHub tokens it has a button that revokes it
   through GitHub." Needs: `held-requests-and-asks`, `secrets-held-outside`, `credential-found-in-text`,
   and the change of Challenge 1 for the second answer.
3. **A sentence in `first-run-offers-to-bring-in`**: "Each repository is scanned with Silo's
   credential formats, history included, before it comes in. What is found is listed with its file,
   commit and format, and nothing is changed."
4. **A sentence in the notes of `secret-entry`**: the leak case and the answer that a value pasted
   into a chat stays not a secret, while Silo offers to make it one from then on.
5. **The changes of Challenges 1 to 3.**
6. **A choice for polarzero**: whether the first run also reports on the agent programs'
   transcripts on your Mac.

## Open questions

- The checksum of `github_pat_` tokens comes from one scanner's tests and was not checked against a
  token GitHub issued.
- Codex and the other agent programs: how long they wait on a hold, and how they take a 403.
- Whether Anthropic's and OpenAI's key formats are stable enough to be a rule's data. Neither
  publishes its format the way GitHub does. The prefixes come from what scanners use.
- Reading `git-receive-pack` inline, for the push rule: its cost per MB is not measured.
- Whether a JWT with an `exp` in the past should still be kept out of the record.

## Run it

From `research/2026-11-24-gap-fixes/prototypes/leaked-secrets/`:

- Synthetic corpus: `python3 corpus.py /tmp/c.jsonl && python3 eval_corpus.py /tmp/c.jsonl`
- Any tree or file (for example `git log -p > /tmp/log.txt`): `python3 scan_tree.py <path>`
- Claude Code traffic: `rig/run_cc.sh /tmp/cc1 && python3 rig/analyse.py /tmp/cc1 && python3 rig/substitute.py /tmp/cc1`
- Hold and refusal: `python3 rig/patch_fake.py /tmp/fk && HOLD_S=150 rig/run_cc.sh /tmp/cch /tmp/fk/fake_anthropic.py`
  (or `REFUSE=6`)

`run_cc.sh` clones this repository into `/work/g5repo` and needs `claude` on the path. It plants
only values made up by `make_scenario.py`.
