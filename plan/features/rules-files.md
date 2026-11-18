## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- A mistake agents repeat becomes a check (a lint, a test or a property), and the rule it replaces leaves the rules file. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. It keeps the rules short, and a check cannot be skimmed past.
- A line enters a rules file only with the failure it would have prevented, named in the commit that adds it. (`AGENTS.md`)
  - Recommendation: yes. Rule 2b already argues from counted failures; this asks it of every line.
- A rule an agent proposes, from a lesson, a memory or a report, enters the rules files only with the owner's yes. (`AGENTS.md`)
  - Recommendation: yes. Today a branch that changes .atelier/ merges on the orchestrator's approval alone.
- At each new model, are the rules pruned by rereading them line by line, or by one small behaviour test per rule, run in a fork? (`AGENTS.md`)
  - Recommendation: talk it through. A reread is cheap and fits short rules; tests prove each rule but are a suite to keep.
- A doc says what the code does now and never how it changed, which is the commits' job. (`AGENTS.md`)
  - Recommendation: yes. The owner's global rule already says this for code comments, and docs drift into changelogs the same way.
- Prompts and rules for agents live in their own files, never as strings inside code. (`AGENTS.md`)
  - Recommendation: yes. A prompt inside a string escapes every reader and every review of the rules.
- The first line of AGENTS.md says what agents optimise for: code that is easy to maintain and the smallest change that works. (`AGENTS.md`)
  - Recommendation: no. A general aim changes little that agents do, and it cites no failure it prevents.
- Agents may shorten and rewrite the docs written for agents, as long as the behaviour tests do not regress and no idea is lost. (`AGENTS.md`)
  - Recommendation: no. The owner keeps these files short by hand, and an earlier line asks for their yes on each change.
