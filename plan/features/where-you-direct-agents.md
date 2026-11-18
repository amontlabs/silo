## Proposed

- Silo builds no chat window. The user talks to their agent program in a terminal inside the computer, or through T3 Code.
- The app serves two ways of working in one window: steering a few sessions live, and leaving forks alone to read their diff later.
- Each running agent session has one place where it is prompted and one where it is stopped, and the app names both.
- Silo builds into the app every view it needs. A view is left to the user's agent only if the agent gets it right first time.
- The app keeps few screens while their design still changes.

## Open

- Where does the user direct agents? The app keeps the diff and the approvals whichever is picked.
  - One chat with the agent in main.
  - A chat with a thread per fork.
  - The app, beside each fork's terminal and diff.
- Are the command line and what an agent sees of Silo polished before the app's screens? Agents use Silo most, but diffs are read and approvals answered in the app every day.
