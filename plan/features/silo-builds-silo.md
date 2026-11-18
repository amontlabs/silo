## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- Once the starter works, the agents that build Silo run in Silo computers, and what breaks for them is fixed before other work. (`plan/decisions.json`)
  - Recommendation: yes. The starter exists for daily use, and this makes building Silo that use.
- Does the first release go out rough with a demo, or only with features that work end to end and look cared for, the rest cut and listed? (`plan/decisions.json`)
  - Recommendation: talk it through. A rough release reaches users sooner, and a cut one leaves fewer features to support.
- A fresh agent with only Silo's docs tries each new or changed command, and any second try or confusing error is a defect. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. Agents are Silo's main users, so their confusion is the product's bug list.
- The rules use different words for the agent that builds Silo and the agent program that lives in a Silo computer. (`AGENTS.md`)
  - Recommendation: yes. Once Silo is built in Silo, both run in one computer, and a command can reach either.
- Which scanner checks Silo's dependencies before each release, and does it run in the whole check or only at release? (a check in `tools/ci/check.sh`)
  - Recommendation: talk it through. A company has blocked a similar product over a few flagged packages; scanning every merge costs time on each.
- An agent files a problem with what it saw, what it expected, the agent program it used, and the steps to repeat it. (`AGENTS.md`)
  - Recommendation: no. WORKER.md's report already has a section for broken things, and a form adds headings to every report.
