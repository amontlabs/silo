## The choice

An agent's part, 25 November 2026, from `research/2026-11-25-codex-read-in-full/`.

**The case.** Codex 0.172.0 with gpt-6.2-sol offers the model no `exec_command` of its own. It
offers one custom tool, `exec`, in an `additional_tools` input item: the model writes JavaScript,
and Codex runs it in a V8 isolate where `tools.exec_command(...)`, `tools.apply_patch(...)` and
every MCP tool are functions. The commands then never cross the boundary as tool calls. One
script of the experiment wrote two files in a loop, renamed one with a command built by
`["mv", "-v"].join(" ")` and added a third with a patch. Read from its source without running it,
it shows three places that call a tool, and the argument of two of them is unknown. Four calls
ran.

| Option | What the boundary knows of that script | What it costs |
|---|---|---|
| A. Turn on Codex's report (recommended) | All four calls, with the command each ran, in `internal_chat_message_metadata_passthrough.executed_tool_calls` of the next request, and `tool_calls_complete: true` | One line in main's Codex config. It is Codex's word, a report and not a fact; an agent inside can turn it off, and a missing report is then visible. The feature is marked under development in Codex and may change |
| B. Read the script only | Three places, two arguments unknown; a loop's count unknown | Nothing to set; `author-from-traffic` loses every change made by a command built at run time |
| C. Run the script in a copy to see what it would call | Whatever a copy shows, which can differ from what ran (time, files, network) | A JavaScript sandbox in Silo for each script; still a guess |

A is recommended. Codex keeps the report only for OpenAI's own hosts (it strips it for any other
provider), which is where the boundary reads it; it was there with an API key and with a ChatGPT
sign-in, over the WebSocket. With the feature off, as Codex ships it, no request carried it.

**What A sets in main:** `[features] executed_tool_call_metadata = true` in `~/.codex/config.toml`,
written with the rest of main's set-up (`main-set-up-for-agents`). Whether OpenAI's real API
accepts the field from a client that turned it on was not tried: no key here.
