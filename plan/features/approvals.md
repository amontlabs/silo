## Decided

- 2026-11-10: approvals are native to Silo: shown in the Silo app, two-way with the agent, not a prompt of the agent's harness. No broker agent.

## Proposed

- What always holds
  - Property: No sequence of calls, crashes, reloads or upgrades turns a pending request into an allowed one without the user's allow; the simulator checks it with crashes.
  - Property: An allow covers exactly what was shown, and no longer applies if that changes afterwards.
  - Property: A pending request holds only the call that asked, so its computer keeps running and no other fork waits behind it.
  - Property: A fork that waits for an approval is kept however long the answer takes; nothing expires and no work is thrown away (the old code expired a request after seven days).
  - Property: An approval request appears only in Silo's own surface, and nothing an agent or a web page can draw looks like one.
  - Property: An approval reaches the app whichever session, agent or background job raised it, and answering it resumes that same run with no manual resume and no second run.
- Asking and answering
  - Rule: The agent states what it wants and gives its arguments; the engine walks the steps of stage, approve, apply and recover.
  - Next to its buttons, an approval shows the exact effect of the stored call (the command, the change, the message as it will be sent) and the reason for it.
  - Rule: An approval names what it widens and is shown for exactly that, so there is no standing broad grant.
  - Rule: No model decides a widening of access or any security question.
  - Rule: A cheap model may only label a request (routine or needs a look, thrashing or not), and the label is shown as a guess.
  - The first time a new kind of approval request appears, the app says so plainly.
  - Test: An approval is raised from a background job; it appears in the app, and answering it continues the same run.
- Options
  - An approval for an action that leaves the computer can require a second factor where the user wants one.
  - An allow can carry an expiry and is then valid only inside that time window.
- Other
  - A payment above a limit set per computer can be one of the uses a rule makes wait for you, if Silo can recognise it.

## Open

- Does an approval only answer a request for more access, or can a rule make each use wait for you (a push, a deploy, a message sent)?
  - Suggested: Let a rule say 'ask each time', built with `github` and `secrets`: release and deploy then need no kind of their own, as `security-framing` asks.
- Can a chat app ever answer an approval, or does it only notify and lead back to Silo?
  - Suggested: Only notify: the note of 10 November keeps approvals in Silo's own app, and an agent may be able to write in the same chat.
- To find out: Read the Smithers 0.28 changelog for how it binds each piece of work to the exact evidence that authorized it.

## From the old code

The old code had it built and driven end to end, about 7,500 lines, and its approach can be reused.

Lessons:
- The summary must come from the call's parameters, never from the agent's text.
- Approvals are answered on the device whose computer asked.
- Write the summary from the call's parameters, never the agent's words, and strip control characters and direction overrides from anything the agent chose.
- An allowed call is checked again as if just made, then run from the stored copy.
- Limits: 30 an hour, 20 waiting, seven-day expiry; a secret value is removed from the file when the approval ends.
- Summaries are written by the engine from parameters, never from the agent's words; agent text is stripped of control and direction characters.
- An allow is recorded first and never re-run on its own after a crash.
- Deny on a merge approval must close the review request too, or it comes back as its own row.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/approvals.rs`
- `app/SiloUI/src-tauri/silod/src/methods/approvals.rs`
- `app/SiloUI/src-tauri/silod/src/methods/approvals/tests/approvals_mbt.rs`
- `docs/direction/evidence/V.txt`
- `docs/SiloUI-POLICY.md#L92`
- `app/SiloUI/src-tauri/engine/src/approvals.rs`
- `app/SiloUI/src-tauri/engine/src/policy/summary.rs`
- `app/SiloUI/cli/src/approvals.rs`
- `docs/SiloUI-POLICY.md#asking-you`
- `docs/direction/evidence/T5a.txt`
- `docs/SiloUI-CLI.md#approvals`
- `app/SiloUI/cli/tests/approvals_wait.rs`

Calls: `approvals.list`, `approvals.get`, `approvals.decide`, `approvals.cancel`, `approvals.note`, `notifications.list`, `silo approvals`, `silo approvals show`, `silo approvals allow`, `silo approvals deny`, `silo approvals status`, `silo approvals wait`, `silo approvals note`, `changes.requests.*`.
