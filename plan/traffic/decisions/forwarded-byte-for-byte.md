# What the boundary forwards between an agent program and its provider

## The choice

An agent's part, 22 November 2026. Claude Code in main sends Anthropic a conversation of 100 000
tokens on every turn, with your Max plan's sign-in in each request.

The options:

- Unchanged, and the sign-in kept nowhere (recommended). Each request and answer arrives as it was
  written, apart from the changes Silo records, so Anthropic's cache works as it does without Silo.
  Your sign-in passes and is written nowhere. It costs a record that cannot replay a request
  exactly, since it lacks the sign-in. Anthropic can also tell that something other than Claude Code
  made the connection, as with a company's proxy.
- A full copy, sign-in included, kept encrypted. You could replay or inspect any request whole. It
  costs a copy of your login on disk, and Silo then stores a subscription's sign-in, which
  Anthropic's terms forbid to a developer.

Recommended: unchanged, and the sign-in kept nowhere. Your bill and your agents behave as they do
without Silo, and nothing of your login stays behind.

## Notes

"The changes listed above" are those of `what-silo-changes-in-traffic`.

That Silo's own boundary forwards byte for byte is still to find out: `reading-each-agent-program`.

## A spike of the boundary, measured (24 November 2026)

An agent's part, 24 November 2026. A spike of the boundary for model traffic was written in Zig,
about 1 000 lines, with OpenSSL, zlib and zstd through their C interfaces. It is a trial and is kept
outside this repository. It does what `ending-the-encrypted-connection` describes:

- It ends the program's TLS connection and knows the host by the name in the handshake.
- It opens its own connection to the host and checks the host's certificate against its own store.
- It forwards each request and reply as it came.
- It reads a copy: decompressed, chunks joined, events parsed.

It ran on Linux, in front of fake providers.

**Byte for byte.** A test client sent 100 requests through the spike, each compressed with gzip,
from 1 KB to 2 MB. The host received every one of them as the same bytes. Every reply reached the
client as the same bytes too, whether chunked, framed by a length, or compressed with gzip.
Claude Code 2.1.331 and Codex 0.172.0 also ran whole conversations through it.

**What it costs in time**, as 20 requests each, median then 95th percentile, against the same fake
host reached directly:

| Reply | First byte, direct | First byte, through the spike | Whole reply, direct | Whole reply, through the spike |
|---|---|---|---|---|
| 200 text pieces, 5 ms apart | 5.8 / 6.1 ms | 5.8 / 6.3 ms | 1081 / 1087 ms | 1080 / 1094 ms |
| A tool call, 5 ms between pieces | 5.5 / 5.8 ms | 5.5 / 5.8 ms | 480 / 486 ms | 479 / 485 ms |
| The same, compressed with gzip | 5.9 / 6.3 ms | 5.9 / 6.5 ms | 484 / 486 ms | 485 / 492 ms |
| 650 KB at once | 13.3 / 22.7 ms | 13.7 / 23.4 ms | 228 / 285 ms | 233 / 301 ms |
| A 2 MB request, short reply | 22.5 / 29.7 ms | 58.3 / 75.0 ms | | |

- **Nagle's algorithm has to be turned off**, on both connections. Without `TCP_NODELAY`, the spike
  added 37 to 40 ms to every first byte: the head and the body went out as two small writes, and
  the second waited for an acknowledgement that the other side delays. With it, the cost of reading
  a stream disappeared into the noise.
- **A large request costs its reading.** The spike reads a request whole before forwarding it, so
  that it can see the tool results it carries (see `denied-tool-call-mechanism`). For 2 MB that was
  34 ms: decompressing, parsing the JSON, hashing. Forwarding while reading would remove most of
  it. Only the next reply has to wait for what the request says, not the request itself.

Not done in the spike: HTTP/2, the WebSocket that Codex uses with a ChatGPT sign-in (it was
forwarded unread), a certificate made for each host (one certificate covered all three names), and
refusing HTTP/3.

## A request sent while you type (24 November 2026)

An agent's part, 24 November 2026. Read from Claude Code 2.1.331's code, not seen in traffic: it
sits behind a switch of Anthropic's, in a "shadow" or an "on" mode. Claude Code can start the next
model request while you are still typing ("upload ahead"). It opens the request and sends the
conversation so far, gzip-compressed, then writes a space into the compressed stream every 10
seconds to keep it open. When you press Enter it sends the rest of the body, your new message, and
the request ends. It drops the request if you stop typing for 30 seconds or after 120 seconds in
all, and if no answer comes in time it sends the request again whole.

For the boundary: a request's body can take up to two minutes to arrive, a few bytes at a time,
and only its end says what it asks. The boundary must not time out such a request, and a hold or a
rule on its content starts when the body ends, not when the request opens. A request dropped this
way gets no reply and costs nothing, but it shows in the record as a request that never ended.

## gRPC needs HTTP/2 at the boundary (24 November 2026)

An agent's part, 24 November 2026. The spike left HTTP/2 out. Most traffic does not need it:
Claude Code's model requests use HTTP/1.1, and clients that are offered only HTTP/1.1 use it. gRPC
does not. A TLS server that offered only HTTP/1.1, or nothing, got the HTTP/2 opening
(`PRI * HTTP/2.0`) from grpcio 1.85 and from @grpc/grpc-js 1.14 all the same, and neither client
offers to fall back.

gRPC is what Google Cloud's client libraries (Firestore, Pub/Sub, Spanner) and OpenTelemetry's
default exporter use, often with a key in the request's headers (Honeycomb's `x-honeycomb-team`,
a Google token). So a host Silo reads, to swap a key or hold a request, must be read over HTTP/2
when the client asks for it, with its trailers, which gRPC uses to say how a call ended. Until
then such hosts can only pass unread, and their keys stay inside the computer. The server is in
the project files, `research/2026-11-24-trust-libraries/alpnsrv.py`.

## A reply cut in the middle (24 November 2026)

An agent's part, 24 November 2026. Silo's boundary sits in the middle of each model reply, so a
restart of the boundary, an update of Silo for example, cuts every reply in flight. Tried by
cutting a streamed reply after its first events, once with a clean close and once with a reset,
against Claude Code 2.1.331 and Codex 0.172.0 (over HTTPS, not Codex's WebSocket).

- Both programs sent the same request again within a second, got the whole reply, and showed the
  answer as if nothing had happened. The agent did not notice.
- The provider still did the work of the cut reply. So each cut costs what the model had written
  up to the cut, paid for and thrown away, and a budget has to count both requests.
- A reply that stops without being closed is different: the earlier measure found that both
  programs wait 300 s before sending again.

What this means for Silo: a restart of the boundary is cheap for the agent if it closes or resets
its connections, never if it leaves them open. Cutting a reply is no way to refuse one either,
since the program sends the request again at once. The scripts are in the project files,
`research/2026-11-24-reply-cut`.
