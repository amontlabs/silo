# Codex read in full: code mode, the ChatGPT sign-in, and its WebSocket under a budget

Research of 25 November 2026, by an agent, for polarzero. #399 found that Codex 0.172.0 hides its
commands from #371's boundary in code mode (one JavaScript `exec` tool in `additional_tools`), and
#403 got Codex signed in with ChatGPT through #378's closed network as far as its workspace lookup,
not to its model call. This reads Codex's own source at the tag `rust-v0.172.0`, takes the ChatGPT
sign-in to its model call, and runs Codex through #385's boundary, to say what the boundary must
answer, pass and read. Linux only, a stand-in for OpenAI and ChatGPT, every key and token fake.

| File | Holds |
|---|---|
| [`provider.py`](provider.py) | A stand-in for `api.openai.com`, `chatgpt.com` and `auth.openai.com`, grown from #403's: the workspace check Codex accepts, a scripted model that calls a tool, every model request kept with its reply, and `DENY` to refuse chosen requests |
| [`night.js`](night.js) | The script the stand-in model sends in code mode: a loop, a command built at run time, a patch |
| [`run.sh`](run.sh), [`inside.sh`](inside.sh), [`chatgpt.sh`](chatgpt.sh) | Codex through #378's closed network (#403's proxy and relay, copied): key or ChatGPT, function or code mode, the report on or off, a renewal, and each ChatGPT request refused in turn |
| [`read.py`](read.py) | What a boundary can read of each tool call, from the kept requests and replies |
| [`boundary.sh`](boundary.sh) | Codex through #385's Zig boundary, with a budget |
| [`boundary/websocket-admission.patch`](boundary/websocket-admission.patch), [`boundary/run.sh`](boundary/run.sh) | The fix to #385's boundary, and the run of every case before and after it |
| [`results/`](results/) | The output of each, as read below |

## In short

1. **Codex signed in with ChatGPT now reaches its model call.** #403's stand-in lacked one field:
   the workspace check must answer `account_routing_override` as `NO_CONSTRAINT`, `us` or `us_cr`
   (`app-server/src/request_processors/account_processor/workspace_routing.rs`). With it, Codex
   opened its WebSocket to `chatgpt.com/backend-api/codex/responses` through `CONNECT` and ran a
   whole turn with a tool call, in function mode and in code mode, and after renewing an expired
   token at `auth.openai.com/oauth/token` (`results/closed-network.tsv`).
2. **What the boundary must pass: three requests.** Each other request was answered 403 in turn
   (`results/deny-each.tsv`). Codex worked without the model list, plugins, MCP, user settings,
   analytics and `ab.chatgpt.com`'s metrics, one at a time and all together. It did not start
   without `GET chatgpt.com/backend-api/wham/accounts/check` ("workspace routing discovery
   failed", after 15 tries). So: the workspace check, the model call, and the token renewal. With
   an API key, only the model call.
3. **A budget did not hold on Codex's WebSocket.** #374 and #385 measured Codex with a stand-in
   that refused its WebSocket, so it fell back to plain requests. With the WebSocket accepted, as
   OpenAI does, #385's boundary read the three `response.create` messages and counted none: under
   a $0.0001 budget with $0.00016 calls, Codex ran its tool and ended its turn
   (`results/boundary.txt`, "before gpt" and "before key"). The patch admits each message as a
   call before it leaves and refuses with an `error` event of status 400: the warm-up was counted,
   the budget reached, the next call refused, Codex stopped with Silo's sentence, and the tool
   never ran ("after gpt", "after key"). Without a budget the same turn passes and all three calls
   are counted ("after gpt-nobudget").
4. **A renewed ChatGPT sign-in is refused as a foreign credential.** #385's boundary knows a
   computer's credential by its exact value. Codex renews its access token (at least every 8 days,
   and on a 401), and with the old value in the config all 30 of its requests were refused; it
   could not start ("before gpt-rotated"). Matching the account id the token names instead passed
   ("after gpt-rotated"), and a unit test shows another account's token still refused.
5. **In code mode the commands never cross the boundary as tool calls, unless Codex reports them.**
   The model sends JavaScript; Codex runs it in a V8 isolate where `tools.exec_command`,
   `tools.apply_patch`, `tools.write_stdin`, `tools.view_image` and every MCP tool
   (`tools.mcp__<server>__<tool>`) are functions, and only what the script prints comes back.
   Codex has a report, off by default and marked under development: with
   `features.executed_tool_call_metadata = true`, the result of each `exec` carries
   `internal_chat_message_metadata_passthrough.executed_tool_calls`, every nested call with its
   arguments as they ran, and `tool_calls_complete` (`results/read.txt`). On `night.js` it gave
   all four calls; reading the script gave three places, two with arguments unknown. Codex keeps
   the report only for OpenAI's own hosts and strips it for any other provider
   (`core/src/client.rs`), so #399's runs, which used a custom provider, could not have seen it.
6. **Over the WebSocket the model's calls are in the reply, not the next request.** Codex sends
   only what is new since `previous_response_id`, so a `custom_tool_call` is never sent back; the
   boundary must read calls from the host's messages and results from the program's. #385's
   boundary already does (`tool-call` lines in the record).

## What Codex sends, from its source

- **Tools.** For gpt-5.5: `tools` holds `exec_command`, `write_stdin`, `apply_patch` (a custom
  tool with a grammar, not JSON), `view_image`, goals, `tool_search`, `web_search`. For
  gpt-6.2-sol: no `tools`, and one `additional_tools` input item with three namespaces:
  `functions` (`exec`, a custom tool whose input is JavaScript with an optional first line
  `// @exec: {...}`; `wait` for a script still running; `request_user_input`), `clock` (`sleep`)
  and `collaboration` (`spawn_agent`, `send_message`, `followup_task`, `wait_agent`, ...). The
  agent tools stay top-level calls, so spawning a subagent is visible as one.
- **Inside a script.** `text()`, `image()`, `notify()` (an extra `custom_tool_call_output` at once),
  `store()`/`load()` across scripts, `yield_control()`. No Node, file system or network of its
  own: everything outside goes through a nested tool, which Codex runs as it would a direct call,
  with its own policy (it refused `rm -f` even with approvals and sandbox bypassed,
  `core/src/exec_policy.rs`).
- **Headers.** The model call carries `originator: codex_exec`, `version`, `openai-beta:
  responses_websockets=2026-02-06`, and with ChatGPT `chatgpt-account-id` and the bearer token
  (`results/requests-by-case.txt`). `x-openai-account-routing-override` was not sent for
  `NO_CONSTRAINT`. The body's `client_metadata` holds Codex's session, thread, turn and window ids,
  which `recognising-a-conversation` can use.

## What it changes in the plan

| Entry | Change |
|---|---|
| `code-mode-calls-reported` (new, traffic, after `reading-each-agent-program`) | Main's set-up turns on Codex's report of a script's calls; the boundary reads it as a report, and a script without it is recorded as not read. Recommended over reading the script only, or running it in a copy |
| `reading-each-agent-program` | The ChatGPT sign-in to its model call, the three requests that must pass, renewal against a check by value, code mode |
| `what-a-budget-promises` | Codex's WebSocket went past a budget in #385's boundary; the patch and what it does not cover |
| `foreign-credential-refused` | A sign-in is known by its account, not by one value of its token |

Notes were added as "An agent's part, 25 November 2026". No verdict, answer of a taken entry, or
list was changed.

## For polarzero to rule

- `code-mode-calls-reported`: turn on Codex's own report in main and read it as a report.
  Recommended.

## What was not verified

- No real OpenAI or ChatGPT host was reached; there is no key or sign-in here. The workspace
  check's answer is the least Codex accepts, not a captured one. Whether OpenAI's API accepts the
  report's field from a client that turned it on, and whether a real warm-up (`generate: false`)
  is billed, were not tried.
- The stand-in offers HTTP/1.1 and no WebSocket compression; Codex asked for
  `permessage-deflate`. The patch's handling of a refused compressed message (it ends the socket)
  was not run against a host that agrees to compression.
- The patch does not cut a reply on the socket when a budget is reached mid-stream, and was not
  fuzzed; #401's `ws_final_block.patch` touches the same reader and was not merged with it.
- A real gpt-6.2-sol may write scripts unlike `night.js`. The report covers whatever ran; the
  script reader in `read.py` is a literal one and is there to show what is lost without it.
- `auth.openai.com` was passed by the proxy in the closed network; #385's boundary, which refuses
  hosts it does not read, would have blocked the renewal had it been routed there.
- Nothing ran on a Mac.

## Run it

Linux with unprivileged user namespaces, Python 3 with `zstandard`, OpenSSL, Zig 0.15.2 with zlib
and zstd, and Codex 0.172.0:

```sh
npm install --prefix /tmp/codex @openai/codex@0.172.0; export CODEX=/tmp/codex/node_modules/.bin/codex
./run.sh                                              # every closed-network case, out/results.tsv
python3 read.py out/bodies gpt-code-meta              # exits 0; gpt-code exits 1
boundary/run.sh /tmp/brun <ref of #385's branch>      # before and after the patch
```

## Sources

- Codex 0.172.0 from npm, and its source at `rust-v0.172.0`: `login/src/auth/manager/workspace_routing.rs`,
  `app-server/.../workspace_routing.rs`, `core/src/tools/code_mode/`, `core/src/tools/executed_tool_calls.rs`
  and `request_metadata.rs`, `core/src/client.rs`, `features/src/lib.rs`, `core/src/exec_policy.rs`.
- #403's proxy, relay and provider; #385's boundary at its head; #399's README and `codex_tools.sh`;
  `how-a-hold-ends` for the refusal Codex shows on its socket.
