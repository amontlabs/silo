# Does a fork carry the sign-in of an agent program

## The choice

An agent's part, 22 November 2026. Claude Code is signed in on main with your subscription, and you
fork main to let an agent try something risky. A fork copies the whole disk and the sign-in is a
file on it, so that agent now holds your sign-in, and the provider may sign one of the two copies
out.

The options:

- Left out unless you say (recommended). A fork starts without the sign-in, as it starts without
  secrets. You, or a rule you set, can let it carry one. It costs you a sign-in in each fork that
  needs one, and Silo has to know where each agent program keeps its sign-in. For a program Silo
  does not know, the fork may carry it, and Silo tells you before it makes the fork.
- Copied, and shown first. The sign-in is copied with the disk, and Silo tells you before the fork
  is made. It costs the promise polarzero took, that a login does not end up in a fork by accident, which
  becomes "never without you seeing it".
- Left open for now. Nothing is decided until logins are designed as a whole. It costs a promise
  that is not kept until then, and the plan has to say so.

Recommended: left out unless you say. A fork is where you put the agent you trust least. It already
starts with none of your secrets, so the sign-in follows the same rule.

This answers only what a fork carries. The rest of logins stays open in `logins`, as polarzero asked. The
audit's finding is in the notes of `login-handling`. It put the second option forward first, and
named this one as the stronger.

## Where each program keeps its sign-in (24 November 2026)

An agent's part, 24 November 2026. The answer needs Silo to know where each agent program keeps its
sign-in. This reads the shipped code of six programs on Linux. No program was signed in or run, so
nothing here says what a provider does when two copies use the same sign-in.

| Program | Its own sign-in | Other sign-ins it keeps | Moved by |
|---|---|---|---|
| Claude Code 2.1.331 | `~/.claude/.credentials.json`, in plain text with a warning; the keychain on macOS | in the same file: the tokens of MCP servers, plugin secrets, the cloud sessions grant. And `~/.claude/.device-keys.json`, a private key that names the device to Anthropic | `CLAUDE_CONFIG_DIR` |
| Codex 0.172.0 | `~/.codex/auth.json`, or the system keyring if `cli_auth_credentials_store` says so | `~/.codex/.credentials.json` for MCP servers, unless the keyring holds them (`mcp_oauth_credentials_store`, `auto` by default) | `CODEX_HOME` |
| Gemini CLI 0.71.1 | `~/.gemini/oauth_creds.json`, in plain text. With `GEMINI_FORCE_ENCRYPTED_FILE_STORAGE=true`: the keychain, or `~/.gemini/gemini-credentials.json` encrypted with a key made from the host name and the user name | `~/.gemini/mcp-oauth-tokens.json`, `a2a-oauth-tokens.json` | `GEMINI_CLI_HOME` |
| Qwen Code 0.28.4 | `~/.qwen/oauth_creds.json` (a fork of Gemini CLI) | as Gemini CLI | |
| opencode 1.19.6 | `~/.local/share/opencode/auth.json`, one file for every provider: API keys and OAuth | `mcp-auth.json` beside it | `XDG_DATA_HOME` |
| Amp | a `secrets.json`; where it lies was not found in the compiled program | | |

GitHub Copilot CLI 1.0.115 ships as a packed binary whose text could not be read without running it.

What this changes:

- **One file can hold the program's sign-in and your sign-ins to other services.** Claude Code
  keeps the Claude login and the tokens of every MCP server you connected (say Linear and GitHub)
  in one file. Leaving the file out of a fork leaves all of them out; keeping it keeps the Claude
  login. Silo cannot honour "left out unless you say" for the Claude login alone without editing
  inside the file, which ties Silo to its format. opencode is the same for API keys of every
  provider. The answer should say which of the two the fork gets: everything in the file left out
  (recommended, since a token for Linear or GitHub is a secret by any other name), or Silo edits
  the file.
- **The device key.** Claude Code keeps a private key in `~/.claude/.device-keys.json` that registers
  the machine with the account (it is used to bind the device and to re-enrol it). A fork that
  carries it is the same device to Anthropic as main. It belongs on the list beside the sign-in,
  and Claude Code itself already lists both files as ones it protects.
- **A fork should get its own machine identity.** Claude Code tells its own live processes apart by
  `/etc/machine-id` and the PID namespace; Codex and Gemini CLI keep an `installation_id`. A fork
  with main's `machine-id` looks to these programs like main itself. Gemini CLI's encrypted store
  goes the other way: a fork with a new host name can no longer read the copy, which is the harmless
  failure. `fork-identity` already gives a fork a new machine identifier and name; this is one
  more reason it must be done before anything in the fork starts.
- **Every program has a variable that moves its files.** A rule that sets `CODEX_HOME` or
  `CLAUDE_CONFIG_DIR` to a place Silo keeps out of copies would work for these programs without
  knowing their file names, but each program's default still has to be known for a user who sets
  nothing.

The scripts and searches are in the project files, `research/2026-11-24-logins`.

Why a copied sign-in signs one copy out, read from the same programs: Codex 0.172.0 carries the
message "Your access token could not be refreshed because your refresh token was already used.
Please log out and sign in again." OpenAI's refresh tokens work once. A fork that carries main's
`auth.json` and runs Codex first renews the login there, and main's next renewal fails with that
message. Claude Code 2.1.331, when a renewal is refused (`invalid_grant`), clears the login in its
file. So the second option, "copied, and shown first", costs more than the promise: it can sign
you out of main without anything in main having changed.

## A sign-in held outside the computer survives a fork (24 November 2026)

An agent's part, 24 November 2026. The note above reads from the code that a copied sign-in can
sign main out. Tried with Claude Code 2.1.331 and a fake MCP server that rotates its refresh
token on each renewal, as OAuth 2.1 asks of a server whose clients hold no secret. Main and a fork
held the same expired sign-in, and each ran `claude mcp list` in turn, the fork first.

- **The sign-in copied in the file:** the fork renewed and connected. Main's renewal then sent a
  refresh token the server had already replaced, was refused, and Claude Code cleared main's
  tokens from its file. Main showed "Needs authentication" and stayed so. Nothing was done in
  main.
- **The sign-in held by the boundary,** as in the trial of `secret-placeholder` ("An MCP server's
  sign-in, held outside"): both computers held only placeholders, the boundary swapped in the one
  real refresh token and kept each new one. Main and the fork both stayed connected through four
  renewals.

What this changes: when you let a fork carry a sign-in, the answer's own way out, Silo should not
copy the file. It should give the fork the same placeholders and keep one real sign-in outside
for both. One sign-in then renews in one place, so neither computer signs the other out, and
leaving the fork stops its use at once without touching main.

One sign-in shared this way needs one more thing at the boundary. Main and the fork ran
`claude mcp list` at the same moment, against a server that takes 0.3 s to renew. A boundary that
forwards each renewal sent the same refresh token twice: the server took the first and refused
the second, and Claude Code cleared the fork's placeholders, so the fork stayed signed out in every
round after, although the boundary still held a good sign-in. A boundary that lets one renewal
through at a time, and gives a renewal that comes just after another the same reply, kept both
computers connected through three rounds, and the server saw one renewal. So the answer should
say: a sign-in held outside renews once for every computer that uses it. The scripts are in the
project files, `research/2026-11-24-mcp-oauth` (`fork.sh`, `race_slow.sh`, `race2_slow.sh`).
