# How is a held request refused on each kind of connection

## The choice

An agent's part, 22 November 2026. A rule holds every request from main to api.stripe.com, and you
do not answer before the hold's limit. Another rule holds an `ssh` connection from main to your
server.

The options:

- In each connection's own terms (recommended). A web request Silo reads gets the status 403, with
  words that say held and not answered, or denied. The usual programs do not retry a 403 by
  themselves. The `ssh` connection, which Silo does not read, is reset. A name lookup is refused.
  Other traffic with no connection to reset is told the port cannot be reached, so the program stops
  waiting at once. It costs the words on the `ssh` side, since a reset looks like a network fault to
  the agent. Which status and which words each agent program reads best is still to find out.
- As a server error. The request gets the status 503, as from a service that is down. It costs
  repeated asks: programs retry a 503 by themselves, and each retry is held and asks you again.
- In silence. Nothing comes back. It costs the agent's time: it waits for its own timeout, often
  minutes, then tries again.

Recommended: in each connection's own terms. A refusal that programs do not retry by themselves
means one ask per request, and the agent learns at once what happened.

## Notes

Left for when it is built, with what the message of a hold says. What is known:

- Silo can put words in a reply only while the caller still listens and where the reply has room
  for words.
- A raw connection can only be refused.

## Which status each program and tool reads best (24 November 2026)

An agent's part, 24 November 2026. Measured with the same rig as `hold-limit-length` (Claude Code
2.1.331, Codex 0.172.0). There are two kinds of held request, and they behave differently.

**A request from a command inside the computer** (curl, git, npm, pip, node, Python), refused
with 403 and Silo's sentence as the body:

- Each tool sends the request once and stops. None retries a 403. curl prints the body; git, npm,
  pip, node and Python print their own error with the status.
- The agent reads the sentence in the command's output, in both programs.
- With 503 instead, npm sends it 3 times over 70 seconds and pip 6 times over 8 seconds. The answer
  is right that a 5xx asks again.

**A request of the agent program to its own model** (a budget at its limit, a hold on the model's
host) is the other kind. Here "a 403 is not retried by the usual programs" is wrong for Codex:

| Status, JSON body | Claude Code: sends | Claude Code shows | Codex: sends | Codex shows |
| --- | --- | --- | --- | --- |
| 400 | 6, in 0.3 s | `API Error: 400` and the sentence | 1 | the body as it is |
| 402 | 1 | `API Error: 402` and the sentence | 6 | status and sentence |
| 403 | 1 | "Failed to authenticate", then the sentence | 6 | status and sentence |
| 404 | 2 | that the model does not exist | 6 | status and sentence |
| 422 | 6 | `API Error: 422` and the sentence | 6 | status and sentence |
| 429 | 7 or more, with backoff | not seen: still trying | 1 | "exceeded retry limit" |
| 451 | 1 | `API Error: 451` and the sentence | 6 | status and sentence |
| 503 | 11, over 180 s | the sentence, and "try again in a moment" | 30, over 25 s | status and sentence |

- Claude Code's 6 sends on a 400 are not plain retries: it drops a beta field, then its deferred
  tools, as if a gateway had refused them.
- Codex sends 5 more times on every 4xx but 400 and 429.
- Codex shows a plain-text body as it is. A 400 with the sentence as plain text is sent once and
  shown cleanly: "Silo held this request and you did not answer within 90 seconds, so it was not
  sent."
- An error inside a stream that answered 200: Claude Code drops the stream and sends the request
  again without streaming, so the answer to that request is what it shows. Codex takes a
  `response.failed` with a code it knows will not pass (`invalid_prompt`) once and shows its
  message; with any other code it sends 5 more times.

So for its own model requests, no one status works for both programs. The least bad pair: **451
for Claude Code, 400 with a plain-text body for Codex.** Each is sent once and shows Silo's words.
A recipe can carry which one its program reads. Whatever Silo answers, a request the program sends
again must get the same answer at once, without a new hold or a new ask: with a 403, Codex would
otherwise make six asks of 90 seconds each.

How long each program waits on its model before trying again, with nothing coming back:

- Claude Code waits 361 seconds for the head of the reply, then sends the request again. Once the
  reply has started, it waits 300 seconds with nothing new, then sends it again without streaming.
- Codex waited more than 700 seconds for the head of the reply, without giving up. Once the reply
  has started, it waits 300 seconds with nothing new, then sends it again, up to 5 times.

## A refused name lookup (24 November 2026)

An agent's part, 24 November 2026. A name server answered every query for `held.example` with
REFUSED, then with NXDOMAIN (the name does not exist), and the same seven tools looked it up.

- With REFUSED, every tool says the lookup failed for now: "Temporary failure in name resolution",
  `EAI_AGAIN` in node, "Could not resolve host" in curl and git. The system's resolver asks twice,
  so the server got 38 queries against 19 for NXDOMAIN.
- With NXDOMAIN, they say the name is not known. That is not true of a host Silo refuses, and other
  resolvers can keep that answer for a while.
- pip and npm try again either way: pip 6 times in 8 seconds, npm 3 times in 70 seconds.
- Neither answer can carry Silo's words. The agent reads a network fault, and a fault that says
  "temporary" invites it to try again.

A proposal: when a rule refuses or holds a host, Silo answers the lookup with the address of its
boundary, as for any other host, and refuses or holds the request itself. On HTTPS, the boundary
already ends the encrypted connection, so the refusal comes back with Silo's words. A lookup is
refused only when the lookup itself is what a rule holds, such as a name used to carry data out.

## The same refusals for Claude Code on Amazon Bedrock (24 November 2026)

An agent's part, 24 November 2026. Claude Code 2.1.331 on Bedrock (`CLAUDE_CODE_USE_BEDROCK=1`)
against a stand-in that refused each streamed model request with a status and a JSON body
carrying the sentence, as Bedrock words its errors.

| Status (Bedrock's error type) | Requests sent | What the user saw |
|---|---|---|
| 451 (`AccessDeniedException`) | 1 | `API Error: 451` and the sentence, at once |
| 402 (`AccessDeniedException`) | 1 | `API Error: 402` and the sentence, at once |
| 400 (`ValidationException`) | 8, in under a second | `API Error: 400` and the sentence |
| 403 (`AccessDeniedException`) | 8 and counting, retried up to 10 times with waits up to 40 s | nothing yet when the test stopped it at 90 s |
| 429 (`ThrottlingException`) | 8 and counting, the same | nothing yet at 90 s |

So the answer for Claude Code holds on Bedrock: 451 for a hold, 402 for a budget. A 403, which a
proxy would send by habit, is the worst choice there: Claude Code takes it for a passing failure
and keeps asking for minutes.

A hold that answers nothing behaves as on Anthropic's API: Claude Code on Bedrock sent the same
streamed request again after 361 s with no reply head, though the request said it would wait
600 s (`x-stainless-timeout`). So the limit of `hold-limit-length` fits Bedrock as it is.

## A refusal on Codex's WebSocket (24 November 2026)

An agent's part, 24 November 2026. The refusals above were tried on plain requests. Codex 0.172.0
sends its model traffic over a WebSocket when it can (`reading-each-agent-program`). A fake
provider on `api.openai.com`, reached through the system's trust store, refused the first
request of a run in six ways. Each run had no other refusal, so the HTTPS requests Codex fell back
to were answered.

| Refusal on the socket | What Codex showed | What Codex did next |
|---|---|---|
| An event `{"type": "error", "status": 400, "error": {"code", "message"}}` | The event, with Silo's words | Stopped: one refusal, the turn failed |
| The same event with status 403 | "Reconnecting... 2/5 (unexpected status 403 Forbidden: Silo held this request…)" | Five tries, then "Falling back from WebSockets to HTTPS transport", and the request went out over HTTPS |
| `response.failed` with Silo's words | "stream disconnected before completion: Silo held this request…" | The same five tries and fallback |
| A close frame, code 1008, Silo's words as the reason | "websocket closed by server before response.completed" | The same five tries and fallback; the reason was not shown |
| The upgrade refused with 400 and Silo's words | An empty error | Stopped; the words were lost |
| The upgrade refused with 403 | "unexpected status 403 Forbidden: Unknown error" | Five tries and fallback; the words were lost |

Codex's code says why: an `error` event that carries a status is treated as the HTTP answer of
that status (`codex-api/src/endpoint/responses_websocket.rs`), so the rule found for plain
requests holds on the socket too: 400 is the one status Codex shows once and does not retry.

So for Codex on a socket, Silo accepts the connection and refuses the held request with an
`error` event of status 400, carrying the reason in `error.message`. Anything else is sent again
up to five times and then over HTTPS, where the same hold has to refuse it again: six asks for one
request, the case `hold-limit-length` warns of, unless the retries are joined to the open ask.

## Where each tool shows Silo's words (24 November 2026)

An agent's part, 24 November 2026. The note above found that git, npm and pip print their own
error on a 403, not Silo's sentence. A fake server refused git 2.43, npm 10.9 and pip 24.0 in four
ways, to find a refusal each one shows:

| Refusal | git | npm | pip |
|---|---|---|---|
| 403, the sentence as `text/plain` | `remote: <sentence>`, then its own error | its own error only | "No matching distribution found", nothing else |
| 403, JSON `{"error": <sentence>}` | its own error only | `403 Forbidden - GET <url> - <sentence>` | the same as above |
| 403 with the sentence as the status line's reason | its own error only | `403 <sentence> - GET <url>` | the same as above |
| The package's index answered, its file refused with 403 and the sentence as the reason | | | `HTTP error 403 Client Error: <sentence> for url: …` |

So one refusal can carry the sentence to all three: status 403, the sentence as the reason of the
status line and as a `text/plain` body. Tried together: git showed the body and npm the reason. pip shows it only when
the refused request is a file and not the index page: a refused index page reads as a package that
does not exist, which sends an agent looking in the wrong place. For a hold on a Python package,
Silo answers the index and holds the file. A reason phrase does not exist in HTTP/2, so this holds
where the tool speaks HTTP/1.1, as these three do.

## Words on a connection Silo does not read (24 November 2026)

An agent's part, 24 November 2026. The answer resets an `ssh` connection and "costs the words".
`refusals-that-say-so` leaves open how the answer is given on a connection that is not HTTP. Tried
on Linux: a small server stood in for Silo, answered one connection in the protocol's own first
messages with a sentence, and closed. No key, certificate or password of the real host is needed
for any of these, because each protocol lets a server refuse before it proves who it is.

| Protocol | What Silo sends | What the program shows |
|---|---|---|
| SSH | its version line, then a "disconnect" message in clear, before any key exchange | OpenSSH 9.6: `Received disconnect from …: <words>`. git (clone, ls-remote) shows that line above its own "Could not read from remote repository". Node ssh2 1.17 and asyncssh 2.24 give the words as the error. paramiko 5.1 says "Negotiation failed" and logs the words only at INFO |
| SSH | a text line before the version line | nothing without `-v` |
| SSH | a reset, as the answer says | "Connection reset by peer" |
| PostgreSQL | "no TLS" to the client's TLS request, then an error | psql 16, psycopg 3.3 and Node pg 8.23 show `FATAL: <words>`. With TLS required (`sslmode=require`, as hosted databases give), the client stops at "server does not support SSL" |
| PostgreSQL, TLS required | "yes" to TLS with Silo's own certificate, then the error inside | `require` shows the words: it encrypts without checking the certificate. `verify-full`, and Node pg with `ssl: true`, show them only where Silo's authority is trusted; otherwise "self-signed certificate" |
| MySQL | an error packet as the first packet, as MySQL does for a host it refuses | mysql2 3.24 and PyMySQL 1.2 show the words, with the code 1130 |
| Redis | `-ERR <words>` to the first command | redis-cli 7.0 and redis-py 8.2 show the words. ioredis 6.1 shows them only when the connection stays open and every command gets the error; node-redis 6.4 shows them in its error events but reconnects forever |

What I would put in the answer's place: a refused connection that Silo does not read gets the
refusal in its protocol's first messages where Silo knows the protocol, and a reset only where it
does not. That covers SSH, the case the answer names, with no host key: an agent whose `git push`
over SSH is refused then reads why, instead of a network fault it retries. Silo knows the protocol
by the port, and for PostgreSQL also by the client's first bytes. MySQL's TLS starts after the
server's first packet, so its refusal needs no certificate. A protocol under TLS from its first
byte, such as Redis with `rediss://`, gets the words only through Silo's certificate, as HTTPS
does. The scripts are in the project files,
`research/2026-11-24-holds/protocols`.

## A refused HTTP/3 connection, in a browser (24 November 2026)

An agent's part, 24 November 2026. The answer leaves open "how each takes the answer on UDP". Of
the programs tried today, only a browser sends QUIC: Claude Code, Codex, curl as Ubuntu builds it,
and the Python and Node libraries all use TCP. Playwright's Chromium 158 loaded a test
host over TCP, got `Alt-Svc: h3=":443"` in the reply, and loaded it five more times.

- **Port unreachable**, the answer's choice: Chromium sent one QUIC packet, got the ICMP answer and
  stayed on TCP. Every page loaded, in the same 10 to 15 ms as with no QUIC at all.
- **Silence**: three QUIC packets within 35 ms, then nothing more; the pages loaded in the same time, since
  Chromium starts TCP alongside QUIC instead of waiting for it.
- **An authority added on the computer is refused for QUIC.** With the test authority in Chromium's
  store, the QUIC handshake reached the test server and Chromium ended it with
  `ERR_QUIC_CERT_ROOT_NOT_KNOWN`: it accepts that authority over TCP and not over QUIC. So a
  connection a browser makes over QUIC can never carry Silo's certificate, which agrees with the
  answer of `ending-the-encrypted-connection`: a host Silo reads is reached over HTTP/3 only if
  Silo passes the UDP unread, which the answer does not do.

Not tried: a browser profile that had already used HTTP/3 to the host, where Chromium may wait a
little for QUIC before starting TCP; a real network delay (the test ran on one machine); Firefox.
The scripts are in the project files, `research/2026-11-24-quic`.

## How each agent program takes a 403 and a 429 from a model host (24 November 2026)

An agent's part, 24 November 2026. A stand-in for each program's model host answered every request
with 403, then with 429, with the body "Silo: this computer's budget is reached. Raise it in the
Silo app." in the shape every provider uses for an error. Each run lasted 60 seconds, on Linux. The
research is in `research/2026-11-24-every-agent-program`.

| Program | 403 | 429 |
|---|---|---|
| Claude Code 2.1.331 | retried, 7 requests in 60 s; nothing shown yet | the same |
| Codex 0.172.0 | 14 requests in 13 s, then stopped and showed the body | 2 requests, stopped, body not shown |
| Gemini CLI 0.71.1 | 4 requests in 16 s, body shown | 5 in 20 s, body shown |
| Qwen Code 0.28.4 | retried, 22 requests in 60 s; nothing shown | the same |
| Cline 3.0.75 | 1 request, body shown | 4 in 14 s, body shown |
| OpenCode 1.19.6 | retried, 6 requests in 33 s; nothing shown | the same |
| Goose 1.60.2 | 1 request, body shown | 7 in 7 s, body shown |
| Aider 0.86.2 | retried, 8 in 32 s, body shown each time | the same |

So the answer's reason does not hold: a 403 is retried by Claude Code, Qwen Code, OpenCode and
Aider, with growing waits. Only Cline and Goose stop at the first one. No status tried made every
program stop and show Silo's words. What the user reads has to come from Silo's own app, and the
record has to expect the same refused request several times.

## A hold the programs wait for, and a refusal they show (24 November 2026)

An agent's part, 24 November 2026, from the research in `research/2026-11-24-agent-programs-follow-ups`.

**While a hold lasts.** The 15 s of Codex and the 60 s of Gemini CLI in the section above are limits
on the first byte of a reply. When the status and headers were sent at once and the reply held,
Gemini CLI waited the whole 300 s, and Codex waited 300 s before sending the request again. With a byte every 5 seconds that the format documents and
that means nothing, every program tried waited the whole hold and took the reply: Anthropic's
`ping` event on Messages (Claude Code, OpenCode, Cline, Goose, Aider, Copilot CLI, 300 s), a
comment on Gemini's stream (300 s). Codex needs an event, not a comment: it waited 420 s on an
event of a name it does not know. Such an event broke OpenCode and Cline on Messages, so it is not
a general answer, and the Responses format documents no harmless event.

In a real hold the provider's reply has already started, so the headers have already gone to the
program. What a hold has to cover is the wait after them, and the measures say a documented
keep-alive byte covers it for every program but Codex, which needs an event on Responses.

**When a hold ends with a refusal.** This entry says a 403 is not retried by the usual programs.
Measured with a refusal of the model requests only: a 403 in the provider's error shape stopped
seven of eight programs, but Aider retried it and Copilot replaced Silo's words with its own. A 400
in the provider's error shape (`invalid_request_error`) stopped all eight within 9 seconds, and each
printed Silo's sentence. The table is in the notes of `budget-denies-or-holds`. So I would make the
answer 400 rather than 403, with Silo's words in the provider's error shape.
