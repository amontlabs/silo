# Do Silo's adapters follow agent programs or model providers

## The choice

An agent's part, 25 November 2026, from #367 and #369, which ran eleven agent programs through a prototype of
the boundary.

The case: Aider sends Chat Completions to `api.openai.com`, and Codex, Cline, OpenCode and Goose
send OpenAI's Responses to the same host. A reader chosen by host, one per provider as the earlier
answer reads, picks the wrong reader for one of them. The prototype now chooses by path
(`/chat/completions`, `/responses`, `/messages`, `generateContent`), and every program that ran was
read.

The options:

- Readers follow formats, chosen by the request's path (recommended). The four formats are named.
  Ollama's own lines, for a model served from your Mac, would be a fifth; `program-that-cannot-be-read`
  and the open draft `vendor-runs-the-model-calls` (#369) cover what is not read. It costs nothing
  the earlier answer did not already pay: still a few readers, not one per program.
- Readers follow providers, as the earlier answer says. It costs Aider, or the four Responses
  programs, on `api.openai.com`.

Recommended: formats, chosen by path. It is what the earlier answer meant by "three or four
formats"; the change is that a host no longer picks the reader.
