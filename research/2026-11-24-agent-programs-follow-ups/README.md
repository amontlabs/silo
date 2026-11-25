# Agent programs: the follow-ups of the first survey (24 November 2026)

An agent's research, 24 November 2026. It builds on
[`../2026-11-24-every-agent-program`](../2026-11-24-every-agent-program/README.md), which ran nine
agent programs through the boundary and left five questions open. This page answers them:

1. What refusal makes each program stop and show Silo's words, for "the budget is reached"?
2. Can a hold outlast the programs' limits on a silent reply (Codex 15 s, Gemini CLI 60 s)?
3. Can the probe deny a call without breaking OpenCode's own `git` snapshot?
4. Local models: Ollama and LM Studio, on the computer, on the Mac, and Ollama's cloud models.
5. GitHub Copilot's own route and Kiro, as far as their model calls.

Each finding is copied into the notes of the plan's entry it bears on, listed at the end.

Nothing here used a real account: every key was a fake and every provider a stand-in on the same
machine. The scripts are in [`prototype/`](prototype/).

## How it was run

Each run is one program in a network and mount namespace of its own (`prototype/run.sh`). In it, a
fake hub (`prototype/fakehub.py`) answers every name the program looks up with 127.0.0.1, makes a
certificate for whatever host the program asks for, signed by a test authority, and plays every
provider at once: Anthropic Messages, OpenAI Chat Completions and Responses, Google's
`generateContent`, Ollama's `/api/chat`. It finds the program's shell tool in the request, asks for
one call, then ends. The test authority is put in the namespace's copy of the system's store, so
nothing outside the namespace is changed. The stand-in probe is the one of
`denied-tool-call-mechanism`, extended (`prototype/sprobe-argv.c`).

| Program | Version | With |
|---|---|---|
| Claude Code | 2.1.331 | an Anthropic key |
| Codex | 0.172.0 | an OpenAI key |
| Gemini CLI | 0.71.1 | a Gemini key, `NODE_USE_SYSTEM_CA=1` |
| OpenCode | 1.19.6 | an Anthropic key |
| Cline | 3.0.75 | an Anthropic key |
| Aider | 0.86.2 | an Anthropic key, `SSL_CERT_FILE` |
| Goose | 1.60.2 | an Anthropic key |
| GitHub Copilot CLI | 1.0.115 | its own key mode (`COPILOT_PROVIDER_*`) with an Anthropic key, and a fake GitHub token |
| Kiro CLI | 3.6.0 | a fake `KIRO_API_KEY`; installed from its release archive, in isolation |
| Ollama | 0.40.18 | its own server, in the namespace |
| LM Studio | `llmster` 0.1.1-2 | its headless server, in the namespace |

## 1. A refusal every program stops on

The fake hub refused every model request, and only those, with one status, in each provider's own
error shape, with the message "Silo: this computer's budget is reached. Raise it in the Silo app."
Each cell gives the model requests the program sent, `+` when it was still trying at the 45 s
limit, whether Silo's sentence reached the user (✓ or ✗), and how long the program ran.

| Program | 400 | 401 | 402 | 403 | 404 | 413 | 429, `retry-after: 86400` | 451 | 500 |
|---|---|---|---|---|---|---|---|---|---|
| Claude Code | 2 ✓ 1 s | 7+ ✗ | 1 ✓ 1 s | 2 ✓ 2 s | 2 ✗ | 1 ✗ | 1 ✓ 1 s | 1 ✓ 2 s | 7+ ✗ |
| Codex | 1 ✓ 7 s | 6 ✓ 14 s | 6 ✓ 14 s | 6 ✓ 14 s | 6 ✓ 14 s | 6 ✓ 14 s | 1+ ✗ | 6 ✓ 15 s | 30 ✗ |
| Gemini CLI | 2 ✓ 2 s | 2 ✓ 3 s | 2 ✓ 3 s | 2 ✓ 3 s | 2 ✓ 4 s | 2 ✓ 4 s | 5 ✓ 25 s | 2 ✓ 4 s | 5 ✓ 23 s |
| OpenCode | 2 ✓ 3 s | 2 ✓ 3 s | 2 ✓ 3 s | 2 ✓ 3 s | 2 ✓ 4 s | 3 ✓ 4 s | 4+ ✗ | 2 ✓ 4 s | 8+ ✗ |
| Cline | 1 ✓ 2 s | 1 ✓ 3 s | 1 ✓ 3 s | 1 ✓ 3 s | 1 ✓ 3 s | 1 ✓ 3 s | 4 ✓ 17 s | 1 ✓ 5 s | 4 ✓ 16 s |
| Aider | 1 ✓ 4 s | 1 ✓ 4 s | 8+ ✓ | 8+ ✓ | 1 ✓ 5 s | 1 ✓ 5 s | 8+ ✓ | 8+ ✓ | 8+ ✓ |
| Goose | 8 ✓ 8 s | 2 ✓ 1 s | 2 ✗ 1 s | 2 ✓ 1 s | 8 ✓ 9 s | 3 ✓ 1 s | 2+ ✗ | 8 ✓ 8 s | 0+ ✗ |
| Copilot CLI, own key | 1 ✓ 4 s | 3 ✗ 6 s | 1 ✓ 4 s | 1 ✗ 5 s | 1 ✗ 4 s | 1 ✗ 6 s | 1 ✓ 4 s | 1 ✓ 6 s | 6 ✓ 21 s |

- **400 is the one status every program stopped on and showed.** In the provider's error shape
  (`invalid_request_error` for Anthropic and OpenAI, `{"error": {"code": 400, …}}` for Google),
  all eight stopped within 9 seconds and printed Silo's sentence. Goose tried 8 times in 8 seconds
  first; the others once or twice.
- **403 is not enough.** Aider retried it 8 times and more, Copilot printed its own text in place
  of Silo's, and Codex sent it 6 times in 14 seconds.
- **Some statuses replace Silo's words with the program's own.** On 402, Goose writes "Please
  check your account with your provider to add more credits", which sends the user to the wrong
  place. On 404, Claude Code says the model "may not exist or you may not have access to it". On
  401, 403, 404 and 413, Copilot shows its own message. On 401, Claude Code keeps retrying and
  shows nothing.
- **429 with a day's `retry-after`** made Claude Code stop at once, but Codex, OpenCode and Goose
  kept waiting. **500** is retried by nearly everyone.
- **A reply in place of a refusal.** A 200 whose model reply is Silo's sentence, ending the turn,
  reached the user in every program that finished (`refuse:200` in the scripts). It needs no
  status at all, but it puts words in the model's mouth, which `terms-on-changing-a-reply` and
  `no-silent-change-to-traffic` weigh against.

### A difference with the first survey

The first survey found Claude Code retrying a 403 seven times in a minute, with nothing shown. Here
it stopped after two requests and showed the sentence, with the same version, also with a body in
no provider's shape. The likely difference: the first survey's stand-in refused every request to
`api.anthropic.com`, the sign-in and settings calls included, while this one refused only the model
requests and let the others fail with 404, as Silo would. That was not run to the end, so this
page does not claim the first table is wrong for its own set-up. For a budget, which refuses model
requests only, this table is the one that applies.

## 2. A hold that the programs wait for

The first survey held a model request with no byte at all. Here the hub held the first model
request for 300 or 420 seconds in three ways, then sent the real reply on the same response:

- **headers only**: the status and headers at once, then nothing;
- **keep-alive**: the same, then every 5 seconds a byte the format ignores: an Anthropic `ping`
  event, an SSE comment for the other streams, a space before a JSON body;
- **unknown event**: the same, with `event: silo.hold` every 5 seconds instead.

Each cell says how long the program waited on the held reply, and whether it then took the reply
(✓) or gave up and sent the whole request again (resent). "—" was not run.

| Program | Headers only | Keep-alive bytes | Unknown event |
|---|---|---|---|
| Claude Code | gave up at 180 s, resent | waited 300 s ✓ (`ping`) | — |
| Codex | gave up at 300 s, resent | gave up at 300 s, resent (SSE comment) | waited 420 s ✓ |
| Gemini CLI | waited 300 s ✓ | waited 300 s ✓ (SSE comment) | waited 420 s ✓ |
| OpenCode | gave up at 300 s, resent | waited 300 s ✓ (`ping`) | failed after 7 s, resent |
| Cline | — | waited 300 s ✓ (`ping`) | failed after 4 s: "Expected a normalized Anthropic stream error" |
| Goose | waited 300 s ✓ | waited 300 s ✓ (`ping`) | waited 420 s ✓ |
| Aider | waited 300 s ✓ | waited 300 s ✓ (`ping`) | waited 420 s ✓ |
| Copilot CLI, own key | waited 300 s ✓ | waited 300 s ✓ (`ping`) | waited 420 s ✓ |

- **The 15 s of Codex and the 60 s of Gemini CLI are limits on the first byte.** Once the headers
  are sent, Gemini CLI waited the full 300 s and went on. So a hold that sends the headers of its
  reply first escapes both.
- **Codex's limit after the first byte is 300 s without an event.** SSE comments do not count for
  it; an event does, even one it does not know. It waited 420 s on `silo.hold` events and took the
  reply.
- **Claude Code gives up after 180 s with headers and nothing else.** Anthropic's own `ping`
  event, which its format documents, kept it and every other program on Messages waiting for the
  full 300 s.
- **An unknown event is not safe everywhere.** On the Messages format, OpenCode resent the request
  and Cline failed on it. So the byte has to be one the format documents: `ping` for Messages, a
  comment for Gemini's stream. OpenAI's Responses format documents no such event, so on it only an
  unknown event kept Codex, and other readers of Responses were not tried.
- In a real hold, the provider's reply has already started when the tool call arrives, so the
  status and headers have already gone to the program. What is measured above is the wait after
  them, which is what a hold needs.

## 3. A denial that leaves the program's own processes alone

The first survey found OpenCode stopping when its own `git` snapshot failed inside a denial's
window. Two ways to keep it were tried.

**One denial, then disarm.** The probe disarms after its first refusal. This would keep OpenCode,
whose first process in the window is the call's shell. It is not sound in general: Copilot CLI's
first process in the window is its own (`bash -l -c … __copilot_login_env_snapshot …`, to read
the login shell's environment), and its call's shell comes second. Under this rule the call would
run. So it was dropped.

**The program's own commands, allowed by their command line.** The probe gets a list of command
lines that start a process of the program's own, as prefixes, and lets an exec through when its
whole command line starts with one of them. Everything that process then writes still goes through
the probe: writes outside the program's own folders are refused as before. For OpenCode 1.19.6,
three prefixes cover every process it started in the window:

```
git rev-parse --path-format=absolute --git-path info/exclude
git -c core.autocrlf=false -c core.longpaths=true -c core.symlinks=true -c core.quotepath=false --git-dir <home>/.local/share/opencode/snapshot/
git --git-dir <home>/.local/share/opencode/snapshot/
```

With them, OpenCode's denied `rm` failed with `EPERM`, its 15 snapshot commands ran, and the
session went on to the end with snapshots on. `existing.txt` stayed.

Why a call cannot use the list: a shell call always starts `bash -c …` (or `/usr/bin/bash`), so its
command line never starts with `git`. A prefix is matched from the first word, never inside. And a
`git` that is let through cannot write the work folder, start a hook, a pager or a helper, or reach
the network: each of those is a write or an exec of its own, which the probe judges as before.

What the other programs start in the window, with nothing allowed:

| Program | Its own processes refused in the window | Went on |
|---|---|---|
| Claude Code | 9: `git status` variants, `rg --files`, `bash -c env`, its shell snapshot | yes |
| Copilot CLI | 1: its login-shell environment, before the call's shell | yes |
| Codex, Gemini CLI, Cline, Goose | none beyond the call | yes |
| OpenCode | 15 `git` commands of its snapshot | no, unless allowed as above |
| Aider | its `git add` and its own write of the edited file, in its own process | the edit fails; it has no calls to deny |

So the list is a field of the program's data file (`agent-program-data-file`), checked by the live
check of `denial-per-program` for each version, like the others.

## 4. Local models

### What each program sends to a local server

Each program was pointed at a server on port 11434 (Ollama's) or 1234 (LM Studio's), in plain HTTP:

| Program | Setting | Path and format |
|---|---|---|
| Claude Code | `ANTHROPIC_BASE_URL` at Ollama | `/v1/messages?beta=true`, Anthropic Messages; it still called `api.anthropic.com` for its bootstrap and settings |
| Codex | `--oss --local-provider ollama` | `/v1/responses`; it first asks `/api/tags` and, if the model is missing, `POST /api/pull` makes the server download it |
| Codex | `--oss --local-provider lmstudio` | `/v1/responses` at port 1234 |
| Goose | `GOOSE_PROVIDER=ollama` | `/api/show`, then `/v1/chat/completions` |
| Aider | `ollama_chat/` model | `/api/show`, then `/api/chat`, Ollama's JSON lines |
| Cline | `-P ollama` | `/api/tags`, then `/api/chat` |
| Copilot CLI | `COPILOT_PROVIDER_BASE_URL`, `COPILOT_OFFLINE=true` | `/v1/chat/completions` |

Six ran the shell call and ended. Aider, which has no tool calls, sent its request to `/api/chat`.
So a local server gets four formats: Messages, Responses, Chat Completions and Ollama's own lines.

### What the servers serve

- **Ollama 0.40.18** serves `/api/chat` and `/api/generate` (JSON lines), `/v1/chat/completions`,
  `/v1/responses` and `/v1/messages`, each answering in its own provider's error shape
  (`prototype/ollama_routes.sh`).
- **LM Studio's headless server** (`llmster`, started with `lms server start`) answered
  `/v1/chat/completions`, `/v1/responses` and `/v1/messages` in the providers' shapes ("No models
  loaded"), and its own `/api/v1/chat` and `/api/v0` and `/api/v1/models`. It keeps a sign-in for
  LM Studio's hub under `~/.lmstudio/credentials`, and its `lms link` joins other machines'
  LM Studio, so a "local" model may run on another machine.

### Ollama's cloud models

Asked for `gpt-oss:120b-cloud`, Ollama's own server in the computer sent the request on to
`ollama.com` (`/api/show`, then `/api/generate`), over TLS, trusting the system's store. Each
request carried `Authorization: <public key>:<signature>`, signed with a key it made on first start,
`~/.ollama/id_ed25519`, over the method, the path and a `ts` parameter. After `ollama signin`, that
key is the account's sign-in.

- It is a login on disk: a fork copies it, so `fork-and-sign-in` and the program's data file have
  to name `~/.ollama/id_ed25519`.
- A placeholder cannot stand in for it, because each request is signed afresh. Silo would have to
  hold the key and sign each request itself, as the research on AWS signatures did.
- `ollama.com` is a provider whose format is Ollama's own lines, so a model of Ollama's cloud,
  reached through a server inside the computer, is model traffic like any other. `OLLAMA_NO_CLOUD=1`
  turns cloud models off in the server.
- Ollama's server also calls `ollama.com` for model recommendations and `registry.ollama.ai` for
  downloads.

### A model on the Mac

`network-modes` says that open never includes your device. So Ollama or LM Studio running on the Mac
cannot be reached from a computer today, and a computer has no access to the Mac's GPU to run one
inside. A model on the Mac is reached in plain HTTP, so Silo could read it without any certificate,
with the four readers above. Whether it is reached at all is a new channel for `channel-list`; it
is raised as a question in the notes of `program-that-cannot-be-read`.

## 5. Copilot's own route and Kiro

**Copilot CLI with a GitHub token** asked `api.github.com/copilot_internal/user` for its plan and
the hosts to use (`api.individual.githubcopilot.com` here), then
`copilot_internal/managed_settings` and `team_customizations`, then the model list at
`GET /meta` on the plan's host, before any model call. A stand-in answer for `/meta` was not
accepted ("invalid type: map, expected a sequence"), so the model calls were not reached. It logs
"HTTP 404 response does not appear to originate from GitHub. Is a proxy or firewall intercepting
this request?" for a 404 without GitHub's headers: it notices a refusal made by something other
than GitHub. With its own key mode (`COPILOT_PROVIDER_TYPE`, `_BASE_URL`, `_API_KEY`, or
`providers.json`), Copilot needs no GitHub sign-in and is an ordinary client of Anthropic, OpenAI
(Chat Completions, Responses, or Responses over a WebSocket), Azure or Google.

**Kiro CLI 3.6.0** installed from its release archive into a folder of its own. With a fake
`KIRO_API_KEY` it calls AWS-style JSON services (`x-amz-target`, `application/x-amz-json-1.0`,
`Authorization: Bearer <key>`, `TokenType: API_KEY`) on `management.<region>.kiro.dev`
(`GetProfile`, `ListAvailableModels`, `GetUsageLimits`) and `runtime.<region>.kiro.dev`
(`GetFeatureConfiguration`, `InvokeMCP`). It stopped at "could not load the available models".
Its own code names the model call `KiroRuntimeService.GenerateAssistantResponse`, with an event
stream for the reply: the AWS framing the Bedrock research found, around Kiro's own events.

- **`GetUsageLimits` is Kiro's own account of what is spent**, asked by the program itself. Silo
  could read it as it passes, the way `own-account-matches` reads a provider's headers.
- Kiro keeps its state in `~/.kiro/` (sessions, settings, logs) and
  `~/.local/share/kiro-cli/data.sqlite3`, which holds the sign-in of `kiro-cli login`. On first
  start it downloads its own Node.js and agent into `~/.local/share/kiro-cli/`.
- It tried every region in turn (us-east-1, eu-central-1, the GovCloud and the isolated regions),
  each with up to three attempts, before giving up.

## What was not done

- Nothing on a Mac. No real sign-in, so no model call of Copilot's own route, Kiro, Amp or Cursor.
- No real model in Ollama or LM Studio: the servers were asked for their routes, and the programs
  talked to the fake hub on those ports.
- The keep-alive bytes were not tried on a WebSocket (Codex's first transport).
- The 403 difference with the first survey was explained, not reproduced.

## Plan entries that got notes

`budget-denies-or-holds`, `how-a-hold-ends`, `hold-limit-length`, `own-processes-in-a-denial`,
`program-that-cannot-be-read`, `vendor-runs-the-model-calls`, `fork-and-sign-in`.

## Running the scripts

Linux, as root, with `unshare`, Python 3 with `cryptography`, and libseccomp:

```
gcc -O2 prototype/sprobe-argv.c -o prototype/sprobe-argv -lseccomp
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout prototype/ca/ca.key \
  -out prototype/ca/ca.crt -days 30 -subj "/CN=Silo test boundary CA" \
  -addext "basicConstraints=critical,CA:true" -addext "keyUsage=critical,keyCertSign,cRLSign"
cat /etc/ssl/certs/ca-certificates.crt prototype/ca/ca.crt > prototype/ca/store-with-ca.crt
mkdir prototype/bin   # links to each program's executable, named as in progs.sh
prototype/matrix.sh ok store 60 cc cx gem       # one call each, the authority in the store
prototype/matrix.sh deny store 60 oc            # a denied call
prototype/matrix.sh refuse:400 store 45 cc cx   # every model request refused with 400
prototype/matrix.sh keep:300:ping store 420 cc  # a reply held 300 s with keep-alive bytes
python3 prototype/refusal_table.py              # the table of section 1, from out/
```

`run.sh` changes nothing outside its namespace: the program's DNS, its port 443 and its copy of the
system's store are the hub's. Every key in `progs.sh` is a fake.
