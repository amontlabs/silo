# How does Silo integrate with an agent program

## The choice

An agent's part, 22 November 2026. Claude Code keeps each conversation in a file on the disk, in a
format Anthropic does not promise to keep. Reading those files would let Silo show the conversation,
and the next update of Claude Code can change them.

The options:

- Only what the vendor documents (recommended). Silo uses a program's options, settings, hooks and
  the files its documentation names, and nothing else. It costs features: where a vendor documents
  nothing, Silo does without, and says so for that program.
- Whatever works. Silo also reads a program's private files and changes the program where that
  helps, with tests that catch a change. It costs breakage after a vendor's update, until Silo
  catches up, and it can go against a vendor's terms.

Recommended: only what the vendor documents. Silo already sees what an agent does from outside, in
the record, so it needs little from the program itself, and a vendor's update should not break what
Silo does for you.
