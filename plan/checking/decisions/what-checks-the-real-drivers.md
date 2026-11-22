# What checks the real drivers and the real boundary

## For the owner

An agent's part, 22 November 2026. This entry is about how Silo is checked, so agents may take it,
as `owners-part-on-engine` allows. You need not read it.

The decision the agents took says: "The virtual machine engine and the network stack are checked by
hostile tests on the real thing." "Engine" is the word in force for Silo's own central program, and
"the VM engine" has already been read as that program: the notes of `engine-patch-review` record "a
patch to the engine" that meant libkrun. Read that way, this decision says Silo's engine is tested
only on the real thing, while the simulator checks its core.

The options:

- Name the real programs (recommended). The sentence says "the virtual machine monitors that run the
  computers, libkrun and Apple's Virtualization framework". A virtual machine monitor is the standard
  name for a program that runs virtual machines. Apple's framework runs macOS computers, so it is
  named beside libkrun. It costs nothing: the checks are the same.
- Keep the taken wording. It costs a word read as Silo's engine, in a plan where "engine" has one
  meaning.

Recommended: name the real programs.

## Notes

The same words change in the entries with no verdict, in the same change: the question of
`linux-vm-engine`, the answer of `core-dependencies`, the question and answer of
`engine-patch-review`, the `what` of `hostile-tests-on-the-real-thing`, and
`plan/engine/README.md`.
