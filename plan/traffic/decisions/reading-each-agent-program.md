# Can the boundary read the model traffic of each agent program

## The choice

An agent's part, 22 November 2026. You want to work with Claude Code and Codex in main. On 19
November an ordinary inspecting proxy, standing in for Silo, read Claude Code 2.1.327 on a Claude
subscription and Codex 0.170.0 on a ChatGPT sign-in. Silo itself has not been tried yet, nor signing
in from scratch through it.

The options:

- Shown as read only after its check (recommended). For each program, and each new version a recipe
  gives it, a test through Silo shows four things: the program accepts Silo's certificate, each way
  it talks to its model is read, signing in and renewing a sign-in work, and its requests leave
  unchanged. A program whose model Silo cannot read still runs, and the app says that its
  conversations, spend and budgets are not covered. A program that refuses Silo's certificate cannot
  reach its model through Silo, so Silo does not offer it and says why. It costs a wait before a new
  program or version is read.
- Read on trust. The first test and the vendors' support for company proxies are taken as enough for
  every program and version. It costs silent gaps: a version that moves to a new host or a new way
  of talking to its model stops being read, while the app still shows its spend as complete.

Recommended: shown as read only after its check. Spend, budgets and denied tool calls are worth only
what Silo really reads, so what the app calls read has to have been seen read.

## Notes

To find out. A first test on 19 November 2026 says yes for Claude Code and Codex.

What has to be checked is narrow: that each agent program accepts a certificate from Silo. Companies inspect traffic this way, and agent programs offer a setting for a company certificate, so this is expected to work. It is a test to run once per program.

## For each agent program and provider

- Does the program accept Silo's certificate, or does it refuse the connection?
- Which transport does it use (plain requests, a stream, a persistent socket), and can each be read?
- Does the sign-in flow of a subscription still work?
- Does reading leave requests unchanged byte for byte?

## What a first test found (19 November 2026)

Claude Code 2.1.327 under a Claude subscription and Codex 0.170.0 under a ChatGPT sign-in were each run once on a Mac, behind an ordinary inspecting proxy that stood in for the boundary.

- Both accept the certificate, and both refuse to connect without it. Neither falls back to another route, so there is no unread way through.
- Claude Code sends plain requests and receives a stream. Codex holds a persistent socket for the model and sends plain requests for the rest. All of it was read: the model that answered, the token counts, the tool calls.
- Both providers answered normally to a connection the proxy had opened. One short run cannot show what happens over days or under heavy use.
- Neither program hands out its encryption keys. So reading without ending the connection is not possible, and the boundary ends it, as planned.

## Still to find out

Signing in from scratch and the renewal of a token through the boundary; trust through the computer's trust store alone, with no setting in the program; that Silo's own boundary forwards byte for byte; other agent programs.

## What the vendors' terms say (read on 19 November 2026)

- Anthropic documents the set-up: "Enterprise TLS-inspection proxies work without additional configuration when their root certificate is installed in the OS trust store". Its terms do not prevent "an end user from signing in to the unmodified Claude Code binary with their own Claude subscription, including where a platform hosts Claude Code".
- OpenAI's terms, read in full, have no clause on reading one's own traffic. Its documentation gives Codex a setting for "a corporate TLS proxy or private root CA". The nearest clauses forbid sharing an account's credentials, extracting output automatically, and trying to extract the model's protected reasoning.

## The research of 19 November 2026, in detail

The sections above are its summary, and this is what they rest on. The report, the researchers'
notes, the saved copies of the terms and the test's summary files are kept outside this
repository.

### The test

An ordinary inspecting proxy, mitmproxy 12.3.1, stood in for the boundary on a Mac, with
polarzero's own sign-ins. Each program was told to trust the proxy's certificate for one command.
Nothing was added to the system's trust store. The proxy, its certificate and the raw output were
deleted afterwards. The summary files that were kept hold, per request, header names, sizes,
hashes and counts. They hold no sign-in and no conversation text.

The proxy is not Silo's boundary. The table says what a proxy in that place could read.

| | Claude Code 2.1.327, Claude subscription | Codex 0.170.0, ChatGPT sign-in |
|---|---|---|
| Certificate trusted | Works. One tool call, one answer. | Works. One tool call, one answer. |
| Certificate not trusted | Refuses: "SSL certificate verification failed". 16 tries, none passed. | Refuses: "Reconnecting... waiting for network". 53 tries, none passed. |
| How model traffic travels | HTTP/1.1, the answer as a stream of events | A persistent socket (WebSocket). Its other calls use HTTP/2. |
| Read by the proxy | The model that answered, four token counts, the tool call, a request id, the rate-limit headers | The model that answered, token counts, the tool call, rate-limit events |
| Hosts | `api.anthropic.com`, and Datadog for its telemetry | `chatgpt.com`, `ab.chatgpt.com` for its telemetry, `github.com` |
| Provider's reaction | Normal answers, status 200, no challenge | Normal answers, no challenge |
| Writes its encryption keys to a file when asked | No | The file is created and stays empty |

One side connection of Codex, to `github.com`, refused the certificate. The likely cause is a
helper that does not read Codex's certificate setting. This was not confirmed. Whether the
certificate in the system's trust store is enough for it is to test in a real computer.

Also not tested: whether a provider reacts over days or under heavy use, the comparison with the
program's own account for Codex (`own-account-matches`), and the desktop apps.

### What Anthropic's pages say

Read directly on 19 November 2026. The pages show no date.

- Claude Code trusts the system's certificates: "By default, Claude Code trusts both its bundled
  Mozilla CA certificates and your operating system's certificate store."
  (code.claude.com/docs/en/network-config)
- Anthropic describes a subscription sign-in that passes through a gateway: "Requests still
  route through the gateway, but a saved claude.ai login remains the active credential, so its
  usage limits and billing apply. Gateways that pass this traffic on to Anthropic must forward
  `Authorization` and the OAuth capability in `anthropic-beta`." The same page says: "Anthropic
  doesn't endorse, maintain, or audit third-party gateway products".
  (code.claude.com/docs/en/llm-gateway)
- What is forbidden: "Anthropic does not permit third-party developers to offer Claude.ai login
  into their own applications, or to route requests through Free, Pro, Max, or Ultra plan
  credentials on behalf of their users. Moreover, developers may not collect, store, or
  intermediate Claude.ai credentials or session tokens — sign-in to a Claude account must complete
  through Anthropic's own flow." (code.claude.com/docs/en/legal-and-compliance) How Anthropic
  reads "intermediate" is the decision `anthropic-reads-intermediate`.
- An API key held as a secret is allowed. The rule "does not restrict how customers provision and
  manage their own API keys [...] for example, configuring an API key in a development
  environment, secrets manager, or machine image". (same page)
- The rule for a product that preinstalls or runs Claude Code: "Preinstalling or running Claude
  Code in your products or services (e.g. in hosted sandboxes or other agent infrastructure)
  requires agreeing to our Commercial Terms of Service". The program "must not be modified", and
  the product "may not remove, disable, or restrict any authentication method built into it". It
  "may not pay for, resell, or intermediate Claude usage" for its users. (same page) The decisions
  are `unmodified-agent-programs`, `logins` and `recipe-under-preinstall-rule`.
- Anthropic "reserves the right to take measures to enforce these restrictions and may do so
  without prior notice".
- The Commercial Terms (effective 17 June 2025) and the Service Specific Terms (effective
  31 August 2026) were read in full. They do not mention Claude Code, recipes or preinstalling.
  They forbid to "reverse engineer or duplicate the Services". Nothing in Silo takes Claude Code
  apart: the boundary reads requests and answers as they pass. Whether reading them counts under
  that clause was not asked of Anthropic.

### What OpenAI's texts say

Read on 19 November 2026, from the English pages.

| Text | Date on the page |
|---|---|
| Terms of use (rest of world) | effective 1 January 2026 |
| Europe terms of use | updated 16 January 2026 |
| Service terms | updated 13 November 2026 |
| Usage policies | effective 20 November 2026 |
| Services agreement (business) | effective 1 January 2026 |

- No clause mentions reading, intercepting or proxying your own traffic. The words "intercept" and
  "proxy" appear in none of the five texts.
- The nearest clauses are in the terms of use. "You may not share your account credentials or make
  your account available to anyone else". You may not "Automatically or programmatically extract
  data or Output". You may not "Interfere with or disrupt our Services, including circumvent any
  rate limits or restrictions or bypass any protective measures or safety mitigations".
- The usage policies add: "You must not attempt to extract protected reasoning through
  unauthorized access, reverse engineering, or circumvention of safeguards." The decision is
  `hidden-reasoning-stays-closed`.
- The part of the service terms on "Sign in with ChatGPT" addresses a developer who uses that
  sign-in "in your services". In Silo the user signs in to Codex itself. The separate "Sign in
  with ChatGPT Terms" were not read in full, so whether they bear on Silo is to confirm.
- OpenAI's documentation: "If your network uses a corporate TLS proxy or private root CA, set
  `CODEX_CA_CERTIFICATE` to a PEM bundle before logging in" (learn.chatgpt.com/docs/auth, read by
  a researcher). Codex is open source under Apache-2.0.
- No rule was found for running Codex inside another product.

### Docker Sandboxes is not taken as a precedent

Docker's documentation, read directly (docs.docker.com/ai/sandboxes/security/credentials), says of
a Claude subscription: "You sign in on the host, and the proxy handles token refresh and routing."
Its proxy "looks up the matching credential on the host, and overwrites the auth header before
forwarding". No public agreement between Docker and Anthropic was found, and Anthropic's rule
begins with "Unless we've mutually agreed otherwise". So this shows what one large vendor ships,
not what the terms allow. Silo's path does not lean on it and does less: the boundary keeps no
sign-in and injects none.

### What other products do

- Anthropic's rule on preinstalling reached its legal page about 21 August 2026, by the page's
  own date. An archived copy of the page of 16 August lacks it, and one of 30 August has it. The
  same edit added the sentence
  that lets a user sign in with a subscription "including where a platform hosts Claude Code".
- Anthropic's own documentation recommends a virtual machine: "A dedicated virtual machine
  provides the strongest separation [...] Options include cloud instances, local hypervisors, and
  microVMs". It names Docker Sandboxes for it. That page states no conditions.
  (code.claude.com/docs/en/sandbox-environments, read directly)
- Shipping Claude Code already installed is common, and the user signs in inside. Daytona: "The
  default Daytona snapshot includes Claude Code". Conductor, an app on the user's own Mac:
  "Conductor comes with Claude Code preinstalled". Both were read directly. The researchers found
  the same for E2B, Fly's Sprites, Blaxel, Namespace and Docker.
- On the pages that were read, their documentation on Claude Code, none of them mentions the
  rule, the Commercial Terms or an agreement with Anthropic. Other pages of theirs were not
  searched.
- From the researchers, not read directly: what Anthropic acted against in 2026 was tools that
  used a subscription's sign-in in a client of their own, in place of the real Claude Code.
- From the researchers, not read directly: Homebrew's package downloads Claude Code from
  Anthropic and hands out no copy, and the Nix package set marks Claude Code's licence as unfree.

Silo's path is the common one, and it does less than some of these products. A recipe fetches
Claude Code from Anthropic and Silo ships no copy (`claude-code-fetched-by-recipe`). The boundary
forwards the sign-in inside each request and stores it nowhere (`logins`).

### How sure this is

- Read directly: Anthropic's legal, network and gateway pages, its Commercial Terms and Service
  Specific Terms in full, Claude Code's licence, Docker's page on credentials and its page on
  Claude Code, OpenAI's five texts, and every test result.
- From the researchers, some of it only from search summaries: field reports, the history of
  Anthropic's enforcement in 2026, the gateway products.
- Not read in full: Anthropic's Consumer Terms and Usage Policy, the "Sign in with ChatGPT Terms",
  and OpenAI's help-centre articles.
- This is a reading of the terms, not legal advice. No lawyer has read it, and neither vendor was
  asked.

## Trust from the system's store alone, on Linux (24 November 2026)

An agent's part, 24 November 2026. This answers one of the points still to find out above, on
Linux only.

A test authority was made, and a certificate signed by it for `api.anthropic.com`,
`api.openai.com` and `chatgpt.com`. A fake provider presented it on port 443 of the same machine,
and `/etc/hosts` sent those names there, so each program reached it as it would reach a boundary
that ends the connection. Each program ran with an empty environment apart from its API key, so
no setting of the machine or of this session told it about the authority. Ubuntu 24.04, Claude
Code 2.1.331, Codex 0.172.0 with an API key.

| Where the authority was | Claude Code | Codex |
|---|---|---|
| Nowhere | Refuses: "Self-signed certificate detected (SELF_SIGNED_CERT_IN_CHAIN) … set NODE_EXTRA_CA_CERTS to that CA bundle, or add it to the system certificate store" | Refuses: its WebSocket fails at the stage `Tls`, and it keeps retrying |
| Only in the system's store (`update-ca-certificates`) | Works: one tool call, one answer | Works: one tool call, one answer |
| Only in the program's setting (`NODE_EXTRA_CA_CERTS`, `CODEX_CA_CERTIFICATE` or `SSL_CERT_FILE`) | Works | Works, with either setting |

So on Linux the computer's trust store is enough for both programs, and no setting in the program
is needed. The Mac, and Codex signed in with ChatGPT, are still to test.

Three more things were seen in the same runs:

- **Claude Code compresses its requests.** To `api.anthropic.com` it sent each request body
  compressed with gzip (`content-encoding: gzip`). To a provider address set by
  `ANTHROPIC_BASE_URL` it did not. The boundary reads a decompressed copy and forwards the
  compressed bytes, as `forwarded-byte-for-byte` already says.
- **Codex falls back from its WebSocket.** With an API key, Codex 0.172.0 first opened a WebSocket
  to `api.openai.com/v1/responses`. When that failed (the fake provider did not speak WebSocket),
  it tried seven times and then sent plain requests with a stream of events. So a boundary that
  refused the WebSocket would still read Codex, through the fallback. Whether the fallback stays in
  later versions is not known.
- **Claude Code calls two more addresses on the same host** before its first model request:
  `GET /api/claude_code/settings` and `GET /api/claude_code/policy_limits`.

## Codex's WebSocket, read through a spike of the boundary (24 November 2026)

An agent's part, 24 November 2026. The spike of the boundary described in `forwarded-byte-for-byte`
was taught WebSocket: frames, masks, fragmented messages and compression (permessage-deflate,
with its state kept from message to message). It was then put in front of a fake provider that
speaks WebSocket, with and without compression. Codex 0.172.0 used it on its own: with an API key,
it asked for compression and opened `wss://api.openai.com/v1/responses`.

- The spike read every message both ways, with and without compression. It held a reply's frames
  while a tool call was open, and a denied `exec_command` failed through the probe as over plain
  requests. The next call ran.
- Each message from Codex is a `response.create` carrying what a plain request's body would. Each
  message from the host is one event, the same JSON as an event of the stream.
- **On a socket, Codex sends only what is new.** Its first message is a warm-up with
  `"generate": false` and no input. Its second carries the whole conversation and names the
  warm-up's reply in `previous_response_id`. After that, each message carries only the new items,
  such as the result of the last call, and names the previous reply. This held with
  `"store": false`. A conversation resumed later opened a new socket and sent the whole
  conversation again.

So for Codex on a socket, the boundary cannot read a conversation from one message. It has to
follow `previous_response_id` from reply to reply within the connection, as
`recognising-a-conversation` already allows. Whether OpenAI's own server accepts compression, and
whether Codex signed in with ChatGPT behaves the same, is not known.

## Claude Code on Amazon Bedrock (24 November 2026)

An agent's part, 24 November 2026. Claude Code 2.1.331 with `CLAUDE_CODE_USE_BEDROCK=1`, against a
stand-in for Bedrock. What the boundary reads differs from Anthropic's API in four ways:

- The address names the model: `POST /model/<model id>/invoke-with-response-stream` for the
  conversation, `/invoke` for short calls. The body is Anthropic's request with
  `anthropic_version: "bedrock-2023-05-31"`, and the betas move from a header into the body
  (`anthropic_beta`).
- The body is sent uncompressed, since its hash is part of the signature
  (`x-amz-content-sha256`).
- The reply is not a stream of server-sent events. It is AWS's binary event stream
  (`application/vnd.amazon.eventstream`): frames with a length, headers and a CRC, each carrying a
  JSON object whose `bytes` holds one of Anthropic's events, base64-encoded. The reader needs this
  framing too, and the probe of `denied-tool-call-mechanism` must see a tool call inside it.
- At start, Claude Code lists the region's inference profiles and sends a short request to three
  models before the conversation begins. `x-claude-code-session-id` is sent here too.

The scripts are in the project files, `research/2026-11-24-bedrock`.
