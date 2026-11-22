# The conversation carried into a fork

## The choice

An agent's part, 22 November 2026. An agent in main has spent the morning on a migration. You fork
main to let it try a risky approach. In the fork you sign in again and run `claude --continue`, and
it goes on with everything it knew.

The options:

- Build it (recommended). Silo checks, for each program it offers, that the conversation is taken up
  again in a copy, and knows the command for it. It costs two copies of one conversation that go on
  apart, in main and in the fork, without knowing of each other. It works only for the programs
  where the check passes.
- Leave it out. The conversation is still on the fork's disk, but nobody checks that a program takes
  it up, and Silo says nothing about it. It costs you finding out program by program, or explaining
  the work again to a fresh conversation.

Recommended: build it. A fork is most useful in the middle of a piece of work, when the agent's
conversation is worth the most.
