# The 19 decisions option A adds: second opinions, to rule in one sitting

An agent's second opinion, 24 November 2026, for polarzero. Option A of [the comparison](README.md)
adds the record of model traffic, spend and a first receipt to #348's first build. That needs these
19 open decisions on top of #348's 70. Each answer below is read from `plan show <id>` at `1b8f191`.
The evidence is in the notes that the drafts of the same day add to these entries: #347 (trials on
Linux and the boundary spike), #351 (the record and budgets), #358 (red team), #364 (the
reconciliation), #368 (the providers' terms), #369 (every other agent program) and #371 (the
morning review prototype). Nothing under `plan/` changes here and no verdict is written.

Every one of the 19 is polarzero's to rule. Each promises something you see, or sets a word, so
none fits what `owners-part-on-engine` lets agents take.

## The sheet

| id | Recommendation | Confidence | In one line |
|---|---|---|---|
| `ending-the-encrypted-connection` | Take with an edit | high | Name what each certificate must hold: Python 3.13, Go and others each refuse a different omission |
| `forwarded-byte-for-byte` | Take with an edit | high | A request with ambiguous framing must be refused, or a second request rides past the record (#358) |
| `what-silo-changes-in-traffic` | Take #364's rewording | medium | "One way only" already fails four cases the drafts found; A changes nothing, so the wider text costs it nothing |
| `reading-each-agent-program` | Take with an edit | high | The first build installs programs unpinned, so "each version a recipe pins" has no version to check |
| `subscription-and-key-alike` | Take as written | high | Measured cases agree: Codex's two hosts, Claude's one host shared by two kinds |
| `provider-sees-silos-connection` | Take as written | high | Keep Silo's own TLS fingerprint; copying the program's would read as disguising the client |
| `anthropic-reads-intermediate` | Take with an edit | medium | Name the Usage Policy's clause of 12 November in the question to Anthropic |
| `logins` | Take as written | medium | Nothing in A holds a login; the vault of #363 is a later replacement, not a reason to wait |
| `fork-and-sign-in` | Take as written | medium | It binds nothing in A, which has no fork, and `logins` needs it |
| `program-that-cannot-be-read` | Take with an edit | high | Under a budget, model traffic Silo cannot count must be denied (#369, #364) |
| `hidden-reasoning-stays-closed` | Take as written | high | Holds for Codex's sealed compactions too |
| `recognising-a-conversation` | Take with an edit | high | Three findings from real traffic: one computer per conversation, rewritten history, and the socket's links |
| `what-traffic-gives` | Take with an edit | medium | Name the counts that the top-level usage leaves out, or spend is short on Anthropic's fallbacks |
| `how-spend-is-shown` | Take as written | high | The plan's share is read from headers the providers send |
| `network-place` | Take as written | medium | Its one finding (a program's start-up hosts) is an opinion for later, not a change to the place |
| `what-a-receipt-is` | Take as written | high | It sets the word `receipt`; the prototype used it as written |
| `what-a-receipt-holds` | Take with an edit | medium | Add what is still running, and put the outcome of the last whole test run first |
| `agent-summary-beside-receipt` | Take with an edit | high | Say that a claim Silo cannot check shows as not checked, never as true |
| `conversation-text-kept` | Take as written | high | The receipt's first line needs it |

Nine are taken as written, nine with an edit, and one with #364's rewording.

---

## `ending-the-encrypted-connection`

**Asks:** How does the boundary read encrypted model traffic?

- **Recommendation:** take with an edit.
- **Confidence:** high. Run on Linux with four clients and five shapes of certificate (#347).
- **The case:** main's agent writes a Python 3.13 script that calls api.anthropic.com with the SDK.
  Silo makes a certificate for that host without the Authority Key Identifier extension, as some
  libraries do unless asked. Node, curl and Go accept it. Python 3.13 refuses it ("Missing
  Authority Key Identifier"), so every Python program of 3.13 or later fails on every host Silo
  reads. Go refuses a certificate that names its host only in the common name, which breaks `gh`
  and `terraform`.
- **Why the answer holds otherwise:** reading only the hosts Silo needs keeps the number of places a
  program can refuse Silo small. On Linux, Claude Code, Codex and eight more programs accepted an
  authority in the system's store (#347, #369). Three needed a setting their recipe would write
  (Gemini CLI, Qwen Code, Aider).
- **Proposed text** (one sentence added after "The versions and settings of the encryption are
  written down and tested, not left to defaults."):

  > Silo's authority marks its basic constraints and key usage as critical. Each certificate Silo
  > makes has an elliptic-curve P-256 key, names its host in the subject alternative name, and
  > carries the Authority and Subject Key Identifiers. A test connects Python, Node, curl and Go to
  > it, since each refuses a different omission.

- **What only a Mac can answer:** whether macOS lets Silo add its authority to main's System
  keychain without a person at the screen (#359, test 21). If not, Claude Code and Codex get it
  through their own settings (`NODE_EXTRA_CA_CERTS`, `CODEX_CA_CERTIFICATE`), which the answer
  already allows ("in the settings of the programs that keep a store of their own").

## `forwarded-byte-for-byte`

**Asks:** What does the boundary forward between an agent program and its provider?

- **Recommendation:** take with an edit.
- **Confidence:** high. The red team ran it against the spike (#358).
- **The case:** a script in main sends api.anthropic.com one request whose framing two parsers read
  differently: a `Content-Length` and a chunked body, or a malformed chunk. The spike read one
  request and recorded it. The host read two, the second carrying a `web_fetch` tool, and the
  program got the second's answer. 3 of 7 such cases got a whole request past the record, and 3 more
  passed a body the spike could not read. A patch of about 100 lines refused all 7, and Claude Code
  2.1.331 ran through it with nothing refused.
- **What conflicts:** the answer says "A message Silo cannot read passes unchanged, and the record
  says it could not be read." For a body in a format Silo does not know, that is right. For framing
  that Silo and the host may read differently, it lets a request through that the record never saw.
- **Proposed text** (added after "A message Silo cannot read passes unchanged, and the record says
  it could not be read."):

  > A request whose framing Silo and the host could read differently, such as two lengths or a
  > malformed chunk, is refused and never forwarded, and the record holds the refusal.

- **Also from #347, for the spec rather than the answer:** `TCP_NODELAY` on both connections (it
  removed 37 to 40 ms from every first byte); no timeout on a request body that arrives slowly,
  since Claude Code can upload a request while you type, for up to two minutes; and a restart of the
  boundary closes or resets every reply in flight, because both programs then send again within a
  second, where a reply left open makes them wait 300 s.

## `what-silo-changes-in-traffic`

**Asks:** What does Silo change in model traffic?

- **Recommendation:** take #364's rewording in place of the answer.
- **Confidence:** medium. The four cases are measured. Whether polarzero wants to rule on secrets in
  this sitting is the doubt.
- **The case:** a Bedrock user keeps the AWS key outside main. Each request is signed with the key,
  so there is no value to swap: the boundary has to sign the request again (#347, a 54-line
  re-signer that Claude Code ran through). "In one way only: it swaps a secret's real value in for
  its placeholder" forbids it. #355, #356 and #361 found three more cases of the same kind.
- **Why it costs A nothing:** in A, Silo holds no secret, so it changes nothing in traffic. The
  decision only has to be true. Taking the narrow text now and replacing it in the second build is a
  second ruling on the same question.
- **Proposed text**, from #364 section 5:

  > Silo changes traffic it reads in three ways only, each recorded with what it replaced: it puts a
  > secret to use, by swapping its value into the place it names or by signing the request again
  > with it; it keeps a secret out, by putting a placeholder in a reply in place of a secret's value
  > or of a token minted from it; and, on your yes or a rule you set, it sends a request changed so
  > that the call can be undone. Anything else that a rule or a budget refuses is not changed: it is
  > not sent.

- **If polarzero would rather not rule on secrets now:** take the answer as written. Nothing in A
  breaks it.

## `reading-each-agent-program`

**Asks:** Can the boundary read the model traffic of each agent program?

- **Recommendation:** take with an edit.
- **Confidence:** high.
- **The case:** the first build installs Claude Code and Codex with their vendors' installers
  (`main-set-up-for-agents`), with no recipe and no pinned version. Claude Code updates itself. One
  morning main runs Claude Code 2.1.340, which no check covered. The answer runs the check "for each
  new version a recipe pins", so it says nothing about this one.
- **What is known:** Claude Code 2.1.331 and Codex 0.172.0 were read through the spike on Linux,
  over plain requests, a stream and Codex's WebSocket with compression (#347). Both trusted the
  system's store on Linux. On a Mac, both read the keychain by their code, which has not been run.
- **Proposed text** (replacing "The check runs again for each new version a recipe pins."):

  > The check runs again for each new version a recipe pins. A version no check covered is still
  > read, and the record and the app mark what was read from it as from a version not checked,
  > until a check covers it.

- **Why not refuse an unchecked version:** Claude Code updates itself several times a week. Refusing
  would stop polarzero's agents at night for an update. Marking it keeps the night and says what is
  less sure.

## `subscription-and-key-alike`

**Asks:** Does a subscription sign-in change how Silo reads traffic?

- **Recommendation:** take as written.
- **Confidence:** high.
- **The case:** Codex under a ChatGPT sign-in talks to chatgpt.com over a WebSocket; with an API key
  it talks to api.openai.com. Claude Code with a subscription and with a key both talk to
  api.anthropic.com. The answer lists hosts per provider and per kind of sign-in, and makes the
  choice per computer where two kinds share a host. Each of these was seen in the trials (#347,
  #369), and nothing found contradicts it.
- **One thing to check in the spec:** the Codex check of #347 ran with an API key. Codex signed in
  with ChatGPT, over chatgpt.com's WebSocket, has been read only by mitmproxy on 19 November. It
  belongs in the first check through Silo's own boundary.

## `provider-sees-silos-connection`

**Asks:** Does a provider treat a subscription's traffic differently when Silo's network stack makes
the connection?

- **Recommendation:** take as written. Do not take #347's proposal to copy the program's TLS
  fingerprint.
- **Confidence:** high.
- **The case:** through Silo, Anthropic sees the boundary's TLS fingerprint (JA4), not Claude Code's
  (#347). #347 proposes connecting upstream with the program's own ClientHello so the provider sees
  what it would see without Silo. That needs a second TLS library (BoringSSL).
- **Why not:** Anthropic's Usage Policy of 12 November names services that "misrepresent the product
  or client being used" (#368). `disguising` and the terms research both put Silo on the side of
  never presenting itself as the program. Every company's inspecting proxy shows its own fingerprint,
  and Anthropic documents those proxies as working. The answer's watch for unusual replies is the
  right measure.

## `anthropic-reads-intermediate`

**Asks:** How does Anthropic read "intermediate" for a boundary that forwards a sign-in token and
keeps nothing?

- **Recommendation:** take with an edit.
- **Confidence:** medium. The reading of the terms is not a lawyer's.
- **The case:** polarzero runs Claude Code on a Max plan in main. Since 12 November the Usage Policy
  forbids to "Resell, proxy, or otherwise provide access to Claude through unauthorized means", in a
  bullet that names services routing requests through consumer subscriptions. The answer's question
  to Anthropic names only "intermediate".
- **Proposed text** (replacing "Silo asks Anthropic in writing, with `recipe-under-preinstall-rule`,
  before anyone but its owner uses it."):

  > Silo asks Anthropic in writing, with `recipe-under-preinstall-rule`, before anyone but its owner
  > uses it, and names both the word and the Usage Policy's clause against providing access
  > "through unauthorized means".

- **For A:** the risk is on polarzero's own account until the answer comes, as the answer already
  says. #368 has the question ready to send.

## `logins`

**Asks:** What part does Silo play in the logins of agent programs?

- **Recommendation:** take as written.
- **Confidence:** medium. #364 section 16 proposes the vault of #363 for MCP sign-ins and API keys,
  which would replace part of this later.
- **The case:** in A, polarzero signs in to Claude Code and Codex inside main, the vendors' way.
  Silo forwards the sign-in in each request and keeps it nowhere. That is this answer.
- **Why not wait for the vault:** the vault needs secrets held outside, which is the second build,
  and #368 found that holding a Claude or Google subscription's sign-in is likely forbidden. The
  vault would replace this answer for API keys and MCP sign-ins only, through a decision that names
  it in `replaces`.
- **Also found:** #369 lists where eight more programs keep their login, and #358 proposes keeping a
  hash of the sign-in so the boundary can refuse it on any other host. Both are for the second build.

## `fork-and-sign-in`

**Asks:** Does a fork carry the sign-in of an agent program?

- **Recommendation:** take as written.
- **Confidence:** medium.
- **Why now:** `logins` needs it, and A has no fork, so taking it now commits the first build to
  nothing. Leaving it open keeps `logins` open. #347 tried a fork carrying an MCP sign-in: the fork's
  renewal signed main out. That supports leaving the sign-in out unless you say.

## `program-that-cannot-be-read`

**Asks:** What does Silo know of an agent program whose traffic cannot be read?

- **Recommendation:** take with an edit.
- **Confidence:** high.
- **The case:** main has a $50 budget. Aider, run by an agent, switches to OpenRouter with one flag
  (`--model`), and Silo does not read OpenRouter. The answer says budgets "do not cover that
  traffic", so the budget is one setting away from counting nothing. Cline, OpenCode and Qwen Code
  have the same one-setting switch (#369). The $453 runner of #351 had a `--provider openai|gemini`
  switch.
- **Proposed text**, #364 section 17's position (added at the end of the answer):

  > Under a budget, a connection to a known model provider's host whose traffic Silo does not read
  > is denied, since the budget could not count it, and the refusal says so.

- **For A:** A has no budget, so the sentence binds the third build. It belongs in the answer now so
  that the first build's record already marks such connections.

## `hidden-reasoning-stays-closed`

**Asks:** Does Silo open the model's hidden reasoning?

- **Recommendation:** take as written.
- **Confidence:** high.
- **Also found:** Codex compacts on OpenAI's side and gets back an `encrypted_content` item (#347).
  It is sealed in the same sense, and the answer already covers it: the record says it passed, with
  its size.

## `recognising-a-conversation`

**Asks:** How is a conversation recognised in each provider's traffic?

- **Recommendation:** take with an edit.
- **Confidence:** high. Each finding comes from real Claude Code 2.1.331 and Codex 0.172.0 traffic.
- **Three cases the answer gets wrong:**
  1. **Rewritten history.** Claude Code drops a leading `cd <dir> &&` from earlier tool inputs and
     inserts a message after the first turn. A test of "begins with the earlier request" split every
     Claude Code conversation after turn 1. Comparing tool calls by id and name kept all four whole
     (#371).
  2. **A fork sends the same session.** A conversation taken up in a fork and in its parent sent the
     same session id from both (#347). By the answer, they are one conversation, and its budget
     counts both.
  3. **A socket sends only what is new.** Over its WebSocket, Codex sends the whole conversation once
     per connection, then only new items, naming the previous reply in `previous_response_id`. The
     answer allows following that link; the spec has to.
- **Proposed text** (replacing the answer):

  > By its content, to find out on captured traffic of each program Silo offers, before a budget on
  > a conversation is offered for it. A conversation belongs to one computer: requests from two
  > computers are never joined, and a fork starts its own conversations, each linked to the one it
  > was copied from. The providers' APIs keep no conversation, so each request carries the whole
  > conversation so far. A request continues a conversation when it goes to the same model and its
  > messages begin with an earlier request of that conversation and the reply to it, compared as the
  > provider's format reads them. The comparison leaves out what programs rewrite when they send the
  > history again: hints such as cache markers, the text of earlier tool inputs, which is compared by
  > the tool call's id and name, and messages a program inserts. So a retry, and a request that goes
  > back to an earlier turn, stay in their conversation. Any other request starts a new
  > conversation. When a request names an earlier reply itself, as OpenAI's API and Codex's socket
  > do, that link is followed. A program that shortens a long history, as Claude Code does when it
  > compacts, starts a new conversation by this test. The two are joined, and a budget on the
  > conversation follows the join, only when both requests carry the same session identifier of the
  > program. That join is exact about the identifier only: a program that changes its identifier
  > starts a new conversation, which starts again at zero, as `what-a-conversation-budget-promises`
  > says.

- **Left for later:** a nested `claude -p` started from a tool call sends a new session id (#347).
  Joining it to its parent needs the processes, which A does not have. Its spend is still in the
  computer's total.

## `what-traffic-gives`

**Asks:** What does Silo learn from model traffic?

- **Recommendation:** take with an edit.
- **Confidence:** medium. Read from the providers' documentation (#351), not seen in Silo's traffic.
- **The case:** a Claude Code request uses Anthropic's server-side fallback or its advisor, and a
  second model runs inside the same request. Only `usage.iterations[]` lists it, and the top-level
  `usage` undercounts it. Server tools such as web search are priced per use (#351: $10 per 1,000 at
  Anthropic), and the reply's service tier changes the price.
- **Proposed text** (replacing "the token counts with cache reads and writes"):

  > the token counts with cache reads and writes, including those of every model that ran inside
  > the request, the tools the provider ran itself, and the service tier that answered

## `how-spend-is-shown`

**Asks:** How is spend shown?

- **Recommendation:** take as written.
- **Confidence:** high.
- **The case:** #371 showed Codex's spend as not known, because gpt-5.5 had no price in its table,
  as the answer says. The plan's share comes from response headers: for Codex
  `x-codex-secondary-used-percent`; for Claude `anthropic-ratelimit-unified-7d-utilization`, a name
  known from community captures (#351). The answer says "as the provider reports it", which covers a
  header that moves.
- **For the spec:** prices need a table of Silo's own with a date of effect and a row per service
  tier (#351, recommendation 9). That is a format, not a change to this answer.

## `network-place`

**Asks:** What does the place Network show?

- **Recommendation:** take as written.
- **Confidence:** medium.
- **The case:** Codex reached github.com, api.github.com, chatgpt.com and ab.chatgpt.com at start,
  before any work, and "first time seen" fired on all four (#371). Telling a program's own traffic
  from what the model asked for is an opinion, so it is a classifier for later, not a line of this
  answer.
- **For A:** there is no app, so the place is the `silo network` output. "The runs that reached it"
  is a report that needs processes. In A, only model calls name their conversation, and every other
  connection shows as not attributed, which the answer already allows.

## `what-a-receipt-is`

**Asks:** What is a receipt?

- **Recommendation:** take as written. It sets the word `receipt`.
- **Confidence:** high. polarzero called the receipt the core of what Silo gives. #371 built one
  from this text, and its first screen caught both false claims of the night.
- **For A:** the account is per conversation, since the conversation is what A can see. A run needs
  processes. The answer's "one run, one task or one fork" then has its first case in the second
  build. A conversation's receipt can be read as the run's, said as such, until runs exist.

## `what-a-receipt-holds`

**Asks:** What does a receipt hold?

- **Recommendation:** take with an edit.
- **Confidence:** medium.
- **Two cases from #371:**
  1. Two of four runs left a process running that nothing in their words mentioned
     (`node scripts/sessiond.js`, `bun test --watch`). The list has no item for it.
  2. claude #1 ran the whole suite (exit 1), then the auth tests alone (exit 0), and said «All tests
     pass.». A receipt that shows "the last test command" shows exit 0. The outcome has to be the
     last run of the whole suite.
- **Proposed text** (replacing "the commands that ran, with their exit codes;"):

  > the commands that ran, with their exit codes, and for each test command the last run of the
  > whole suite apart from narrower runs; the processes it started that are still running;

- **For A:** without processes, the exit codes are those the program sent to its model, a report
  read from traffic. "Still running" says that it is not known yet, as the answer's last sentence
  requires.

## `agent-summary-beside-receipt`

**Asks:** May the agent's own summary be shown beside a receipt?

- **Recommendation:** take with an edit.
- **Confidence:** high.
- **The case:** #371's classifier checked two kinds of claim ("tests pass" against the last whole
  suite, "only changed files in the repo" against file facts outside it) and caught both false
  ones. A claim of a third kind, "the API is backwards compatible", it could not check.
- **Proposed text** (added after "A summary can be wrong, or written to mislead."):

  > Where a classifier checks a claim of the summary against the receipt, its result is an opinion,
  > shown as one, and a claim it cannot check is shown as not checked, never as true.

## `conversation-text-kept`

**Asks:** Is the text of conversations kept?

- **Recommendation:** take as written.
- **Confidence:** high.
- **Why:** the receipt's first item is what you asked, word for word, and the claim check reads the
  agent's last words. Both need the text. The audit's own wording is already the answer.

## What the 19 do not settle for A

- `which-process-holds-the-boundary` (in #348's 70) answers "the helper of that computer". #370's
  helper is Swift. The boundary is Zig, as the spike is. The spec below reads "the helper" as the
  launchd job that holds the computer, with the boundary as a second process in it. If polarzero
  reads the answer as one process, it needs an edit.
- `secret-placeholder`, `secrets-held-outside` and every budget stay with the second and third
  builds.
- Whether Silo may read a Claude subscription for anyone but polarzero waits on Anthropic.
