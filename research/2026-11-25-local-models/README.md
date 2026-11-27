# A model on your Mac, reached from main through the boundary

You run `qwen3-coder:30b` in Ollama on your Mac, on its GPU. You want Aider in main to use it.
Today it cannot: `network-modes` says open never includes your device, and #358 and #363 showed why
a computer must not reach the Mac's loopback. #369 left this as a question in the notes of
`program-that-cannot-be-read`. This is a design for it, with a prototype of the part that matters:
what the boundary lets through, what it records, and what it refuses.

Nothing here ran a real Ollama or LM Studio. The servers on the "Mac" are stand-ins that answer in
the shapes #369 recorded and the providers document. Running Ollama in this environment was not
possible.

## The design

A new channel, "a model on your Mac", granted per computer, one server at a time.

1. **You grant it.** In the Silo app: "Let main use Ollama on this Mac". Optionally, only some
   models. It is off by default. Granting is yours; a precaution never does it.
2. **The computer gets one address.** Inside main, `http://models.silo.internal:11434` (or `:1234`
   for LM Studio). Silo's network stack maps that address to the boundary, not to the Mac's
   loopback, so the sealed stack of #363 stays sealed. The computer is known from which stack the
   connection came in on, never from anything the computer sends.
3. **The boundary is the only one that dials the Mac.** It reads each request, and passes only
   those that run or list a model already on the Mac. It sends them to `127.0.0.1:11434` with the
   Host that server expects and no Origin.
4. **Each call is recorded** like any model call: the computer, the model, the route, tokens in and
   out, at no price. Plain HTTP, so no certificate is involved.
5. **A refusal is a 400 in the shape of the route's format**, with Silo's sentence, because that is
   the status every program #369 tried stops on and shows.

What the gate refuses, and why:

| Refused | The case |
|---|---|
| `/api/pull`, `/api/push`, `/api/create`, `/api/copy`, `/api/delete`, `/api/blobs`, and LM Studio's download and load routes | #369 saw Codex ask Ollama to pull a missing model: many gigabytes onto your Mac, chosen by the agent. `push` would upload a model of yours; `create` would make one on your Mac |
| A cloud model of Ollama's (`gpt-oss:120b-cloud`) | It runs on ollama.com under your Mac's own sign-in (#369), so it would leave through your Mac, outside main's rules and budget |
| A model of yours that is a cloud model under another name | `ollama create my-coder` FROM a cloud model has no `-cloud` in its name. The gate asks Ollama's `/api/show`, which names the `remote_host` of such a model |
| A model not on the computer's list, when you set one | |
| Requests read two ways by two parsers | As #358 and #363 found for the boundary |

What it changes, and records that it changed (`no-silent-change-to-traffic`):

- `keep_alive` above 5 minutes, or `-1` (loaded for ever), becomes 5 minutes. An agent can
  otherwise keep a 30 GB model in your Mac's memory all night.
- `options.num_ctx` above 65,536 becomes 65,536. A context of a million tokens would exhaust the
  Mac's memory.
- LM Studio's `ttl`, which keeps a model it loaded on demand, is capped the same way.

## What the prototype measured

[`prototype/gate.py`](prototype/gate.py) is the gate. [`prototype/tests.py`](prototype/tests.py)
sends it what the programs #369 tried send, and what a hostile agent would.
[`prototype/results.txt`](prototype/results.txt): 41 of 41 passed.

- **Tokens counted in all four formats**, streamed and not: Ollama's own lines (Aider, Cline), Chat
  Completions (Goose, Copilot CLI, LM Studio), Responses (Codex `--oss`) and Messages (Claude Code
  pointed at Ollama). Each matched the usage the server reported.
- **One gap: Chat Completions streamed without `stream_options.include_usage`.** The reply carries
  no usage, so the record has none. Silo could add the option to the request, which changes the
  traffic and adds one chunk to the reply, or count the chunks, which is an estimate. Local calls
  have no price, so this only costs the token counts. Not decided here.
- **None of the refused routes reached the Mac**, and each answered 400 in its format's shape.
- **The Mac's server saw `Host: 127.0.0.1:11434` and no `Origin`**, whatever the computer sent.
  Ollama refuses a Host that is not local, as a guard against DNS rebinding; the stand-in does
  the same.
- **Time added per call: about 0.1 ms at the median** on loopback (0.64 ms direct, 0.80 ms through
  the gate, 200 calls each). A model's own time is seconds.

## What this asks of the plan

- **`channel-list`** gets a channel: "a model on your Mac". It goes from a computer to one named
  server on your Mac, through the boundary, and lets through only calls that run or list a model.
- **`network-modes`** stays as it is. Open still never includes your device: the computer does not
  reach the Mac's network, only the boundary's address for this grant.
- **`program-that-cannot-be-read`** gets an answer to the question #369 raised: this channel,
  off by default, per computer, one server at a time.
- **Reading Ollama's own lines** is a fourth reader beside the three provider formats. It is 20
  lines in the prototype, because the usage is in the last line.

## A property this touches

`nothing-shared-from-your-device` is in force: "Nothing from your device is shared into a
computer." A model server on your Mac, reached from main, is something of your device shared into
a computer, however narrow the route. So the channel cannot be built while the property reads as
it does. Two ways, for polarzero:

- Keep the property and leave models on the Mac out.
- Change it to name the exceptions you grant one by one: "Nothing from your device is shared into
  a computer unless you grant it, one channel at a time, and the boundary carries it." This also
  covers ports and files, which already cross by grant (`ports-reached-from-your-mac`,
  `files-imported-from-your-mac`), though in the other direction.

This is a challenge to a statement in force, so it is the owner's: it goes in the notes of
`program-that-cannot-be-read`, and the property stays as it is until polarzero rules.

## Choices for polarzero

Written as notes on `program-that-cannot-be-read`.

- **Build the channel (recommended), or leave models on the Mac out.** Leaving them out means a
  model runs inside the computer on its processor, without the Mac's GPU. A 30B model on a CPU is
  too slow to drive an agent.
- **Any model on the Mac, or a list per computer.** Recommended: any model already on the Mac, with
  a list as an option. Pulling new models stays refused either way.
- **Ollama's cloud models.** Refused (recommended), or let through and counted as a provider at
  Ollama's prices. Letting them through spends your Ollama account from inside a computer, and the
  traffic leaves through your Mac's network, not the computer's.

## What was not verified

- No real Ollama or LM Studio: the routes, error shapes and `remote_host` come from #369's capture
  and the servers' documentation. A run on the Mac with a real model is the next step: the Mac
  test kit could take it.
- LM Studio's `lms link`, where a "local" model runs on another machine: the gate cannot tell such a
  model apart from a local one. What LM Studio's model list says about it was not seen.
- Several computers sharing one model on the Mac: Ollama queues them. A per-computer limit on calls
  at once was not built.
- How Silo's network stack maps `models.silo.internal` to the boundary. The patched gvproxy of
  #363 serves no address on the gateway; this needs one listener per grant, which #363's patch does
  not have yet.

## Running it

```
prototype/run.sh /tmp/out
```

Python 3 only. Output: `results.txt`, the gate's record in `record.jsonl`, and what reached the
"Mac" in `mac.jsonl`.
