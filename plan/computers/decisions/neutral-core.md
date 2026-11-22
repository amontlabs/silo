# Does the engine's core know which system a computer runs

## For the owner

An agent's part, 22 November 2026. Apple raises its limit from two macOS computers running on a Mac
to four, or Silo adds a third system later. Something in Silo has to change.

The options:

- The part that decides never names a system (recommended). The part of Silo that decides for every
  computer works the same for macOS and Linux. What differs, such as Apple's limit, comes as a
  number from the part that runs each system, so Apple's change touches one place. It costs design
  up front: every difference has to be said as a value of that kind, and one that does not fit has
  to be thought through before it is added.
- The part that decides knows each system. Differences are written where decisions are made. It
  costs reach: a change for one system touches the part that decides for every computer, and its
  tests run again for both systems.

Recommended: never names a system. It keeps the two systems the same wherever the platform does not
force a difference, as you already decided, and keeps the part that decides small enough to check.

## Notes

"Core" names both the engine and the pure function inside it. `engine-shape` says which. Here it
is the core inside the engine.
