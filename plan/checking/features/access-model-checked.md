# Access checked on every short history

## The choice

An agent's part, 22 November 2026. The part of Silo that decides who may get what is the first code
written: which computer holds which secret and may reach which host. A test runs it on every
possible sequence of a few calls, among three computers, two secrets and two hosts, and checks after
each call that nothing was granted except by you or by a rule you set.

This is what `model-of-access-first`, which you took, already asks for. Taking it is a yes to
building it when its stage comes.

The options:

- Build it (recommended). Every short history is tried, not a sample, so a sequence such as fork,
  grant, then restore that would let a secret into a fork is found before a release. It costs time
  at the start: this part is written and checked before the rest of Silo is built on it, and it must
  stay small enough for a person to read.
- Leave it out. It costs the strongest check of the promise people care most about, and it goes
  against a decision you took.

Recommended: build it. A wrong grant cannot be taken back, so the code that decides grants is where
checking every case pays most.
