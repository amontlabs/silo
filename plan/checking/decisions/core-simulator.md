# How the core is checked

## Sources

- [sled simulation guide](https://sled.rs/simulation.html): the simulator as a recipe.

## Found

A trial on 21 November 2026 built a small core in Zig 0.17.1 and a simulator for it, on
polarzero's Mac (Apple M4 Max). The core holds the small model of access that `model-tool` is tried
on: five tables and seven calls. The code is thrown away, as every trial's code is.

The proposed answer holds up.

- The simulator makes a history of calls from a seed and checks every property after every step.
- It crashes the engine on purpose in two places: between two calls, and after the core answered
  but before its answer was kept.
- One million histories ran in 2.66 s with no failure. That is 32.5 million steps.
- Two faults were put into the core on purpose. The simulator found the first within 3 histories
  on average and the second within 11.
- A failing history is reduced by removing inputs, until removing any one more makes it pass.
  Both faults came down to the smallest history that shows them: one call, and two calls.
- The same seed gives the same history, the same failure and the same reduced history.
- The core is 322 lines, the simulator 201 and the properties 92.

Zig has its own fuzzer since 0.16. On the same core it found the two faults in 28 and 414 runs.
A fuzz test that fails exits with 0, so a check that uses it has to read its output.

The timings were taken while other trials ran on the same Mac, at a load average of 70 to 183.
Read them as upper bounds.

Not checked: drivers, fakes and their faults (`simulator-faults`), a log written in part, and
Linux.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Fixed seeds for random tests.** Seed every randomized test so a failure replays. The simulator's
  trial replays from a seed. This makes it the rule for every randomized test.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/trie/README.md?plain=1#L330))
