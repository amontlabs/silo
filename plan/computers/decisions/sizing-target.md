# What load is one device sized for

## The choice

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

## Simulators per agent, against the memory main is given (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-checkpoint-retention/` (section
"Simulators per agent"), a model and a simulation on Linux, nothing measured on a Mac.

- **Main's memory is set when it starts.** Apple's framework gives a macOS computer a fixed size,
  and whether it gives any back is to find out (`memory-given-back`). So everything the agents run
  in main, simulators included, fits in what main was given before the night.
- **What fits.** With the Mac keeping 6 to 10 GB for itself and one Linux computer beside main, a
  booted simulator between 0.9 GB (one vendor's figure) and 3.4 GB (xcfs), and one to three Xcode
  builds at about 4 GB each at their peak: on a 16 GB Mac, main has about 8 GB and no simulator
  fits beside a build. On 32 GB, 4 agents get 2 booted simulators at 3.4 GB (8 at 0.9 GB), and 16
  agents with three builds at once do not fit before any simulator. 16 agents get 2 to 7 on 48 GB
  and 6 to 19 on 64 GB.
- **How many 16 agents need.** Agents that think and edit for about 3 minutes, build for 1, and run
  the app for 1.5 minutes on half their turns: with a simulator held per use and kept booted until
  another agent needs the place, 6 booted keep the wait under 30 s for 95 uses in 100, and 4 cost
  each agent 7% of the night waiting. With a simulator held for the whole session, every agent
  beyond the maximum waits all night.

A challenge to the answer, on that case: "about 16 agents in main, on a Mac with 32 GB" does not
hold for an iOS developer, with or without simulators, because three Xcode builds at once already
take most of what main gets. A proposal: for iOS work, the heavy load is 16 agents with 6 booted
simulators on a 64 GB Mac, and the common one 4 agents with 2 booted on 32 GB; Mac test 25 of
#361's kit measures both, with the numbers of the model as what it confirms or corrects.
