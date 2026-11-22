# How are the classifiers and precautions that ship with Silo shipped

## For the owner

An agent's part, 22 November 2026. Silo ships Push check, which holds pushes it finds risky. Before
installing it you open it in Silo. Its list of what it reads and may hold, its rules, how its
computer is made and the code that computer runs are all plain files you can read, as they would be
for an extension someone else wrote.

The options:

- Readable files (recommended). Anyone can check what Silo's own extensions do, and start from one
  to write their own. It costs keeping how they decide in the open: an agent can read how Push check
  decides and try to stay under it, so what it finds stays an opinion, and the limits that must hold
  are rules.
- Shipped ready-built. You see what each one may read and do, but not its code. It costs your trust:
  you take Silo's word for what they do, and nobody can start from them.

Recommended: readable files. Silo's own extensions are then held to the standard of anyone else's,
and Silo does not rely on a check that works only while nobody can read it.
