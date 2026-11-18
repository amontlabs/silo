## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes becomes a line of `plan/decisions.json`.

- Where does the user direct agents: (a) one chat with the agent in main, (b) a chat with a thread per fork, or (c) the app, beside each fork's terminal and diff?
  - Recommendation: talk it through. (a) follows agent-lives-inside, and the app keeps the diff and approvals whichever is picked.
- Silo builds no chat window of its own; the user talks to their agent program in a terminal inside the computer, or through T3 Code.
  - Recommendation: yes. The vendors build the chat, and agent-lives-inside already puts the agent in a terminal in main.
- The command line and what an agent sees of Silo are polished before the app's screens.
  - Recommendation: talk it through. Agents use Silo most, but the owner reads diffs and answers approvals in the app every day.
- The app serves both ways of working in one window: steering a few sessions live, and leaving forks alone to read their diff later.
  - Recommendation: yes. Both are daily use, and a separate monitoring tool would be one more thing to run.
- Silo builds into the app every view it needs, and leaves one to the user's agent only if the agent gets it right first time.
  - Recommendation: yes. Generated views half work, take several rounds and spend the user's tokens.
- Each running agent session has one place where it is prompted and one where it is stopped, and the app names both.
  - Recommendation: yes. With terminals, T3 Code and many forks, it is easy to lose which window holds a session.
- The app keeps few screens while their design still changes.
  - Recommendation: yes. Each screen is work to redo while the shape of the loop still moves.
