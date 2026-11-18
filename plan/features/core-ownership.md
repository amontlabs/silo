## Proposed

- A property's statement is written by a person. Agents write the generators and the code that satisfy it.
- The core and its interfaces sit in directories of their own, so they can be read apart from the rest.
- A dependency sits behind a narrow interface of Silo's own, so it can be replaced whole.

## Open

- How much of the core goes through a person?
  - Its design: the tables, inputs, events and effects, settled while a feature's notes are written. These are the costliest choices to redo.
  - Its changes: a change to the core, or to code that guards secrets, merges only once a person has read it. Tests cannot see a hidden backdoor, but every core change then waits on that reading.
  - Neither: agents design and merge, as now.
- Where the tests already cover a behaviour, does code that reads quickly win over extra compile-time guarantees? Compile-time checks are part of why the engine is in Zig.
