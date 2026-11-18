## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- Are the checks (lint rules, the simulator, the live-check harness) built before the first engine feature, or alongside, as the plan's order has them? (`plan/decisions.json`)
  - Recommendation: talk it through. The plan puts properties-checked last in the shell stage; adding checks later has failed before, but checks first delay usable code.
- Is an engine feature built docs first (user docs, the interface, black-box tests, then code), or research first, then a plan, then code with its tests? (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: talk it through. Docs first give the owner something to read early; trying both on one feature would settle it.
- Decision one-feature-at-a-time builds the engine before the app; instead, the app is built and polished first, to show what the engine must provide. (`plan/decisions.json`)
  - Recommendation: no. The app holds no logic (mac-app-in-swift), and a fake engine client lets screens start early anyway.
- Engine behaviour is tested through the client the command line uses, and tests inside the Zig core cover only what that client cannot reach. (`AGENTS.md`)
  - Recommendation: yes. The app holds no logic, so this tests what users meet without launching the app.
- Every component, the app included, gets a way for an agent to run it and read its output before features are built on it. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. An agent that cannot see what its code does can only guess.
- Experiments and first tries of an integration run in a throwaway branch, in whatever is fastest, and only what is needed moves into the engine. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. Worktrees make a throwaway branch free, and engine-in-zig keeps the result in Zig.
- For an open stack or design choice, an agent builds the same small thing in each option, and they are reviewed side by side. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. It settles a choice on evidence for the price of one small build per option.
- For a feature under discussion, a throwaway demo is shown to the owner before its notes are settled. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: talk it through. The owner sees something real early, but each demo is code built twice.
- After each stage of the plan, a cleanup pass runs against a written description of a good codebase, with no feature work mixed in. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: talk it through. It keeps debt from compounding, but it is a pause the plan's order does not have.
