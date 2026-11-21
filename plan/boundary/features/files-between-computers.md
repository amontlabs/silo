# Files sent between computers

## The owner's words (21 November 2026)

> A file channel between computers and from the user machine to a computer indeed would be a good
> feature. Between computers I believe there could be a tool, and this channel should be able to
> be controlled with rules (as in user might want to allow exchange between computer A and B, from
> A to B, with multiple groups, etc.).

The need: people who run agents on several machines move files between them by hand.

## An agent's notes

- Each file that crosses is a request at the boundary, and a rule allows, holds or denies it. With
  no rule, nothing crosses: no channel exists by default.
- The plan has no word for a group of computers. One is needed before a rule names a group.
- The tool the agents use is not designed. `what-an-agent-inside-may-do` binds it: a computer cannot
  get more than it has, so the rule comes from you.
- Silo sees each crossing from outside, so it is a fact in the record of both computers.
- Two cases are already in the plan. Code moves as a commit (`linux-beside-a-macos-main`,
  `code-leaves-through-git`). Chosen files go from a fork to its parent before a drop
  (`before-a-drop`, `keeping-from-a-fork`). This entry is the general case, between any two
  computers.
- `what-silo-does-not-build` proposes that Silo "passes no messages between agents". A file can
  carry a message. An agent's reading: Silo carries the file and neither reads it nor chooses where
  it goes. The owner has not ruled on this.
- Bad paths to design: the receiving computer is paused, stopped or dropped. Its disk is full. The
  file changes while it is copied. A file of the same name is already there. A hold ends with no
  answer.
- Stage: placed in the app's stage beside `ways-out-shown` by an agent. The owner said it "would be
  a good feature" and did not say when.
