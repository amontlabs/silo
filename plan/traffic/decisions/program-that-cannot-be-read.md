# What does Silo know of an agent program whose traffic cannot be read

## The choice

An agent's part, 22 November 2026. You try a new agent program in main. It talks to a model provider
whose format Silo cannot read yet. Main has a budget of 50 dollars.

The options:

- What Silo sees, and no more (recommended). The record holds the hosts, the sizes and the timing of
  that traffic. The app says that this program's conversations are not read, and main's budget does
  not count what it spends. It costs turns, tool calls and spend for that program until Silo reads
  its provider.
- Also read the program's own files. Silo reads the conversation the program keeps in the computer
  and shows it, marked as told by the program. It costs a reader for each program, kept up with each
  of its versions, and an agent can rewrite those files.

Recommended: what Silo sees, and no more. A few providers carry nearly all model traffic, so reading
a new provider covers every program that uses it, where a reader of files covers one program.

## A model on your Mac, designed and tried (25 November 2026)

An agent's part, from `research/2026-11-25-local-models`. It answers the question above with a
design and a prototype against stand-ins of Ollama and LM Studio (no real server ran).

- **The channel.** You grant one computer one model server of your Mac. Inside, it gets
  `models.silo.internal:11434`. The boundary, on the Mac, is the only one that dials the Mac's
  loopback, with the Host that server expects and no Origin.
- **Only calls that run or list a model pass.** Pulling, pushing, creating, copying and deleting a
  model, and LM Studio's download and load, are refused with a 400 in the route's format and never
  reach the Mac. Codex asking Ollama to pull a missing model (#369) is the real case.
- **Cloud models are refused**, by name (`-cloud`) and by what Ollama's `/api/show` says
  (`remote_host`), which catches a model made with `ollama create` from a cloud model.
- **Memory is capped.** `keep_alive` over 5 minutes or `-1`, and a context over 65,536 tokens, are
  lowered, and the record says so.
- **Every call is recorded** with its tokens, read from all four formats the programs use. One gap:
  Chat Completions streamed without `include_usage` carries no usage.
- About 0.1 ms is added per call on loopback.

This touches `nothing-shared-from-your-device`, a property in force: "Nothing from your device is
shared into a computer." The channel shares a server of the Mac into a computer. It can be built
only if polarzero changes the property, for example to "nothing from your device is shared into a
computer unless you grant it, one channel at a time, through the boundary". Until then the
property holds and the channel waits.

Recommended: the channel, off by default, any model already on the Mac, cloud models refused.
