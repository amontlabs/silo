# Agent programs change under Silo: a contract test per release

Research of 25 and 26 November 2026, by an agent, for polarzero. `reading-each-agent-program`
says the boundary's check "runs again for each new version a recipe pins". This measures how often
Claude Code, Codex and Gemini CLI changed the shape of their model traffic between 25 May and
25 November 2026, and builds that check: a suite that runs one release of a program, unchanged,
through #374's boundary against stand-in providers, and says what the boundary read and what moved.
Everything ran on Linux in a private network namespace. Nothing ran on a Mac or against a real
provider.

| Folder | Holds |
|---|---|
| [`contract/`](contract/) | The suite. `run.sh <program> <version> <task\|refuse> <env\|store> <out>` runs one release; `check.py` checks it (13 checks, each naming the plan entry that rests on it) and compares its fingerprint with a baseline; `suite.sh` runs a list of releases in order, each against the one before |
| [`contract/standin.py`](contract/standin.py) | The providers: Anthropic Messages, OpenAI Responses over HTTP and over a WebSocket, Chat Completions, Gemini. The model asks for one shell tool call, then ends. Token counts are fixed by the request, so what the boundary counts can be compared with what was billed |
| [`contract/dns.py`](contract/dns.py) | Points every name at the boundary and writes each lookup, so a release that talks to a new host shows it |
| [`history/`](history/) | `sweep.sh` reads every release published on npm since 25 May; `markers.py` the traffic-shape strings each carries (betas, paths, headers, item types, hosts, tool names); `changes.py` counts the changes |
| [`results/suite/`](results/suite/) | One folder per run: the contract's report, the fingerprint, the difference from the release before, the lookups. The captures themselves hold prompts and stay out of the repository |

## Releases in six months

| Program | Stable releases, 25 May to 25 November | Dynamic runs |
|---|---:|---|
| Claude Code | 158 (2.1.152 to 2.1.332): about one a day | 8, one a month, plus the newest on a plan token, through the trust store alone, and refused |
| Codex | 69 (0.134.0 to 0.172.0), and about 6,000 pre-release tags | 8, the same way |
| Gemini CLI | 34 (0.44.0 to 0.71.1) | 8, the same way |

## In short

1. **Claude Code changes something the boundary sees in most releases, and never yet what it
   reads.** Between each monthly release run, the betas it asks for changed every time (13 added,
   4 dropped in six months), its tools changed in 5 of 7 steps, and it added 3 headers and new body
   fields (`diagnostics`, `thinking.display`). The path, the stream format, the block types and the
   conversation key (`metadata.user_id` → `session_id`) stayed the same: all 13 checks pass on all 8
   releases, with an API key, with a plan token (`Authorization: Bearer sk-ant-oat…`, read as a plan
   credential), and with Silo's authority only in the computer's trust store (no
   `NODE_EXTRA_CA_CERTS`). Of the 61 releases swept statically to 3 August, 25 changed at least one
   marker.
2. **Codex changed what the boundary reads three times in six months.** 0.142.5 moved the session
   into `client_metadata`; 0.146.1 moved every tool into one `additional_tools` input item (code
   mode, #399) and dropped the request's `tools`; 0.167.0 added `custom_tool_call_output`. Each shows
   as a failed `shape-unchanged` against the release before. A reader written for May's Codex
   misses all of its tool calls since 0.146.1.
3. **Codex's model calls go over a WebSocket by default, with an API key too, and #374's boundary
   neither counts nor refuses them there.** With a stand-in that accepts the WebSocket (#374's
   refused it, so Codex fell back to HTTP), every Codex release from 0.134.0 to 0.172.0 sent all its
   calls on one socket. The boundary records no call in its ledger, only the last response's tokens
   when the socket closes: 0.172.0 was billed 17,817 input tokens and counted 593. Under a budget of
   one millionth of a dollar, Codex 0.172.0 finished its task and exited 0 with no refusal
   (`refuse` run). #385 and #395 list admission inside a WebSocket as not built; this shows it is
   Codex's default path, so a budget does not hold Codex today.
4. **Codex `exec` reads its key from `CODEX_API_KEY`, not `OPENAI_API_KEY`.** With only
   `OPENAI_API_KEY`, releases 0.134.0 to 0.172.0 sent their model calls with no credential at all,
   and #374's boundary let them through: its check of a computer's credentials refuses an unknown
   credential but not a missing one (`main.zig`, `if (credentialOf(...)) |cv|`). A recipe that
   writes the wrong variable is a silent failure only a contract run catches.
5. **Gemini CLI did not change its shape in six months, and two things are not read.** All 8
   releases send the same paths, parts and tools. #374's tool-call watcher reads no Gemini
   `functionCall` (0 of 1 on every release), and with an API key Gemini CLI sends no conversation
   identifier, so `recognising-a-conversation`'s recommended option starts each compaction at zero
   for it. A refusal in Gemini's shape stops it with Silo's sentence (`refuse` passes).
6. **Every program looks up hosts besides its model's.** Codex looks up `chatgpt.com`,
   `ab.chatgpt.com`, `github.com` and `api.github.com` (refused by the boundary); Gemini CLI
   `play.googleapis.com`; Claude Code only `api.anthropic.com`, where it also calls about ten
   `/api/…` paths that are not model calls.

## What it changes in the plan

| Entry | Change |
|---|---|
| `reading-each-agent-program` | Notes: the check now exists and what it found. With Claude Code releasing about daily and updating itself, "each new version a recipe pins" is only a check if the computer runs the pinned version: see the choice in the notes |
| `recognising-a-conversation` | (to add) Gemini CLI with an API key sends no session; Codex moved its session in 0.142.5 |
| `what-a-budget-promises`, `budget-denies-or-holds` | (to add) No budget holds Codex while admission inside a WebSocket is not built |

## Not verified

- Signing in from scratch and token renewal: the suite uses fake keys and a fake plan token.
- Codex 0.146.1 to 0.160.0 did not run the stand-in's tool call (`runs-the-tool-call` fails): the
  stand-in's code-mode answer is the likely cause, not the program. Not looked into.
- The static sweep stopped at Claude Code 2.1.221 and Codex 0.161.0 when the session ended; the
  dynamic runs cover the whole six months. Gemini CLI's markers read the same in every release.
- Release notes and changelogs were not read.
- The boundary under test is #374's; #395's has the same WebSocket and credential code.
