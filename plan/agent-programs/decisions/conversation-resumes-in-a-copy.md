# Does each agent program take a conversation up again in a copied computer

## The choice

An agent's part, 22 November 2026. You fork main at 14:02 while Claude Code is 40 turns into work on
the login page. In the fork you run `claude --resume`, pick that conversation and go on from turn
40, while the same conversation goes on in main.

The options:

- Checked for each version (recommended). For each version of Claude Code and Codex that Silo's
  recipes pin, a live check takes a conversation up again in a fork, after a restore, and in a fork
  and its parent at once. A version that passed shows that its conversations can go on in a fork.
  One that failed, or was not checked, shows its conversations as staying in the computer where they
  started. It costs a check with a real account and real model calls for each new version, and a new
  version waits for its check before Silo says it works.
- Assumed to work. It costs surprises you find in the middle of work: `--resume` in a fork that
  shows nothing, or two copies of a conversation that change each other's turns.

Recommended: checked for each version. Going on with a conversation in a fork is a promise that only
a check on that version can back.

## A first check, with a copy of the disk (24 November 2026)

An agent's part, 24 November 2026. There was no virtual machine to fork here, so a copy stood in:
the program's home and the work folder copied whole, then mounted over the same paths in a mount
namespace of their own. With a fake model, each program took one turn with a tool call, then the
same conversation was taken up in the parent and in the copy at the same moment, twice in the
copy, and once more in the parent. The scripts are in the project's files, under
`research/2026-11-24-fork-resume/`.

- **Claude Code 2.1.331** (`claude -p --resume <id>`): yes, on every count. Both took the
  conversation up, each request carried the earlier turns, and each side's next turn carried its
  own turns and not the other's. A file written in the copy stayed in the copy.
- **Codex 0.172.0** (`codex exec resume <id>`): yes, on the same counts.

What this does not cover: a sign-in. The fake model takes any key, so whether a login copied with
the disk still works, or is broken by the other side refreshing it, is not tested.

**Both sides send the same identifiers.** Parent and copy keep the conversation's id:
`x-claude-code-session-id` and the `session_id` and `device_id` in `metadata.user_id` for Claude
Code, and `session-id`, `thread-id` and `prompt_cache_key` for Codex. To the provider, and to
anything at the boundary that keys on them, they are one conversation sent from two places. See
`recognising-a-conversation`.
