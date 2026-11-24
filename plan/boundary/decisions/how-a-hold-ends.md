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
