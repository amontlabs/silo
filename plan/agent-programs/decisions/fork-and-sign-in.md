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

## Two more sign-ins on disk (24 November 2026)

An agent's part, 24 November 2026, from the research in `research/2026-11-24-agent-programs-follow-ups`.

- **Ollama** makes a key at its first start, `~/.ollama/id_ed25519`, and signs every request to
  `ollama.com` with it: `Authorization: <public key>:<signature>`, over the method, the path and
  a time. After `ollama signin`, that key is the account's sign-in for cloud models. A fork copies
  it. A placeholder cannot stand in for it, since each request is signed afresh: kept out of the
  computer, it would have to be held by Silo, which signs each request, as the research on AWS
  signatures did.
- **Kiro CLI** keeps the sign-in of `kiro-cli login` in `~/.local/share/kiro-cli/data.sqlite3`,
  and its sessions in `~/.kiro/`. `KIRO_API_KEY` is the other way in.
- **LM Studio** keeps its hub sign-in under `~/.lmstudio/credentials`.

Each is a field for the program's data file (`agent-program-data-file`).
