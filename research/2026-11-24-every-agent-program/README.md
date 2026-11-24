# Every agent program through the boundary (24 November 2026)

An agent's research, 24 November 2026. Until now the boundary was tried with Claude Code and Codex
only. This page runs the other agent programs that people give full access to through the same
tests, on Linux, with fake providers and the spike of the boundary from `forwarded-byte-for-byte`.
Each finding is copied into the notes of the plan's entry it bears on, listed at the end.

Nothing here used a real account. Every key was a fake, and every provider was a stand-in on the
same machine. The scripts are in [`prototype/`](prototype/).

## The programs

| Program | Version | Runs on Linux here | How it was reached |
|---|---|---|---|
| Claude Code | 2.1.331 | yes | npm |
| Codex | 0.172.0 | yes | npm |
| Gemini CLI | 0.71.1 | yes | npm `@google/gemini-cli` |
| Qwen Code | 0.28.4 | yes | npm `@qwen-code/qwen-code`, a fork of Gemini CLI |
| Cline CLI | 3.0.75 | yes | npm `cline` |
| OpenCode | 1.19.6 | yes | npm `opencode-ai` |
| Goose | 1.60.2 | yes | the release binary from GitHub |
| Aider | 0.86.2 | yes | `uv tool install aider-chat` |
| GitHub Copilot CLI | 1.0.115 | yes | npm `@github/copilot` |
| Amp | 0.0.1795536036 | yes | npm `@sourcegraph/amp` |
| Cursor CLI (`cursor-agent`) | 2026.11.22-e9042b2 | yes | the package `cursor.com/install` downloads |
| Kiro CLI | | not run | its installer was refused in this environment |
| Windsurf | | not run | an editor, with no command-line agent |
| Ollama, LM Studio | | not run | stand-ins on their ports, for the paths programs use |

## How it was run

- **The survey.** A listener on port 443 with a certificate for every provider's host, signed by a
  test authority, logged for each connection the host name, the protocol agreed (HTTP/2 or
  HTTP/1.1), whether the program trusted the certificate, and its first requests. It answered
  every request with an error. Each program ran in a mount namespace of its own where
  `/etc/hosts` sends the providers' names to 127.0.0.1, so nothing else on the machine was
  redirected.
- **Whole conversations.** The spike of the boundary on port 443, in front of a fake provider. The
  fake plays the model: it asks for a shell call that removes `existing.txt`, then a harmless shell
  call, then a file write to `DENIED.txt`, then a harmless write. The spike denies each call whose
  input holds `DENIED` and drives the stand-in probe of `denied-tool-call-mechanism`.
- **Refusals.** The listener answered every request with 403, then with 429, and a body that says
  "Silo: this computer's budget is reached. Raise it in the Silo app."
- **Holds.** The listener never answered a model request. Each program ran for 12 minutes in a
  network namespace of its own.

## 1. Trusting Silo's certificate

The test authority was put in the system's store only (`update-ca-certificates`), with no setting
in the program.

| Program | System store alone | What made it work otherwise |
|---|---|---|
| Claude Code, Codex, Cline, OpenCode, Goose, Copilot CLI, Amp, Cursor CLI | Works | |
| Gemini CLI | Refuses | `NODE_USE_SYSTEM_CA=1` (or `NODE_EXTRA_CA_CERTS`) |
| Qwen Code | Refuses | `NODE_EXTRA_CA_CERTS` (or `NODE_USE_SYSTEM_CA=1`) |
| Aider | Refuses | `SSL_CERT_FILE` naming the system's bundle |

- Gemini CLI and Qwen Code run on plain Node.js, which trusts only the list it carries unless told
  otherwise. Aider is Python, whose HTTP library uses the `certifi` list. These three are the
  programs that need a setting from their recipe, as `reading-each-agent-program` allows.
- Cline builds its own bundle at start, `~/.cline/cli-node-extra-ca-certs.pem` (486 certificates
  here), from the system's store. So it follows the store, through a file it writes.
- With no authority anywhere, every program refused. None skipped the check. A Node.js program
  completes the handshake and then drops the connection, so a server sees a finished handshake and
  no request: only a request proves trust.

## 2. Hosts, formats and protocols

What each program sent, with an API key, to a host it took for its provider:

| Program | Model host | Format of the model requests | Protocol offered |
|---|---|---|---|
| Claude Code | `api.anthropic.com` | Anthropic Messages | HTTP/1.1 |
| Codex | `api.openai.com` | OpenAI Responses, over a WebSocket first | HTTP/1.1 and HTTP/2 |
| Gemini CLI | `generativelanguage.googleapis.com` | Google's `streamGenerateContent?alt=sse` and `generateContent` | HTTP/1.1 |
| Qwen Code | `dashscope-intl.aliyuncs.com` | Chat Completions | HTTP/1.1 |
| Cline | `api.anthropic.com` or `api.openai.com` | Messages, or Responses | HTTP/1.1 |
| OpenCode | `api.anthropic.com` or `api.openai.com` | Messages, or Responses | HTTP/1.1 |
| Goose | `api.anthropic.com` or `api.openai.com` | Messages, or Responses | HTTP/2 first |
| Aider | `api.openai.com` or `api.anthropic.com` | Chat Completions, or Messages | HTTP/1.1 |
| Copilot CLI | `api.github.com`, then `api.githubcopilot.com` | GitHub's own exchange for a token, then the model calls (not reached with a fake token) | HTTP/1.1 |
| Amp | `ampcode.com` | Amp's own API (`/api/internal?…`); model calls go through Amp's server | HTTP/1.1 |
| Cursor CLI | `api2.cursor.sh` | Cursor's own RPC (Connect, protobuf) | HTTP/1.1 for the sign-in calls |

- **One host serves several formats.** `api.openai.com` gets Chat Completions from Aider and
  Responses from Codex, Cline, OpenCode and Goose. So the boundary picks the reader by the
  request's path, not by the host. The spike now does: `/chat/completions`, `generateContent`,
  `/messages` and `/responses` choose the reader.
- **Every program fell back to HTTP/1.1** when the boundary offered nothing else. Goose offers
  HTTP/2 first and worked over HTTP/1.1 at once. Codex's side connection to `github.com` did too.
- **Cursor's agent prefers HTTP/2.** Its code holds a setting, `network.useHttp1ForAgent`, and a
  switch on Cursor's server that forces HTTP/1.1. Its agent streams in both directions, which
  needs HTTP/2 unless that setting is on. Its messages are Cursor's protobuf, which Cursor does
  not document.
- **Amp has no keys of its own any more.** Amp's announcement "No more BYOK" says it removed the
  mode that used your own API keys. Every model call goes to `ampcode.com`, and Amp's server calls
  the model.
- **Programs fetch more than the model.** Goose and Qwen Code read `models.dev/api.json`. OpenCode
  and Cline ask `registry.npmjs.org` for updates. Gemini CLI sends telemetry to
  `play.googleapis.com`. Codex reads `github.com`, `api.github.com` and `chatgpt.com` for its
  plugins.

### Two more formats

- **Google's format.** A call comes whole, in one event: `{"functionCall": {"name", "args"}}`.
  Nothing has to be held while it streams. The model may give no id. Gemini CLI then makes its
  own, `run_shell_command_<time>_0`, and sends the result under that id. The boundary cannot match
  that result to the call by id, so it matches it by the call's name. The first run, matching by
  id only, never saw the result: the probe stayed armed, and the next, allowed calls failed too.
  When the model gives an id, Gemini CLI sends it back unchanged.
- **Chat Completions.** A call starts with the first piece that names its index and is whole when
  the reply says why it finished (`finish_reason`). Results come back as messages with the role
  `tool`. Qwen Code, Aider, Goose with Ollama, and most local servers speak it.
- **Ollama's own API**, used by Aider for `ollama_chat/` models, is `POST /api/chat` with a stream
  of JSON lines, not of events. It is a fifth format. It travels in plain HTTP, on port 11434.

The spike was taught the first two, with tests: about 150 lines in Zig
([`prototype/spike-chat-gemini.patch`](prototype/spike-chat-gemini.patch)).

## 3. A denied tool call, program by program

Through the spike, which denied the calls holding `DENIED` and drove the stand-in probe.

| Program | Denied shell call | Denied file write | Allowed calls after it | Went on |
|---|---|---|---|---|
| Gemini CLI | result `spawn EPERM` | `EPERM … open '…/DENIED.txt.<uuid>.tmp'` | ran | yes |
| Qwen Code | result `spawn EPERM` | `EPERM … open '…/DENIED.txt.<hex>.tmp'` | ran | yes |
| Cline | `Command failed: EPERM … posix_spawn '/bin/bash'` | `Editor operation failed: EPERM … open` | ran | yes, and submitted |
| Goose | `Failed to spawn shell command: Operation not permitted` | `Failed to write …: Operation not permitted` | ran | yes |
| OpenCode | `EPERM … posix_spawn '/usr/bin/bash'`, then the session stopped | | | no |
| OpenCode, `"snapshot": false` | `EPERM … posix_spawn '/usr/bin/bash'` | `Unknown: FileSystem.writeFile` | ran | yes |

- In every case nothing of the denied call ran: `existing.txt` stayed, `DENIED.txt` was never
  written, and the program sent the error once, with no retry.
- **OpenCode stops.** After each tool call it runs `git` to take a snapshot of the work folder. In
  the denial's window that `git` fails too, and OpenCode ends the session with "Unexpected server
  error". With snapshots off, a setting OpenCode documents, it behaves like the others. Claude Code
  also runs `git` after a write, and it carried on.
- **The error is not always marked as one.** Claude Code and OpenCode send the result with
  `is_error: true`. Cline sends it as text with `"success": false`. Goose sends only the text.
  Every model saw the error, but a reader of the record cannot tell an error from the flag alone.
- **Aider has no tool calls.** It asks the model for edits as text blocks and applies them itself,
  and it runs shell commands it finds in the text. The boundary sees no call to deny, only text.

## 4. A refusal from the boundary

Every model request answered with 403, then with 429, with Silo's sentence in the body:

| Program | 403: requests | 403: Silo's sentence shown | 429: requests | 429: shown |
|---|---|---|---|---|
| Claude Code | 7 in 60 s, still retrying | no | 7 in 60 s, still retrying | no |
| Codex | 14 in 13 s, then stopped | yes | 2, then stopped | no |
| Gemini CLI | 4 in 16 s | yes | 5 in 20 s | yes |
| Qwen Code | 22 in 60 s, still retrying | no | 23 in 60 s, still retrying | no |
| Cline | 1 | yes | 4 in 14 s | yes |
| OpenCode | 6 in 33 s, still retrying | no | 8 in 35 s, still retrying | no |
| Goose | 1 | yes | 7 in 7 s | yes |
| Aider | 8 in 32 s, still retrying | yes | 8 to 24 in 32 to 43 s | yes |
| Copilot CLI | 8 in 0.4 s, then stopped (its token exchange at `api.github.com`, not a model call) | yes | the same | yes |

- `how-a-hold-ends` says that a 403 is not retried by the usual programs. Claude Code, OpenCode,
  Qwen Code and Aider retried it, with growing waits. Only Cline and Goose stopped at the first
  403.
- Claude Code showed nothing of the body in the minute it ran: it was still retrying. So
  "the budget is reached" has to reach the user from Silo's own app, not from the agent program.
- Amp and Cursor stop before any model call when their own sign-in fails, so a refusal of a model
  call could not be tried on them with a fake key.

## 5. A reply that never comes

Each program sent one model request that got no answer at all, not even a first byte, for 12
minutes. A close is the program closing the held connection.

| Program | What it did |
|---|---|
| Codex | Closed after 15 s and sent the request again, five times, then fell back from its WebSocket to plain requests and waited on the last one until the end |
| Gemini CLI | Closed after 60 s and sent it again, five times, then stopped with an error at 327 s |
| Qwen Code | Closed after 120 s and sent it again, six times in 12 minutes |
| Goose | Reset the stream after 600 s and sent it again |
| Aider | Closed after 600 s and sent it again |
| OpenCode | Never closed it. It sent new requests on other connections at 303 s and 608 s |
| Claude Code | Never closed it. It sent one more model request on another connection at 183 s |

- `hold-limit-length` proposes 90 seconds. Codex gives up on a silent reply after 15 seconds and
  Gemini CLI after 60, so a reply held longer than that is sent again by the program, with the whole
  conversation, before the hold ends. The boundary sees each retry as a new request.
- This measures a reply with no byte at all. When Silo holds a reply from the start of a tool call,
  the bytes before it have gone on, and whether a program counts its wait from the last byte was
  not tried.
- Cline was left out: its run had a limit of 40 seconds of its own.

## 6. What runs outside the boundary's reach

- **Vendor-run model calls.** Cursor, Amp, Kiro, Windsurf and Copilot send the conversation to the
  vendor's own server, which calls the model. Silo can read only what goes to that server, in the
  vendor's format. Copilot's is close to the providers' formats; Cursor's and Windsurf's are
  protobuf that their vendors do not document; Kiro's is AWS's. The decision
  `vendor-runs-the-model-calls` asks what Silo does with them.
- **Local models.** A model that runs in the computer sends nothing across the boundary
  (`program-that-cannot-be-read`). A model served from the Mac, such as Ollama on port 11434 or LM
  Studio on 1234, is reached in plain HTTP. The boundary can read it without any certificate, but
  only with readers for Chat Completions and for Ollama's own lines.
- **A way around a budget.** An agent with full access can point its own program at a provider
  Silo does not read: `cline -P openrouter`, a model in OpenCode's settings, `qwen` with another
  `OPENAI_BASE_URL`, Ollama on the Mac. The research in #351 lists the routes and the rule that
  closes them: under a budget, model traffic to a host Silo does not read is denied. These
  programs make the route a one-line change, so the rule has to be on by default under a budget.

## 7. Where each program keeps its login

Seen on disk after the runs, or read in the vendor's documentation where marked.

| Program | Where an API key or sign-in lands |
|---|---|
| Cline | `~/.cline/data/settings/providers.json`, the API key in plain text, when given with `-k` (seen) |
| OpenCode | `~/.local/share/opencode/auth.json` after `opencode auth login` (documentation) |
| Gemini CLI | `~/.gemini/oauth_creds.json` for a Google sign-in (documentation) |
| Qwen Code | `~/.qwen/oauth_creds.json` for a Qwen sign-in (documentation) |
| Copilot CLI | the system's keychain, else `~/.copilot/` (documentation); a token in `COPILOT_GITHUB_TOKEN`, `GH_TOKEN` or `GITHUB_TOKEN` |
| Cursor CLI | `~/.cursor/` (seen: `cli-config.json`), or `CURSOR_API_KEY` |
| Amp | `~/.local/share/amp/` (seen), or `AMP_API_KEY` |
| Goose | the system's keyring, or `~/.config/goose/secrets.yaml` when the keyring is off (documentation) |
| Aider | environment variables, `.env` or `~/.aider.conf.yml` (documentation) |

A key given on Cline's command line is written to the disk, so it ends up in every fork of the
computer unless `fork-and-sign-in` keeps it out. This is a field for each program's data file.

## What was not done

- Nothing on a Mac. The trust results are Linux's.
- No real sign-in, and so no sign-in from scratch through the boundary, for any program.
- Copilot, Amp and Cursor were not taken past their sign-in, so their model traffic was not seen.
- Kiro was not installed. Windsurf has no command-line agent.
- The resume of a conversation after a denial was not tried for the new programs.
