## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes becomes a line of `plan/decisions.json`.

- Where does the agent do a task: (a) in main, forking when asked, (b) in main, forking on its own, or (c) always in a fork, so main changes only by merge?
  - Recommendation: talk it through. (a) risks a user never forking; (c) makes every chat a task in the loop but puts a merge on small edits.
- Can a user have several mains, one per project or per use such as work and personal, or does one main hold everything?
  - Recommendation: talk it through. Several mains keep one client's secrets away from another's; one main lets the agent see everything from one place.
- Objection to agent-lives-inside: each tool call should run in its own computer, for least privilege, rather than one agent holding every tool in one computer.
  - Recommendation: no. It would need the agent program changed, and a fork per task already bounds what one task holds.
- An agent's memory is the files of its computer, which the user can read and diff; the conversation is disposable and Silo keeps none.
  - Recommendation: yes. A new session in the same computer then picks up from the files, and the diff shows what the agent kept.
- Each computer exists once: another device or person uses it live or forks it, and two diverging copies of one computer never exist.
  - Recommendation: yes. export and fork-from-teammate already make new computers; this rules out syncing one computer between devices.
- When an agent left alone degrades main's own setup, Silo's answer is a diff of main against an older checkpoint, and a restore.
  - Recommendation: yes. Both exist already; the diff only has to compare main with one of its own checkpoints.
- A fork's life has six stages in a fixed order: checkpoint, fork, work and check, note what was done, ask to merge, merge.
  - Recommendation: no. The loop's features already set these steps; a fixed order rules out auto-merge-rules and leaves out drop.
- A named checkpoint never changes once it is saved.
  - Recommendation: yes. Forks, restores and tasks start from it and must get the same thing every time.
