# The first run

## The choice

An agent's part, 22 November 2026. You install Silo on your Mac. With no account and no key, the
first run checks your disk and network, starts the download of macOS, and asks its questions while
it waits: which agent programs to set up, and whether to switch on the rule that lets Claude Code
reach Anthropic.

The options:

- Build it (recommended). Main is ready to work when the install ends, with nothing set up that was
  not checked. It costs a few questions on the first run, and a yes before any agent can reach its
  provider.
- Leave it out. You make main, install your agent programs and write their rules yourself, from a
  guide. It costs that setup before the first agent works.

Recommended: build it. Every agent needs at least one rule before it can reach its provider, so a
first run that sets nothing up leaves you with agents that cannot work.

## What each program asks on its first start, with a key from Silo (24 November 2026)

An agent's part, 24 November 2026. When Silo gives a fresh computer a placeholder key, each agent
program still asks its own first questions. Tried with Claude Code 2.1.331 and Codex 0.172.0 in a
fresh home folder, pressing Enter on each screen as a hurried person would.

- **Claude Code with the key in `ANTHROPIC_API_KEY`** asked four things: a colour theme; "Detected a
  custom API key in your environment. Do you want to use this API key?", with **No** chosen
  beforehand and marked "recommended"; security notes; and whether you trust the folder, with
  **No, exit** chosen beforehand. Enter on each declines the key, then quits.
- **With the key from `apiKeyHelper`** in Claude Code's `settings.json` instead, a documented
  setting that runs a command printing the key, the question about the key is gone. The theme,
  the notes and the folder question remain.
- Claude Code skipped every screen when its own state file `~/.claude.json` already said they
  were answered, but those fields are not documented, so `documented-surfaces` rules them out.
- **Codex with the key in `OPENAI_API_KEY`** did not use it: it offered "Sign in with ChatGPT",
  chosen beforehand, and Enter opened a browser sign-in. After `codex login --with-api-key`, given
  the placeholder, it went straight to its prompt, and the placeholder sat in its `auth.json`.

What this changes for the answer: "Main is ready to work when the install ends" holds only if Silo
gives each program its key the way the program takes it without a question: `apiKeyHelper` for
Claude Code, `codex login --with-api-key` for Codex. Both belong in the program's piece
(`recipe-pieces`). Claude Code's three remaining questions are asked once per computer, in the
program's own terminal, and a fork carries the answers with the disk. The scripts are in the
project files, `research/2026-11-24-first-run-screens`.
