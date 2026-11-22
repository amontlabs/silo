# What load is one device sized for

## For the owner

An agent's part, 22 November 2026. On a MacBook Pro with 32 GB, 16 agents work in main across two
repositories. On a MacBook Air with 16 GB, three agents work in main and one fork tries a migration.

The options:

- Two loads, on 16 GB and on 32 GB (recommended). Silo is tested on both, and each Mac gets a
  maximum that was measured. It costs two loads to measure for each release, and on 16 GB only a few
  agents.
- One load, the heavy one on a large Mac. It costs the smaller Mac: nothing says how Silo does
  there, and its maximum there is a guess.

Recommended: two loads. Many developers work on a 16 GB Mac, and the heavy case is the one you set
for those who run many agents.
