# How a fork becomes main

## For the owner

An agent's part, 22 November 2026. A fork of main has spent the morning moving your project to Node
24, and it works. Meanwhile an agent in main committed a fix in `~/code/api` and did not push it.
You make the fork main.

The options:

- The fork takes main's place whole (recommended). Silo first shows the fix as only in the old main,
  so you can bring it over. Then the old main stops and is kept aside, and the fork gets main's
  name, rules, secrets, budget and grants. It costs the work in progress of the agents still running
  in main: they stop, and you start them again in the new main.
- The fork becomes main with a fork's limits. It has no secrets and cannot push until you give each
  one again. It costs a round of grants at each switch, and a main that cannot do its daily work
  until you finish them.

Recommended: the fork takes main's place whole. You make the switch yourself, after seeing what
would be left behind, so the new main can be trusted as the old one was.

## Notes

In force already: a fork ends in exactly one outcome (`how-a-fork-ends`), and when it becomes
main the old main is kept aside (`old-main-kept-aside`).

Part of the question:

- The kind of that action: granting, destructive or protective. The kind decides who may trigger
  it (`kind-before-design`).
