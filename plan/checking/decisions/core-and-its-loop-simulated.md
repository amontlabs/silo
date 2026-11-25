# How the core is checked

## The choice

An agent's part, 25 November 2026, from #352 (files 01 and 03). `core-simulator` was taken by agents, so this
replacement is the agents' to take with a judge (`owners-part-on-engine`), and polarzero confirms it
or not.

The case: the engine crashes between writing a checkpoint's record and telling the core it is done.
The core touches nothing, so the earlier answer's simulator never sees that crash: it happens in the
loop that feeds the core. And a liveness property, such as "what is wanted and what is seen agree
within N steps", cannot hold after every step while faults keep coming.

The options:

- Simulate the core and its loop, and check liveness once faults stop (recommended). It costs the
  loop's reach to the disk, the clock and the drivers going through interfaces given at start.
- The core alone, every property after every step, as the earlier answer says.

Recommended: the core and its loop. `liveness-has-a-number` already requires each liveness property
to have its number, and this is where that number is checked.
