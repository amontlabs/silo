# What is a fork given, and can it push code by itself

## The choice

An agent's part, 22 November 2026. An agent finishes a fix in a fork and runs `git push`. Either it
goes through, or it is refused until you allow it. You decided that code leaves a fork through Git
at any time, and this decision says what a fork is given, so it would replace that one.

The options:

- Push needs your yes (recommended). A fork cannot push until you, or a rule you set, allow it. You
  can also bring its commits back to the computer it came from. It costs one more step to get code
  out.
- Push as its parent. A fork can push wherever the computer it came from can, with no step from you.
  It costs the limit on a fork that runs something you do not trust, which can then push too.

Recommended: push needs your yes. A fork is made for risky tries. A rule you write once removes the
extra step for the forks you trust.

The answer also drops read-only access to repositories, which this entry gave every fork. That
follows your decision that no destination is special, and it is part of the question in
`reaching-a-computer`. A fork then reads a private repository with a secret that only reads, which a
rule of yours gives it.

Silo keeps a push from happening by giving the fork no secret that can write. A Git credential kept
as a file on the disk would still be copied, as a sign-in is (`fork-and-sign-in`). The audit's
finding is in the notes of `code-leaves-through-git`.
