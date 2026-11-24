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

## Local models (24 November 2026)

An agent's part, 24 November 2026. A model served from your Mac, rather than inside the computer,
does cross the boundary, in plain HTTP. Aider 0.86.2 with an `ollama_chat/` model sent
`POST /api/chat` to port 11434, Ollama's own API, whose reply is a stream of JSON lines rather than
of events. Goose 1.60.2 with Ollama sent Chat Completions to the same port. LM Studio's port is
1234. So Silo can read a model on the Mac without any certificate, with a reader for Chat
Completions and one for Ollama's lines. Whether to add the second is open. A model the agent runs
inside the computer sends nothing across, as this entry says.

## Programs that make another provider one setting away (24 November 2026)

An agent's part, 24 November 2026. Cline (`-P openrouter`), OpenCode (a model in its settings),
Qwen Code (`OPENAI_BASE_URL`) and Aider (`--model`) each switch to another provider with one
change that an agent with full access can make itself. Traffic to a provider Silo does not read is
outside spend and budgets, as this entry says. So under a budget, the rule that denies model
traffic Silo does not read (the research for #351) has to be on by default, or the budget is one
setting away from not counting.

## Local models, measured further (24 November 2026)

An agent's part, 24 November 2026, from the research in `research/2026-11-24-agent-programs-follow-ups`.

- **What a local server receives.** Pointed at a server on Ollama's or LM Studio's port, Claude
  Code sent Anthropic Messages (`ANTHROPIC_BASE_URL`), Codex sent Responses (`--oss`, with Ollama
  or LM Studio), Goose and Copilot CLI sent Chat Completions, and Aider and Cline sent Ollama's own
  `/api/chat`. Ollama 0.40.18 serves all four, and LM Studio's headless server serves the first
  three and its own API. So reading a local model takes the readers Silo has, plus Ollama's lines.
- **Codex downloads the model through the server.** With `--oss` and a model the server lacks,
  Codex asks Ollama to pull it (`POST /api/pull`), and Ollama downloads it from
  `registry.ollama.ai`, many gigabytes, through the boundary.
- **Ollama's cloud models are model traffic like any other.** Asked for `gpt-oss:120b-cloud`,
  Ollama's own server in the computer sent the request on to `ollama.com` over TLS, trusting the
  system's store, in Ollama's own format. So a cloud model run through a "local" server is read,
  priced and budgeted as a provider, with the reader of Ollama's lines. `OLLAMA_NO_CLOUD=1` turns
  cloud models off in the server.
- **LM Studio can run its "local" model elsewhere.** `lms link` joins the LM Studio of another
  machine.

### A question this raises: a model on your Mac

`network-modes` says that open never includes your device. So Ollama or LM Studio running on the
Mac, where the Mac's GPU is, cannot be reached from a computer, and the Linux computer has no use
of that GPU to run the model itself. Someone who works with local models today loses them in Silo.

A real case: you run `qwen3-coder` in Ollama on your Mac and want Aider in main to use it. The
options:

- A channel for a model on your Mac. One port of the Mac, named by you (11434 or 1234), is
  forwarded into the computers you choose, as a channel of `channel-list`. The traffic is plain
  HTTP, so Silo reads it without a certificate and records each call with its tokens at no price.
  It costs a path from the computer to a program on your Mac, which an agent can use for anything
  that server answers, such as pulling a model or, with LM Studio, reaching linked machines.
- Nothing on the Mac. A local model runs inside the computer, on its processor. It costs speed:
  the Mac's GPU is out of reach.

Recommended: the channel, off by default, one port at a time. A model on the Mac is a common way to
work without paying a provider, and its traffic is easier to read than any provider's.
