# The record

What Silo writes down about a computer: facts and reports, written once.

## For

polarzero asked for a good system to track what happens in a computer: which agent made which
calls, when, with which secret, at what cost, which files it wrote and deleted. On top of it, a
way to make sense of it: which session wrote what, what might be a problem, what takes space and
can be purged. polarzero called this the core.

## Why it is so

- **Two ways into the record, kept apart by how Silo knows** (`words-for-what-silo-knows`,
  `what-the-record-holds`, `what-a-report-carries`). What a program inside says can be interfered
  with by the agent, so it is never taken for what Silo saw from outside. Opinions are computed on
  top, in the part `opinions`.
- **The books balance** (`books-balance`, `what-runs-wrote`), because "best effort" compounds
  (`holes`).
- **One chain from the computer down to each trace** (`the-chain`), so that an alert opens on what
  triggered it, and on the turn that led to it.
- **Noise and repetition are left out** (`file-reads`, `store-messages-once`).

## One example

A file in a computer changed. The disk changed: a fact. The probe says `cargo build` wrote it: a
report. `cargo build` was started by the second launch of Claude Code: an opinion. That launch
asked for the tool call "cargo build": the call is a fact, and tying it to the process is an
opinion (`the-chain`).
