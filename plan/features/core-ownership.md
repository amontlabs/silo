## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- Who writes an engine feature's notes: the owner, an agent whose notes the owner reads before `plan start`, or an agent alone, as today? (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: talk it through. The middle option keeps the owner in the core's design for the cost of one read per feature.
- AGENTS.md rule 4 lets the orchestrator merge alone; instead, a change to the engine core or to code that guards secrets merges only once the owner has read it. (`AGENTS.md`)
  - Recommendation: talk it through. Tests cannot see a hidden backdoor, but every core change then waits on the owner's reading.
- ORCHESTRATOR.md rule 2b lets the orchestrator or judge settle what a task leaves open; instead, the owner decides the core's tables, inputs, events and effects. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: talk it through. These are the costliest choices to redo, and most come up while the notes are written, when the owner is there.
- The owner writes the statement of each property, and agents write the generators and the code that satisfy it. (`AGENTS.md`)
  - Recommendation: yes. A property is a sentence or two, and a wrong one passes every test written from it.
- For code that guards secrets, rules and approvals, an agent's prototype is thrown away and the owner writes a version they fully understand. (`AGENTS.md`)
  - Recommendation: no. It puts the owner's hands on security code first while security is not the focus (security-not-the-focus).
- The tree keeps the files the owner reads, the core and its interfaces, in directories of their own. (`AGENTS.md`)
  - Recommendation: yes. Free at the first commit and costly later, and useful even if the owner reads only the interfaces.
- Where the tests already cover a behaviour, agents prefer code the owner reads quickly over extra compile-time guarantees. (`AGENTS.md`)
  - Recommendation: talk it through. Readable code serves the owner's review, but compile-time checks are part of why the engine is in Zig.
- A dependency, or code the owner cannot judge, sits behind a narrow interface of Silo's own, so it can be replaced whole. (`AGENTS.md`)
  - Recommendation: yes. It costs one boundary per dependency and limits what a bad one can reach.
- A list names the engine areas, such as the state machine, where agents may not debug without the owner. (`AGENTS.md`)
  - Recommendation: no. The build never stops to ask the owner (rule 2b), so those areas would stall.
- Each engine design decision names the person with systems experience who reviewed it, or the property or simulator run that stands in. (`plan/decisions.json`)
  - Recommendation: no. The owner works alone, so nearly every entry would cite a property, which properties-checked already asks for.
