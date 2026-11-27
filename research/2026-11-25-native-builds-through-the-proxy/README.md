# The native Claude Code build and Codex's WebSocket through the closed network

Research of 25 November 2026, by an agent, for polarzero. #382 (gap 11) found that every trial of
the boundary ran Claude Code from npm, while main installs it with `claude.ai/install.sh`, and that
Codex's WebSocket had never gone through a `CONNECT` proxy. This page runs both through #378's
closed network on Linux: a computer with no route and no name server, whose only way out is a
proxy on `127.0.0.1:3128` relayed over a Unix socket that stands in for vsock.

Both work. The failures found are what a person sees when a setting is missing, and one of them
is silent: Codex that does not trust Silo's certificate says it is waiting for the network, and
never stops.

| File | Holds |
|---|---|
| [`run.sh`](run.sh), [`inside.sh`](inside.sh) | The experiment: a stand-in provider and the proxy outside, each case inside a network namespace with only its loopback |
| [`provider.py`](provider.py) | A stand-in for Anthropic's and OpenAI's APIs over TLS that also speaks WebSocket, and logs each handshake |
| [`proxy.py`](proxy.py), [`relay.py`](relay.py) | #378's proxy and relay; the proxy takes `ALLOW="*"` here, so every host a program tries is logged |
| [`store.sh`](store.sh) | Runs one command with the test authority in the system's own store |
| [`results.tsv`](results.tsv) | Each case: exit status, seconds, the last line of output |
| [`untrusted.py`](untrusted.py), [`untrusted.txt`](untrusted.txt) | What the proxy's log and the TLS side's log each say about a refused certificate |

## In short

- **The native build is the one main gets, and it works through the proxy.** The Claude Code in
  this container is an executable built with Bun 1.4.6, and its SHA-256 equals the one
  `downloads.claude.ai` lists for 2.1.332 on `linux-x64`, the build `install.sh` installed today.
  It follows `HTTPS_PROXY` and `https_proxy`. It trusts Silo's test authority through any of
  `NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE` or the system's store, with no `NODE_USE_SYSTEM_CA`.
  Signed in with a plan token (`CLAUDE_CODE_OAUTH_TOKEN`), it works the same.
- **Codex's WebSocket goes through `CONNECT` with no setting of its own.** Codex 0.172.0 with an API
  key opened `wss://api.openai.com/v1/responses` as a `CONNECT api.openai.com:443` and ran its
  warm-up and its turn on it, in 0.4 s. It follows `HTTPS_PROXY` and `https_proxy`, and trusts the
  authority through `SSL_CERT_FILE` or the system's store.
- **Codex that does not trust Silo's certificate never ends.** It tries the WebSocket five times,
  falls back to plain requests, then prints "Reconnecting... waiting for network" for as long as it
  runs. It was stopped at 400 s, after 53 handshakes. Claude Code in the same case stops after
  18.5 s and names the certificate.
- **Only the side that ends TLS sees a refused certificate.** The proxy logs each of Codex's 61
  connections as allowed, with bytes each way. The TLS side saw 53 handshakes fail: Codex sends the
  alert `unknown_ca`, Claude Code closes the connection. So the boundary, not the proxy, is where
  "this program does not trust Silo" can be recorded and shown.
- **A program with no proxy setting looks offline for minutes.** Claude Code retried for 195 s, as
  #378 found with the npm build. Codex was still retrying at 400 s.
- **The hosts at start differ by program.** Claude Code reached only `api.anthropic.com` (12
  connections, 6 with `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`). Codex with an API key also
  reached `github.com`, `api.github.com`, `chatgpt.com` and `ab.chatgpt.com` before its first turn.
  `claude update` reaches `downloads.claude.ai` through the proxy.
- **Commands the agent runs keep the settings.** A command run by Claude Code's Bash tool had
  `HTTPS_PROXY`, `NODE_EXTRA_CA_CERTS` and `SSL_CERT_FILE`, and `curl` got 200. Node's own `fetch`
  in that command failed with `EAI_AGAIN`, as #378 found: main's environment also needs
  `NODE_USE_ENV_PROXY=1`.

## The cases

Each case ran in a fresh home folder, inside a network namespace with only its loopback and an
`/etc/hosts` with only `localhost`. `P` is `http://127.0.0.1:3128`. Every key is a fake one.

| Case | Settings | Result |
|---|---|---|
| `cc-none` | no proxy | 195 s, then "Can't reach the API server — check your internet or DNS (EAI_AGAIN)" |
| `cc-proxy-node-ca` | `HTTPS_PROXY`, `NODE_EXTRA_CA_CERTS` | answered in 5.8 s |
| `cc-proxy-lower-node-ca` | `https_proxy`, `NODE_EXTRA_CA_CERTS` | answered |
| `cc-proxy-ssl-cert-file` | `HTTPS_PROXY`, `SSL_CERT_FILE` naming the store with the authority | answered |
| `cc-proxy-store` | `HTTPS_PROXY`, the authority in `/etc/ssl/certs/ca-certificates.crt` | answered |
| `cc-proxy-store-use-sys` | the same, and `NODE_USE_SYSTEM_CA=1` | answered |
| `cc-proxy-no-ca` | `HTTPS_PROXY`, the authority nowhere | 18.5 s, "Self-signed certificate detected (SELF_SIGNED_CERT_IN_CHAIN)…" with the fix in the message |
| `cc-proxy-no-updater` | `DISABLE_AUTOUPDATER=1` | answered, same 12 connections |
| `cc-proxy-no-traffic` | `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` | answered, 6 connections |
| `cc-plan-proxy` | a plan token instead of a key | answered |
| `cc-tool-command` | its Bash tool runs `curl` and Node's `fetch` | `curl` 200, `fetch` `EAI_AGAIN` |
| `cc-update` | `claude update` | `downloads.claude.ai` through the proxy (answered 502 here) |
| `cx-none` | no proxy | still "Reconnecting... waiting for network" at 400 s |
| `cx-proxy` | `HTTPS_PROXY`, `SSL_CERT_FILE` | the WebSocket through `CONNECT`, answered in 0.4 s |
| `cx-proxy-lower` | `https_proxy`, `SSL_CERT_FILE` | the same |
| `cx-proxy-store` | `HTTPS_PROXY`, the authority in the system's store | the same (16 s on its first run, all before its first connection; 0.4 s again) |
| `cx-proxy-no-ca` | `HTTPS_PROXY`, the authority nowhere | five WebSocket tries with "invalid peer certificate: UnknownIssuer", the fallback, then "waiting for network" until stopped at 400 s |
| `cx-chatgpt-proxy` | signed in with ChatGPT (a made-up `auth.json`) | every call to `chatgpt.com` through `CONNECT`; stopped before the model call (below) |

On a WebSocket, Codex asked for compression (`permessage-deflate`). The stand-in did not agree to
it, and Codex went on without. Its first message was the warm-up (`"generate": false`), and its
second named the warm-up's reply in `previous_response_id`, as the spike of
`reading-each-agent-program` found.

## What fails, and the fix

**1. A program that does not trust Silo's certificate is invisible in #378's design.** The proxy
cannot see inside a tunnel. `untrusted.txt` shows the same case from both sides:

| Case | Proxy: connections allowed | TLS side: handshakes done, failed |
|---|---|---|
| `cc-proxy-no-ca` | 27 | 0, 27 closed by the client |
| `cx-proxy-no-ca` | 61 | 0, 53 with the alert `unknown_ca` |

The fix is on the boundary, which ends TLS on the hosts it reads: record each handshake a client
refuses, with the host and the client's alert, and merge them into one line for the morning, for
example "Codex did not trust Silo's certificate on api.openai.com: 53 refusals from 02:10, it was
stuck". A boundary that has this sees the failure within the first refusal. It was shown here on
the stand-in's side only; #374's boundary does not record it yet.

**2. Codex hides the cause and never stops.** Its error names the certificate only while it tries
the WebSocket. After the fallback it says "waiting for network". A night where Codex lacks the
authority is a night with no work and no error. Main's set-up should put the authority in the
system's store and set `SSL_CERT_FILE`, and the first run should run Codex once through the
boundary before it says main is ready, as #382 asks.

**3. Programs that ignore the proxy look offline for minutes.** Nothing to fix in the programs.
#382's gap 10 (count what the dead-end card drops) is what makes it visible.

**4. Node's own `fetch` in a command needs `NODE_USE_ENV_PROXY=1`.** Already in #378; seen again
here in a command the agent ran.

## What it changes in the plan

Notes were added as "Agent, 25 November 2026", under "Native builds through the proxy". No verdict,
answer or list was changed.

| Entry | What the note says |
|---|---|
| `reading-each-agent-program` | The install-script build and Codex's WebSocket ran through the proxy; the settings each needs; what is still not tried |
| `program-that-cannot-be-read` | A program that refuses Silo's certificate is seen only by the boundary; Codex then looks offline and never stops; record and show the refusals |
| `alert-only-on-a-change` | The hosts each program reaches at start, before any work, for the list of hosts that count as seen |

## Not verified

- Codex signed in with ChatGPT did not reach its model call. Before it, Codex asks
  `chatgpt.com/backend-api/wham/accounts/check` for its workspaces, and the stand-in's answer got
  past two of its checks but not the third ("invalid account routing override"). All 29 of its
  requests before that (25 connections, all to `chatgpt.com`) went through `CONNECT`. Whether its WebSocket to `chatgpt.com` behaves like
  the one to `api.openai.com` needs a real sign-in, which this container does not have.
- The stand-in offers only HTTP/1.1. Which protocol each program would pick if HTTP/2 were offered
  was not measured; a `CONNECT` tunnel carries either.
- Commands run by Codex were not tried; its default `shell_environment_policy` passes the
  environment on, which was not checked here.
- Nothing ran on a Mac or in a macOS guest. The relay is a Unix socket, not vsock.
- A real provider was not reached: there are no API keys in this container.

## Sources

- #378's `closed-network/` (relay and proxy), #382's gap 11, `reading-each-agent-program` at #391's
  head.
- Claude Code 2.1.332 (`/opt/claude-code/bin/claude`, checked against
  `downloads.claude.ai/claude-code-releases/2.1.332/manifest.json`), Codex 0.172.0 from npm
  (`@openai/codex`, its `codex-linux-x64` binary), Node 22.23, on Linux 6.18.
