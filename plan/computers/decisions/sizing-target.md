# What load is one device sized for

## The choice

An agent's part, 25 November 2026, from the challenge of #361, with #350.

The case: an iOS developer runs 16 agents in main on a 32 GB Mac, and each checks its work in a
booted simulator. Each holds about 3.4 GB (measured by the xcfs project on a Mac, not by Silo), so four
already take 13.6 GB before any build, and 16 do not fit. The two-VM limit rules out a computer per
simulator.

The options:

- Add the simulators as a load of their own: a few booted, shared by many agents, with the maximum
  shown (recommended). It costs agents waiting for a simulator, which the open draft
  `simulators-for-parallel-agents` (#361) settles. Mac kit test 25 measures how many fit.
- Keep two loads counted in agents, as the earlier answer says. It promises a heavy load an iOS
  developer cannot reach.

Recommended: add the simulators. #350 also asks that main plus one and two Linux forks be measured on
a 16 GB Mac before the common load is promised; the answer's last sentence already requires it.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

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

## For an iOS developer

An agent's challenge, 24 November 2026. The heavy load, 16 agents in main on a 32 GB Mac, does not
fit an iOS developer: an agent that checks its work wants a booted simulator, and each holds about
3.4 GB (measured by the xcfs project on a Mac, not by Silo). Four booted simulators already take
13.6 GB, before any build. What fits is many agents sharing a few simulators, with a maximum booted
at once, as `simulators-for-parallel-agents` proposes. The load to measure for this user is then
the number of simulators, not of agents. Mac test 25 of `research/2026-11-24-mac-and-ios-developers/README.md` measures it in a computer.
