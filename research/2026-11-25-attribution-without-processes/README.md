# Attribution without processes: what option A can say from model traffic alone

Research of 25 November 2026, by an agent, for polarzero. Option A of #373 records model traffic,
file changes between checkpoints and connections, and no processes. #382 found that its first
morning names `codex` as the author of a deletion it cannot attribute (its gap 12), and #373's own
spec says "which conversation changed a file is not claimed". This measures what A can claim
instead, from the tool calls and tool results in the traffic, against what #371's probe saw on the
same night.

| File | Holds |
|---|---|
| [`attribute.py`](attribute.py) | The attributor: each file fact and connection to a conversation and a tool call, from facts alone, scored against #371's probe-based `which-run-did-it@0.1` |
| [`commands.py`](commands.py) | Exit codes and "still running" read from tool results, against the probe |
| [`patch_proto.py`](patch_proto.py) | Copies #371's prototype and changes it: the scripted model speaks Codex's code mode; `--fix` makes the boundary read it; `--one-main` runs Codex in main with the others |
| [`nights.sh`](nights.sh) | The six nights below |
| [`checks/tool_calls_read.py`](checks/tool_calls_read.py) | Exits 1 when a conversation's tool calls are missing from the record |
| [`checks/codex_tools.sh`](checks/codex_tools.sh) | Which tools Codex 0.172.0 offers each model, from its first request |
| [`results/`](results/) | The output of each, as read below |

## The nights

Each is #371's night: the real Claude Code 2.1.332 and the real Codex 0.172.0 against #371's
scripted model, four runs, a checkpoint every 15 s, strace as the probe. Nothing ran on a Mac.

| Night | Codex | Where Codex runs | Boundary |
|---|---|---|---|
| `exec-command` | gpt-5.5: tools `exec_command`, `apply_patch` | a fork, as in #371 | `--fix` |
| `code-mode` | gpt-6.2-sol: one JavaScript `exec` tool | a fork | #371's, unchanged |
| `code-mode-fix` | gpt-6.2-sol, the script prints the output only | a fork | `--fix` |
| `code-mode-json` | gpt-6.2-sol, the script prints the whole result | a fork | `--fix` |
| `one-main` | gpt-5.5 | main, with the three Claude Code runs | `--fix` |
| `one-main-code-mode` | gpt-6.2-sol, output only | main | `--fix` |

`one-main` is option A's usual case: every agent works in main, at the same time.

## In short

1. **From traffic alone, 453 of the 461 file changes the probe attributed on `one-main` name the
   right conversation, and none names a wrong one.** The rules, strongest first: a tool call sent
   the file's whole content (1), named the path (32), named a folder above it (400, one `rm -rf`),
   or ran a command known to write it (20, git). With one checkpoint in 16 (11.5 tool calls open
   per window), it is still 453 right and 0 wrong. The tool call is right for 452.
2. **What stays unattributed is what a command does without naming it.** On `one-main`: the bun
   installer appending to `~/.bashrc` (`curl … | bash`), and the app's `data/sessions.json`, written
   by tests and a daemon three conversations started. A probe is needed for those, and they are
   exactly the kind a user asks about: the `~/.bashrc` change raised the `agent-settings` alert,
   which would show no author.
3. **Codex 0.172.0 hides its commands from #371's boundary when the model is gpt-6.2-sol.** Codex
   then sends no `tools`, but one `additional_tools` input item with a custom JavaScript tool `exec`
   that calls `tools.exec_command(...)`. #371's boundary read 0 of 4 tool calls and marked the whole
   conversation as a side one (`checks/tool_calls_read.py` exits 1 on `code-mode`). Reading
   `custom_tool_call` items and `additional_tools` fixes it (exits 0 on `code-mode-fix`); the
   attribution then matches the function-call night. `apply_patch` is a custom tool for gpt-5.5
   too, which #371's boundary would also miss.
4. **In code mode, the exit code and "still running" are in the traffic only if the model's script
   prints them.** With `text(r.output)`, 13 of 18 commands have an exit code in the traffic instead
   of 16, and Codex's `bun test --watch` left running is not said. With `text(JSON.stringify(r))`
   both are back. Silo cannot choose what the model prints.
5. **"Still running" is partly in the traffic.** A tool call with no result yet is still running
   (claude #3's foreground daemon), and a command with `nohup`, `setsid`, `disown` or a trailing
   `&` may leave a process its result does not mention (claude #1's daemon). Both processes the
   probe found left running are flagged that way, on every night. One more flag is wrong: Codex
   said `bun test --watch` was still running, and `codex exec` killed it when it ended.
6. **The program's session identifier names its own files.** Claude Code's transcripts are named by
   the `session_id` in `metadata.user_id`, Codex's by its `prompt_cache_key`. #371's boundary used
   it to join conversations but did not write it. With it kept, 33 of the programs' own file changes
   on `one-main` name their conversation by it, and 97 more because one conversation of that
   program was running; 9 stay unattributed, named by a process id (`~/.claude/sessions/<pid>.json`)
   or by none. None is wrong.
7. **Exit codes from the traffic equal the probe's whenever both exist:** 16 of 16 on the
   function-call nights, 13 of 13 in code mode.

## What it changes in the plan

| Entry | Change |
|---|---|
| `author-from-traffic` (new, record, after `what-a-receipt-holds`) | Until Silo sees processes, a change names its conversation as an opinion when one conversation's tool call sent its content, named it or a folder above it, or ran a command known to write it. Recommended over "not claimed" (#373) and over "any conversation that was running" |
| `reading-each-agent-program` | Code mode: `additional_tools`, the custom `exec` tool and `apply_patch`; what its results carry; `spawn_agent`'s task is sent `encrypted` |
| `what-a-receipt-holds` | Under A, "still running" from open tool calls and backgrounding commands instead of "not known yet"; the exit code is a report from the traffic and can be missing in code mode |
| `recognising-a-conversation` | Keep the program's session identifier on every model-call fact |
| `the-chain` | A link the table lacks under A: file change → tool call, from the tool call's input, an opinion |

Notes were added to each as "An agent's part, 25 November 2026". No verdict, answer of a taken
entry, or list was changed.

## Against #373 and #382

#382's gap 12 recommends that A's first screen name no program on the deletion and show the tool
call beside it. That is right for the paste site, and for `~/.bashrc` here. For the deletion it
leaves out what the record knows: the only open tool call that names `~/work/shared-fixtures` is
`rm -rf ~/work/shared-fixtures` in one conversation, and on six nights that rule never named a wrong
conversation. #373's sentence "which conversation changed a file is not claimed" should become the
rule of `author-from-traffic`, with the line marked ◇ and its tool call shown.

## For polarzero to rule

- `author-from-traffic`: name the conversation from a tool call that sent the content, named the
  path or a folder above it, or ran a known writer; everything else stays "while these ran".
  Recommended.

## What was not verified

- The night is #371's, scripted: four runs that mostly touch different paths. A real night with
  two agents in one repository will have more shared files (lockfiles, build output, test data),
  which this rule leaves unattributed, as `matching-reports-to-facts` does for the probe. The
  share of those on a real night was not measured.
- The probe is the reference, not the truth. Where it said unattributed, the fact was not scored.
- Checkpoints were made coarser by dropping them, on a night compressed to minutes. Whether 11.5
  open tool calls per window is what a 5-minute checkpoint sees on a real night was not measured.
- The code-mode scripts are written by the scripted model in two fixed shapes. A real gpt-6.2-sol
  may build commands in loops or from variables, which the literal reader does not follow. No real
  model ran.
- Claude Code's subagents (`Task`) and Codex's `spawn_agent` did not run. Codex sends a subagent's
  task `encrypted`, so the boundary cannot read what it was asked.
- Nothing ran on macOS. Paths on a Mac are `/Users/<name>`; the rules do not depend on it.

## Run it

Python 3, Node, strace, `claude` on the PATH, and Codex 0.172.0:

```sh
npm install --prefix /tmp/codex @openai/codex@0.172.0
CODEX_BIN=/tmp/codex/node_modules/.bin/codex ./nights.sh /tmp/nights        # six nights, about 2 minutes each
python3 attribute.py /tmp/nights/one-main [--coarsen 16] [--wrong --none]
python3 commands.py /tmp/nights/one-main -v
python3 checks/tool_calls_read.py /tmp/nights/code-mode                      # exits 1
```

## Sources

- #371's prototype, `research/2026-11-24-morning-review/proto`, at #391's head, read and run
  unchanged apart from `patch_proto.py`.
- #373's `specs.md` ("Step 5: the record"), #382's README and transcript (gaps 6 and 12), #374's
  note that Codex sends "one JavaScript `exec` tool in an `additional_tools` input item".
- Codex 0.172.0's first request for four models, captured by `checks/codex_tools.sh`.
