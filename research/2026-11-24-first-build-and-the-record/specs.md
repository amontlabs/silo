# What option A changes in #370's build specs

An agent's part, 24 November 2026, for polarzero and for the agents who will build. #370 specifies
#348's first build in steps 1 to 5. Option A of [the comparison](README.md) keeps those steps and
adds the record of model traffic, spend and a first receipt. This page says what changes in each
page of #370, and specifies the one new step, the boundary for model traffic, at the level #370 uses.
Where it rests on one of the 19 decisions, it follows the text proposed in
[decisions.md](decisions.md).

Nothing here is decided, and nothing may be built before polarzero takes the features it serves.
Nothing here has run on a Mac.

## The steps under A

| Step | #370 | Under A |
|---|---|---|
| 0 | The Mac kit | Tests 07 and 21 move to the front, with a new test (below) |
| 1 | The engine, the recorder, `silo` | The same, and the access model grants hosts and the authority |
| 2 | Main runs | Main on Silo's network stack from its first start, behind the boundary. The first run puts Silo's authority in main |
| 3 | A night | The same, and a restart of the boundary in the night's tests |
| 4 | The way back | The same, and thinning keeps a checkpoint an open alert points to |
| 5 | The record | File facts, `changes` and `verify`. `look`, `put-back` and `import` move to just after the first build |
| 6 | (none) | **New: the boundary for model traffic, conversations, spend, receipts** |

Step 6 does not need steps 2 to 5 to start. It runs against a Linux test computer, or #371's rig,
from the end of step 1, while the Mac kit runs. It joins main in step 2: the boundary is in main's
network path from main's first start.

## Step 0: the Mac tests

From #359's kit, in this order:

- **Test 07**, gvproxy under a macOS computer, with its API token and without its map to the Mac's
  `127.0.0.1`. Under #370 it waits until after main runs. Under A it comes before step 2, because
  main has no Apple's NAT phase.
- **Test 21**, whether Claude Code and Codex trust an authority in main's System keychain, and
  whether one can be added over SSH with no one at the screen. Its outcome `keychain-needs-a-person`
  leaves each program's own setting as the only way, which step 2 then writes.
- **A new test, 34: a night through the boundary.** Claude Code on polarzero's Max plan and Codex
  signed in with ChatGPT, in a macOS computer, through a boundary that reads both, for a whole night.
  Done when the conversations are recognised whole, the plan's headers are read, every reply from a
  provider that is not a normal one is a fact, and nothing differs from the same night without the
  boundary (`provider-sees-silos-connection`). It needs step 6's boundary, so it runs at the end of
  step 6, not before step 1.

## Step 1: the engine

`1-engine.md` changes in one place.

- **The access model grants real hosts.** `core/access.zig` already has `host` in its type. Under A,
  step 2 grants main the hosts the boundary reads, and the authority's use for them, so the
  exhaustive test covers a grant that exists in the first build. That answers #348's challenge to
  `model-of-access-first`: the model is checked against real grants from the first day.
- **The ring is sized for model calls.** One model call is one line, written when its reply ends,
  not one line per streamed event. A night of #351's case (214 turns) is a few thousand lines. The
  engine sees none of them: they take the record's path (`record-path-and-core`), from the boundary
  to the recorder.

## Step 2: main runs

`2-main-runs.md` changes in three places.

- **The network.** The table "Apple's NAT (first) / Patched gvproxy" keeps only its second column.
  The helper attaches `VZFileHandleNetworkDeviceAttachment` to a gvproxy it starts, from main's
  first start. gvproxy's API is removed or needs its token (#358), and its map to the Mac's
  `127.0.0.1` is gone. A closed main is possible from the first build. `silo status` says
  `network open, model traffic read, nothing else checked`.
- **The boundary is in the path.** gvproxy hands TCP connections to Silo's boundary, which reads the
  hosts of the list (step 6) and passes every other connection on, recording its host, size and
  timing. UDP to port 443 of a host the boundary reads is refused, so HTTP/3 falls back to TCP
  (`ending-the-encrypted-connection`, #347's QUIC check).
- **The first run puts the authority in main.** A new operation step, after the agent programs are
  installed (`main-set-up-for-agents`):
  1. Silo makes its authority on the Mac, once, at `<data>/boundary/authority.key` (0600) and
     `authority.crt`. The key never enters a computer.
  2. Over SSH it adds `authority.crt` to main's System keychain as trusted for TLS, if test 21 says
     that works with no one at the screen.
  3. In every case, it writes the settings for the programs that keep a store of their own, in
     `/etc/zshenv` and `/etc/profile` of main, so tmux and SSH sessions get them:
     `NODE_EXTRA_CA_CERTS`, `CODEX_CA_CERTIFICATE`, `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE`,
     `AWS_CA_BUNDLE` and `NODE_OPTIONS=--use-system-ca`, each naming a bundle of the system's roots
     plus Silo's authority, as #347's list does on Linux.
  4. It runs `claude --version` and `codex --version` and one request to each provider's host
     through the boundary with no credential, and checks that the handshake succeeds. A failure
     stops the first run and names the program.

  The first run's summary says, in its own line: "Silo reads the traffic between Claude Code and
  Codex and their providers, to count spend. Your sign-in passes and is written nowhere."

`<data>` gains:

```
<data>/
  boundary/
    authority.key  authority.crt     Silo's authority, made once
    roots.pem                        the roots Silo checks the providers' certificates against
    hosts.json                       the hosts read, per provider and kind of sign-in, with the format
    prices.json                      Silo's price table, with dates of effect
  content/
    <sha256>                         the text of messages, once each (conversation-text-kept)
```

## Step 3: a night

`3-a-night.md` adds one test. The boundary is restarted during a streamed reply, with a clean
close and with a reset. Done when Claude Code and Codex send the request again within a second and
the conversation goes on, and the record holds the cut call with its counts marked missing (#347).
A boundary that leaves a reply open makes both programs wait 300 s, so the boundary's stop closes
every connection it holds.

## Step 4: the way back

`4-the-way-back.md` changes in two places.

- **Thinning** keeps a checkpoint that an open alert points to until the alert is set aside (#351,
  recommendation 15). In A the alerts are the claim mismatches and files changed outside your
  repositories.
- **Restore** says what it cannot bring back. The restore screen lists the conversations that sent
  requests after the checkpoint, and the plan's share used since. A provider keeps what it stored
  (Anthropic's Files, OpenAI's stored responses, #351), which a restore of main does not touch.

## Step 5: the record

`5-the-record.md` keeps the reader, the comparer, `silo` with no argument, `silo changes` and
`silo verify`. `look`, `put-back` and `import` move to just after the first build, with their
specs as written. The first screen gains the conversations:

```
$ silo
main · running · 8 h since you last looked (Mon 22:58)

Needs you
  claude  «All tests pass.»  ◇ the last whole `npm test` exited 1 at 03:12      silo receipt c-7f2a
  deleted ~/work/shared-fixtures/ · 3,412 files · outside your repositories   02:40 → 02:45
          ◇ codex c-91bd ran `rm -rf ../shared-fixtures` at 02:41
  sent 2.1 MB to paste.rs · first time seen · not attributed                    02:47

Conversations since Mon 22:58
  claude  c-7f2a  6 h 54 · 212 calls · $14.20 at API prices · 38 files in ~/work/app
  codex   c-91bd  5 h 16 · 96 calls · price not known (gpt-5.5) · 3 files outside your repositories
  Max plan: 34% of this week used (from Anthropic's headers)

Not known yet: processes, and which conversation changed which file. They come with the probe.
```

◇ marks an opinion. Which conversation changed a file is not claimed. A file change and a tool call
whose command names that path, in the same stretch between two checkpoints, are shown side by side
in the receipt, and the link stays an opinion (`the-chain`).

`silo receipt <conversation>` prints what `what-a-receipt-holds` lists, in its order, with the
items A cannot know said as not known yet:

| Item | Under A |
|---|---|
| What you asked | the first user message of the conversation, word for word, from `content/` |
| The checks Silo ran | none in the first build: "Silo ran no checks" |
| Files that changed | file facts between the checkpoints around the conversation, grouped by repository and outside. Not attributed to the conversation: shown as "changed while it ran" |
| Commands and exit codes | each tool call of the conversation, with the exit code the program sent to its model, marked as a report from the traffic. The last run of the whole suite of each test command first |
| Still running | "not known yet" |
| Hosts and what was sent | the conversation's own model calls; other hosts of main in the same window, as not attributed |
| Model calls, spend | calls, models that answered, spend at API prices, the plan's share |
| Time | from the first request to the last reply, and the time between replies and the next request as waiting |
| The agent's summary | the last reply's text, beside the receipt, with each claim of the two kinds #371 checks marked as checked, mismatched or not checked |

The classifier of claims is the first opinion Silo writes. Opinions go to a chain of their own
(`what-the-record-holds`, formats.md). It ships with two kinds of claim, as #371 did.

## Step 6: the boundary for model traffic

New page, at the level of #370's others. It builds `model-traffic-read`, `model-calls-in-the-record`,
`spend-per-conversation` and the first receipt.

### The pieces

| Piece | Where | What it does |
|---|---|---|
| The boundary | `boundary/`, a Zig program, `silo-boundary`, one per computer | Ends TLS for the hosts of `hosts.json`, forwards byte for byte, reads a copy, sends model-call lines to the recorder |
| The readers | `boundary/read/` | Anthropic Messages (stream of events), OpenAI Responses (stream and WebSocket), Chat Completions. Each reads only its own fields at their own level (`what-traffic-gives`' notes) |
| Conversations | `boundary/conversation.zig` | `recognising-a-conversation` as proposed in decisions.md, per computer |
| Prices | `boundary/price.zig`, `prices.json` | Spend from counts, a missing price is "not known" |
| The receipt and the claims | `cli/receipt.zig`, `opinions/claims.zig` | Step 5's output |

**Which process.** `which-process-holds-the-boundary` answers "the helper of that computer". The
helper is Swift (`macos-vm-driver`). The boundary is a second process in the helper's launchd job,
started by the helper with gvproxy, and stopped with it. It outlives the engine as the helper does.
If polarzero reads the answer as one process, the boundary is a library the Swift helper calls
through its C interface, which costs a Zig build inside the helper's.

### TLS

- The boundary knows the host by SNI. A host not in `hosts.json` passes unread, through gvproxy as
  before.
- For a host it reads, it makes a certificate at the first connection: P-256 key, the host in the
  subject alternative name, Authority and Subject Key Identifiers, signed by the authority. #347
  measured 0.27 ms to make one. The test connects Python 3.13, Node, curl and Go.
- Upstream, it checks the host's certificate against `roots.pem`, never main's store. It offers
  `http/1.1` only, since neither Claude Code nor Codex offers ALPN and every program tried fell back
  to HTTP/1.1 (#369). HTTP/2 waits for a host that needs it (gRPC, #347).
- It does not copy the program's TLS fingerprint (`provider-sees-silos-connection`, decisions.md).

### Forwarding

From the spike of #347 and the red team's patch of #358:

- Byte for byte, both ways, compressed bodies forwarded as they came and read from a copy (gzip,
  zstd, and permessage-deflate on Codex's socket with its state kept per message).
- `TCP_NODELAY` on both connections.
- A request is read whole before it is forwarded only where a rule needs it. In A no rule reads a
  request, so requests stream through, and reading a copy costs them nothing.
- Strict framing: two lengths, a malformed chunk or anything a second parser could read as two
  requests is refused and recorded (decisions.md, `forwarded-byte-for-byte`).
- No timeout on a slow request body.
- At stop, every connection is closed or reset.
- The `Authorization` header, `x-api-key`, cookies and the sign-in callback are never copied into
  anything the boundary writes.

### What it writes

One `model-call` line per call, when its reply ends or the connection drops:

```json
{"v":1,"seq":2210,"prev":"…","time":"2026-11-25T03:12:44.018Z","computer":"c1","source":{"id":"boundary:c1","count":"b:2c7e…","n":88},"is":"fact","kind":"model-call","host":"api.anthropic.com","provider":"anthropic","format":"messages","program":{"agent":"claude-cli/2.1.340","checked":false},"conversation":"c-7f2a","request_id":"req_…","model":{"asked":"claude-opus-5-5","answered":"claude-opus-5-5"},"started":"…","ended":"…","status":200,"tokens":{"in":1840,"out":612,"cache_read":98211,"cache_write":0,"iterations":[]},"tools":{"called":[{"id":"toolu_…","name":"Bash","input":"sha256:…"}],"results":[{"id":"toolu_…","exit":1,"content":"sha256:…"}]},"provider_tools":[],"plan":{"7d":0.34,"reset":"…"},"spend":{"usd":"0.4112","price":"2026-11-01"},"text":{"request":"sha256:…","reply":"sha256:…"}}
```

- The text of messages goes once to `content/<sha256>`, and the line holds its hash
  (`store-messages-once`, `conversation-text-kept`).
- `program.checked` is `false` for a version no check covered (decisions.md,
  `reading-each-agent-program`).
- A count the reply did not give is absent from `tokens` and named in `missing`, never zero.
- A connection to a host not read is a `connection` line: host, port, bytes each way, start, end.
- A refused HTTP/3 attempt and a failed handshake are `connection` lines with the refusal.

`formats.md`'s table "What each step writes" gains a column for step 6: `boundary/` and `content/`
in `<data>`, `model-call` and `connection` lines from the source `boundary:<computer>`, and the
opinions chain from the claims classifier.

### Cost targets

From #347's spike, to hold on a Mac: the first byte of a streamed reply within 1 ms of a direct
connection at the median; a 2 MB request forwarded within 40 ms; a certificate made within 1 ms.

### Tests

- Byte for byte: #347's 100 compressed requests from 1 KB to 2 MB, and every reply shape.
- The red team's seven malformed requests (#358): all refused, and Claude Code and Codex unaffected.
- Real programs: Claude Code and Codex, each with a subscription and with an API key, on Linux,
  then in main. Conversations whole through a compaction and a `--resume` (#347, #371).
- A cut reply, from step 3.
- The record: `silo verify` holds with the boundary and the recorder writing at once.
- Mac test 34 above.

### Done when

- A night of Claude Code and Codex in main ends with every model call in the record, every
  conversation recognised, spend at API prices and the plan's share shown, and `silo receipt` for
  each.
- On #371's night replayed in main with real models, both false claims show as mismatched.
- No credential appears in any file under `<data>`, checked by searching for the sign-in's value
  after the night.

## Not specified here

- Budgets, holds, denied tool calls and secrets: the second and third builds.
- Programs other than Claude Code and Codex: `reading-each-agent-program` checks each before it is
  read. Gemini CLI signed in with Google passes unread (#368).
- Processes, and which conversation changed which file: the probe, in the second build.
