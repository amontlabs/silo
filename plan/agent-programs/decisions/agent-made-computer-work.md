# How does an agent's work in a computer it asked for reach the user

## The choice

An agent's part, 22 November 2026. The agent in main ran the tests on a clean Linux computer it
made, and tried a migration in a fork. Now it needs the results, and you want to see what happened.

The options:

- Through receipts and Git (recommended). The agent reads the receipt of the test task: what ran,
  how it ended, what it printed. It brings the fork's commits back to main through Git. You see both
  computers in Silo's window, under the agent that made them. It costs everything else: a file the
  tests produced, such as a coverage report, comes back only through a rule you set for files
  between computers.
- A shared folder. The agent that asked can read and write the files of the computers it made. It
  costs a crossing between computers that is open by default, while everything else between
  computers passes only by a rule you set.

Recommended: through receipts and Git. They carry what an agent needs back from a test or a try, and
nothing crosses between computers that you did not set.
